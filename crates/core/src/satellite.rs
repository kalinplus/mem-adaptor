//! Satellite issuance, registration, resolution, and relocation detection for home mode (DEC-20 items 3-6).
//! Sits between the CLI's home-mode orchestration and the engine: the engine keeps consuming `SatelliteSpec`
//! and never learns the `.mem-adaptor/` layout, while this module owns that layout, the registry TOML, the
//! deterministic short-ID derivation, and the pure relocation check.
//! Registration is append-only after an approved apply (DEC-11: planning never writes); a corrupt registry or
//! receipt is a refused execution basis, not a silent fallback to first-run behavior.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail, ensure};
use data_encoding::BASE32_NOPAD;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::Result;
use crate::governance::{Config, SatelliteEntry};
use crate::plugins::{Reader, Registry, SourceKind};
use crate::reports::ReceiptReport;

/// Domain-separation prefix for satellite ID derivation; it keeps these hashes distinct from record identities.
const DERIVE_PREFIX: &str = "mem-adaptor:satellite:v1";

/// Matched-record floor below which a small source never triggers relocation detection (DEC-20 item 6).
pub const RELOCATION_FLOOR: usize = 5;

/// Matched ratio at or above which a source round is suspected to be a moved satellite or a reused path.
pub const RELOCATION_RATIO: f64 = 0.6;

/// Control directory inside a home; the registry and receipt chains live here and travel with the home.
const CONTROL_DIR: &str = ".mem-adaptor";

/// Derives the candidate satellite ID from an already canonicalized absolute source path (DEC-20 item 3).
/// The first five digest bytes become eight lowercase base32 characters. No timestamp participates, so the
/// plan and apply phases derive the same candidate for the same path and the plan digest stays stable.
/// Symlinks resolved to the same directory derive the same ID because callers canonicalize first.
pub fn derive_id(canonical_path: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(DERIVE_PREFIX.as_bytes());
    hash.update([0]);
    hash.update(canonical_path.as_bytes());
    BASE32_NOPAD
        .encode(&hash.finalize()[..5])
        .to_ascii_lowercase()
}

/// Checks the issued satellite ID shape: exactly eight lowercase base32 characters, no look-alike digits.
pub fn valid_satellite_id(id: &str) -> bool {
    id.len() == 8
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
}

/// Location of a home's policy and satellite registry file (DEC-19).
pub fn home_config_path(home: &Path) -> PathBuf {
    home.join(CONTROL_DIR).join("config.toml")
}

/// Root of the home's receipt chains; `init` creates it so the layout is visible before the first apply.
pub fn receipts_root(home: &Path) -> PathBuf {
    home.join(CONTROL_DIR).join("receipts")
}

/// Location of one satellite's receipt chain inside the home (DEC-19, DEC-20 item 7).
pub fn receipts_dir(home: &Path, satellite_id: &str) -> PathBuf {
    receipts_root(home).join(satellite_id)
}

/// Files this round's receipts inside the home, one chain per satellite ID, and returns the written paths.
/// The satellite ID in each receipt decides the directory, so a receipt can never be filed under another
/// satellite's chain; a receipt without a satellite cannot belong to a home run and is an error rather than a
/// silently dropped governance record. Writing is atomic per file, but a failure partway through leaves the
/// already written receipts in place, which the caller must report as a partially completed run.
pub fn save_receipts(home: &Path, receipts: &[ReceiptReport]) -> Result<Vec<PathBuf>> {
    let mut saved = Vec::new();
    for receipt in receipts {
        let satellite = receipt
            .source
            .satellite
            .as_ref()
            .context("Home mode cannot file a receipt without a satellite ID")?;
        let directory = receipts_dir(home, &satellite.id);
        let relative = format!("{}.json", receipt.run_id);
        // `previous: None` makes the write noclobber: a run ID is unique, so an existing file with the same
        // name means a colliding or replayed run rather than a receipt this call may replace.
        crate::writer::atomic_file(
            &directory,
            &relative,
            &serde_json::to_vec_pretty(receipt)?,
            None,
        )?;
        saved.push(directory.join(relative));
    }
    Ok(saved)
}

/// Reads and validates a TOML configuration; a missing file is `None`, a corrupt or invalid file is an error.
/// Used for both the home registry and the injectable direct-mode user configuration.
pub fn read_config(path: &Path) -> Result<Option<Config>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| anyhow::anyhow!("Configuration is not valid UTF-8"))?;
    let config: Config = toml::from_str(&text)
        .map_err(|error| anyhow::anyhow!("Invalid configuration TOML: {}", error))
        .with_context(|| crate::gate::mask(&path.to_string_lossy()))?;
    crate::schema::validate("config", &config).with_context(|| {
        format!(
            "Configuration violates the config schema: {}",
            crate::gate::mask(&path.to_string_lossy())
        )
    })?;
    Ok(Some(config))
}

/// Validates and replaces one configuration file through a temporary file in its own directory.
/// Callers preserve the existing registry before rewriting; this helper neither merges nor appends.
pub fn write_config(path: &Path, config: &Config) -> Result<()> {
    crate::schema::validate("config", config)?;
    let text = toml::to_string(config)
        .map_err(|error| anyhow::anyhow!("Configuration cannot be serialized: {error}"))?;
    let parent = path.parent().context("Invalid configuration path")?;
    fs::create_dir_all(parent)?;
    // NamedTempFile creates owner-readable files (0o600 on Unix), matching receipt permissions: the registry
    // records private source locations.
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

/// Source shape and system detected from Reader claims, without parsing any record.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDetection {
    /// Canonicalized source location; for a ZIP archive this is the archive itself, not the extraction.
    pub canonical_path: PathBuf,
    /// Single claiming Reader ID, `mixed` when several claim, or `unknown` when none does.
    pub system: String,
    pub kind: SourceKind,
}

/// Detects the source system and shape by loading the inventory and collecting Reader claims (DEC-20 item 4).
/// The declared `source_kind`, not the on-disk shape, decides whether a source is an export bundle.
/// Source loading errors propagate; an unclaimed source is reported as `unknown` rather than refused here,
/// because the engine separately reports an empty or unrecognized inventory.
pub fn detect_source(readers: &[Box<dyn Reader>], path: &Path) -> Result<SourceDetection> {
    let source = crate::source::load_source(path)?;
    let mut claiming: Vec<(&str, SourceKind)> = Vec::new();
    for reader in readers {
        if !reader.claim(&source.files).is_empty() {
            claiming.push((reader.id(), reader.source_kind()));
        }
    }
    let system = match claiming.len() {
        0 => "unknown",
        1 => claiming[0].0,
        _ => "mixed",
    };
    let kind = if claiming
        .iter()
        .any(|(_, kind)| *kind == SourceKind::ExportBundle)
    {
        SourceKind::ExportBundle
    } else {
        SourceKind::Directory
    };
    Ok(SourceDetection {
        canonical_path: source.root,
        system: system.into(),
        kind,
    })
}

/// A satellite identity ready to be issued; the registry append happens only after an approved apply.
#[derive(Debug, Clone, PartialEq)]
pub struct SatelliteCandidate {
    pub id: String,
    pub label: String,
    pub system: String,
    /// Directory satellites bind their canonical path; export bundles register no path.
    pub path: Option<String>,
}

/// Outcome of satellite resolution for one run, before any registry write (DEC-20 item 5).
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Continue a registered satellite. `rebind` means the registry path binding differs from the current
    /// source path and must be converged after an approved apply; `label` is the effective display label.
    Registered {
        id: String,
        label: String,
        rebind: bool,
    },
    /// Issue a new satellite; registration happens only after an approved apply (DEC-11).
    New(SatelliteCandidate),
    /// The current path is unregistered and derived a candidate; the caller must compare this round's source
    /// fingerprints against the registered satellites' latest receipts and resolve again with that result.
    NeedsDetection(SatelliteCandidate),
    /// Detection found a suspected move or path reuse. An interactive caller must let the user choose between
    /// rebinding and continuing as a new satellite; a non-interactive caller must refuse (DEC-20 item 6).
    /// `id_occupied` records that the derived ID already belongs to another satellite, which forbids issuing
    /// it again: choosing a new satellite then fails instead of silently renumbering.
    Suspected {
        candidate: SatelliteCandidate,
        suspects: Vec<RelocationCandidate>,
        id_occupied: bool,
    },
}

/// Resolves the satellite for this run in the pinned order: explicit `--satellite` > registry path match >
/// derived candidate (DEC-20 item 5). `suspects` is the relocation-detection result: `None` means detection
/// has not run yet, so an unregistered path returns `NeedsDetection` instead of guessing.
/// Pure and deterministic: it reads no files, writes nothing, and never renumbers an issued ID.
/// An explicit `new` is itself the user's choice to continue as a new satellite, so it skips detection and is
/// only constrained by ID and path uniqueness; export bundles always require an explicit satellite.
pub fn resolve(
    entries: &[SatelliteEntry],
    explicit: Option<&str>,
    label: Option<&str>,
    detection: &SourceDetection,
    suspects: Option<&[RelocationCandidate]>,
) -> Result<Resolution> {
    if let Some(label) = label {
        ensure!(!label.is_empty(), "Satellite label must not be empty");
    }
    let path = detection.canonical_path.to_string_lossy();
    ensure!(
        crate::gate::mask(&path) == *path,
        "Sensitive source path cannot be registered or reported"
    );
    // Only directory satellites carry a rebindable path; a bundle's download location is not an identity.
    let effective_path = (detection.kind == SourceKind::Directory).then(|| path.into_owned());
    let default_label = default_label(detection);
    let chosen = |fallback: &str| label.unwrap_or(fallback).to_owned();

    match explicit {
        Some("new") => {
            if let Some(path) = effective_path.as_deref()
                && let Some(existing) = find_by_path(entries, path)
            {
                bail!(
                    "This path is already registered to satellite {} ({}); use --satellite {} to continue that chain",
                    existing.id,
                    existing.label,
                    existing.id
                );
            }
            let candidate = candidate(detection, chosen(&default_label));
            if let Some(existing) = find_by_id(entries, &candidate.id) {
                bail!(
                    "Derived satellite ID {} is already registered to {}; refusing to issue the same ID twice. Use --satellite {} to continue that satellite",
                    candidate.id,
                    registered_location(existing),
                    existing.id
                );
            }
            Ok(Resolution::New(candidate))
        }
        Some(id) => {
            ensure!(
                valid_satellite_id(id),
                "Satellite must be an 8-character base32 ID or \"new\""
            );
            let entry = find_by_id(entries, id).with_context(|| {
                format!(
                    "Satellite {id} is not registered in this home; registered: {}",
                    registered_summary(entries)
                )
            })?;
            // Only a directory source may rebind, and only a satellite that already carries a path binding:
            // a bundle satellite keeps no path, and a bundle source never clears a directory binding.
            let rebind =
                entry.path.is_some() && effective_path.is_some() && entry.path != effective_path;
            Ok(Resolution::Registered {
                id: id.into(),
                label: chosen(&entry.label),
                rebind,
            })
        }
        None => {
            ensure!(
                detection.kind != SourceKind::ExportBundle,
                "Export-bundle sources need an explicit --satellite <ID|new>: their download path changes every export, so no path binding can identify them"
            );
            // The kind check above guarantees a directory source, which always carries a path binding.
            let path = effective_path
                .as_deref()
                .expect("directory sources always carry a path binding");
            if let Some(entry) = find_by_path(entries, path) {
                return Ok(Resolution::Registered {
                    id: entry.id.clone(),
                    label: chosen(&entry.label),
                    rebind: false,
                });
            }
            let candidate = candidate(detection, chosen(&default_label));
            let occupied = find_by_id(entries, &candidate.id).is_some();
            match suspects {
                None => Ok(Resolution::NeedsDetection(candidate)),
                Some(suspects) if !suspects.is_empty() => Ok(Resolution::Suspected {
                    candidate,
                    suspects: suspects.to_vec(),
                    id_occupied: occupied,
                }),
                Some(_) => {
                    if let Some(existing) = find_by_id(entries, &candidate.id) {
                        bail!(
                            "Derived satellite ID {} is already registered to {}; refusing to issue the same ID twice. Use --satellite {} to continue that satellite",
                            candidate.id,
                            registered_location(existing),
                            existing.id
                        );
                    }
                    Ok(Resolution::New(candidate))
                }
            }
        }
    }
}

/// Builds the issuable identity for the current source under the given display label.
fn candidate(detection: &SourceDetection, label: String) -> SatelliteCandidate {
    SatelliteCandidate {
        id: derive_id(&detection.canonical_path.to_string_lossy()),
        label,
        system: detection.system.clone(),
        path: (detection.kind == SourceKind::Directory)
            .then(|| detection.canonical_path.to_string_lossy().into_owned()),
    }
}

/// Derives the default display label from the source location's final name (pinned decision 1: the file name
/// for an export bundle, the directory name for a directory), falling back to a generic label for a root path.
pub fn default_label(detection: &SourceDetection) -> String {
    detection
        .canonical_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "satellite".into())
}

/// Looks up a registered satellite by its issued ID.
pub fn find_by_id<'entries>(
    entries: &'entries [SatelliteEntry],
    id: &str,
) -> Option<&'entries SatelliteEntry> {
    entries.iter().find(|entry| entry.id == id)
}

/// Looks up a registered satellite by its current canonical path binding; export bundles never match.
pub fn find_by_path<'entries>(
    entries: &'entries [SatelliteEntry],
    path: &str,
) -> Option<&'entries SatelliteEntry> {
    entries
        .iter()
        .find(|entry| entry.path.as_deref() == Some(path))
}

/// Appends a newly issued satellite after an approved apply, refusing duplicate IDs and paths (DEC-20 item 3).
/// Uniqueness is guaranteed by this check against every existing entry, not by the derivation's collision odds.
pub fn register(entries: &mut Vec<SatelliteEntry>, entry: SatelliteEntry) -> Result<()> {
    ensure!(
        valid_satellite_id(&entry.id),
        "Satellite ID must be 8 lowercase base32 characters"
    );
    ensure!(!entry.label.is_empty(), "Satellite label must not be empty");
    ensure!(
        !entry.system.is_empty(),
        "Satellite system must not be empty"
    );
    if let Some(existing) = find_by_id(entries, &entry.id) {
        bail!(
            "Satellite {} is already registered to {}; refusing to issue the same ID twice",
            entry.id,
            registered_location(existing)
        );
    }
    if let Some(path) = &entry.path
        && let Some(existing) = find_by_path(entries, path)
    {
        bail!(
            "Path {} is already registered to satellite {}",
            crate::gate::mask(path),
            existing.id
        );
    }
    entries.push(entry);
    Ok(())
}

/// One source record's identity and body evidence, the only inputs relocation matching may compare.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fingerprint {
    pub source_record_id: String,
    pub content_hash: String,
}

/// Reads this round's records and projects their distinct identity/body evidence for relocation matching.
/// Detection runs before the satellite is resolved, so no satellite identity is bound here; that is sound
/// because `source_record_id` and `content_hash` are satellite-independent (only `canonical_id` mixes in the
/// satellite segment), so these fingerprints match the ones stored in a satellite's receipt entries.
/// It claims and parses exactly like the engine does, writes nothing, and produces no report. Distinct pairs
/// are counted, so within-round duplicates neither inflate nor deflate the matched ratio.
pub fn current_fingerprints(registry: &Registry, path: &Path) -> Result<Vec<Fingerprint>> {
    let source = crate::source::load_source(path)?;
    let mut claimed = BTreeSet::new();
    let mut fingerprints = BTreeSet::new();
    for reader in &registry.readers {
        for claim in reader.claim(&source.files) {
            ensure!(
                claimed.insert(claim.path.clone()),
                "File claimed by multiple Readers"
            );
            let output = reader.read(&claim, &source).with_context(|| {
                format!(
                    "Source parsing failed [{}] at {}",
                    reader.id(),
                    crate::gate::mask(&claim.path)
                )
            })?;
            for record in &output.records {
                fingerprints.insert(Fingerprint {
                    source_record_id: record.source_record_id.clone(),
                    content_hash: record.content_hash.clone(),
                });
            }
        }
    }
    Ok(fingerprints.into_iter().collect())
}

/// A registered satellite whose latest receipt mostly matches the current source round.
#[derive(Debug, Clone, PartialEq)]
pub struct RelocationCandidate {
    pub satellite_id: String,
    pub label: String,
    /// Records whose `source_record_id` and `content_hash` both appear in that satellite's latest receipt.
    pub matched: usize,
    /// Source records in the current round; the ratio's denominator.
    pub total: usize,
    pub ratio: f64,
}

/// Pure relocation check (DEC-20 item 6): a single match requires an identical `source_record_id` and
/// `content_hash`; a satellite is suspected when at least `RELOCATION_FLOOR` records match and they cover at
/// least `RELOCATION_RATIO` of this round. Satellites without a valid receipt never match, small sources never
/// trigger, and the result only proposes a choice: it never merges or rebinds anything by itself.
pub fn relocation_candidates(
    entries: &[SatelliteEntry],
    receipts: &BTreeMap<String, Vec<Fingerprint>>,
    current: &[Fingerprint],
) -> Vec<RelocationCandidate> {
    if current.is_empty() {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for entry in entries {
        let Some(history) = receipts.get(&entry.id) else {
            continue;
        };
        let known: BTreeSet<&Fingerprint> = history.iter().collect();
        let matched = current
            .iter()
            .filter(|fingerprint| known.contains(fingerprint))
            .count();
        let ratio = matched as f64 / current.len() as f64;
        if matched >= RELOCATION_FLOOR && ratio >= RELOCATION_RATIO {
            candidates.push(RelocationCandidate {
                satellite_id: entry.id.clone(),
                label: entry.label.clone(),
                matched,
                total: current.len(),
                ratio,
            });
        }
    }
    candidates
}

/// Reads each registered satellite's most recent valid receipt from the home receipts directory (DEC-19).
/// A satellite without receipts contributes nothing; a corrupt receipt, or one filed under another satellite's
/// ID, is a refused execution basis rather than a silently skipped file, because relocation detection would
/// otherwise miss a real move and quietly issue a second satellite for the same memories.
pub fn latest_receipts(
    home: &Path,
    entries: &[SatelliteEntry],
) -> Result<BTreeMap<String, Vec<Fingerprint>>> {
    let mut latest = BTreeMap::new();
    for entry in entries {
        let directory = receipts_dir(home, entry.id.as_str());
        let items = match fs::read_dir(&directory) {
            Ok(items) => items,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Cannot read satellite receipt directory: {}",
                        crate::gate::mask(&directory.to_string_lossy())
                    )
                });
            }
        };
        let mut best: Option<(OffsetDateTime, Vec<Fingerprint>)> = None;
        for item in items {
            let path = item?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let receipt = read_receipt(&path)?;
            ensure!(
                receipt
                    .source
                    .satellite
                    .as_ref()
                    .map(|satellite| satellite.id.as_str())
                    == Some(entry.id.as_str()),
                "Receipt {} belongs to another satellite",
                crate::gate::mask(&path.to_string_lossy())
            );
            let created = OffsetDateTime::parse(&receipt.created_at, &Rfc3339)
                .context("Receipt creation time is not RFC 3339")?;
            if best.as_ref().is_none_or(|(newest, _)| created > *newest) {
                best = Some((created, fingerprints(&receipt)));
            }
        }
        if let Some((_, fingerprints)) = best {
            latest.insert(entry.id.clone(), fingerprints);
        }
    }
    Ok(latest)
}

/// Loads and validates one receipt file; structural validity, not write success, is what this checks.
pub fn read_receipt(path: &Path) -> Result<ReceiptReport> {
    let bytes = fs::read(path)?;
    let receipt: ReceiptReport = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid receipt JSON or fields"))
        .with_context(|| crate::gate::mask(&path.to_string_lossy()))?;
    crate::schema::validate("receipt-report", &receipt)?;
    Ok(receipt)
}

/// Projects a receipt's per-record identity and body evidence for relocation matching.
pub fn fingerprints(receipt: &ReceiptReport) -> Vec<Fingerprint> {
    receipt
        .entries
        .iter()
        .map(|entry| Fingerprint {
            source_record_id: entry.source_record_id.clone(),
            content_hash: entry.content_hash.clone(),
        })
        .collect()
}

/// Names a registered satellite's current binding for diagnostics without exposing more than the registry holds.
fn registered_location(entry: &SatelliteEntry) -> String {
    entry
        .path
        .as_deref()
        .map(crate::gate::mask)
        .unwrap_or_else(|| "an export bundle".into())
}

/// Lists registered satellites for a missing-ID diagnostic; an empty registry says so explicitly.
fn registered_summary(entries: &[SatelliteEntry]) -> String {
    if entries.is_empty() {
        return "none".into();
    }
    entries
        .iter()
        .map(|entry| format!("{} ({})", entry.id, entry.label))
        .collect::<Vec<_>>()
        .join(", ")
}
