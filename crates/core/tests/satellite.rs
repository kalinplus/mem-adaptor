//! Satellite issuance, registry, resolution, and relocation evidence for home mode (DEC-20 items 3-6).
//! Derivation vectors are recomputed independently in Python from the DEC-20 formula, so a changed prefix,
//! separator, or truncation fails here rather than silently renumbering every satellite.
//! Registry, receipt-chain, and detection cases use isolated temporary directories, synthetic Readers, and
//! synthetic records; CLI prompts and `init` live in the CLI crate and are covered there.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use mem_adaptor_core::Result;
use mem_adaptor_core::engine::SCHEMA_VERSION;
use mem_adaptor_core::governance::{
    Config, GateAction, GatePolicy, HomeConfig, HomeFormat, PolicyOrigin, SatelliteEntry,
};
use mem_adaptor_core::plugins::{
    Claim, FileInventory, Reader, ReaderOutput, Registry, SourceFs, SourceKind,
};
use mem_adaptor_core::reports::{ReceiptReport, SatelliteSpec};
use mem_adaptor_core::satellite::{
    self, Fingerprint, RelocationCandidate, Resolution, SourceDetection,
};
use tempfile::TempDir;

// Independently recomputed vectors: lower-case Base32 without padding of
// hashlib.sha256(b"mem-adaptor:satellite:v1" + b"\x00" + path.encode()).digest()[:5].
const VAULT: &str = "/synthetic/vault";
const VAULT_ID: &str = "rlaqxsde";
const VAULT2_ID: &str = "z2j3r443";
const OTHER: &str = "/synthetic/other";
const OTHER_ID: &str = "yki7p4rw";
const BUNDLE: &str = "/synthetic/chatgpt-export.zip";
const BUNDLE_ID: &str = "jongrphl";

/// Builds one registered satellite; `path` is `None` for an export bundle, whose download location is unstable.
fn entry(id: &str, label: &str, path: Option<&str>) -> SatelliteEntry {
    SatelliteEntry {
        id: id.into(),
        label: label.into(),
        path: path.map(str::to_owned),
        system: "markdown".into(),
        created_at: support::TIME.into(),
    }
}

/// Builds a resolved source shape without touching the filesystem, so resolution order stays testable alone.
fn detection(path: &str, system: &str, kind: SourceKind) -> SourceDetection {
    SourceDetection {
        canonical_path: PathBuf::from(path),
        system: system.into(),
        kind,
    }
}

/// Builds one distinct identity/body pair; relocation matching requires both fields to agree.
fn fingerprint(index: usize) -> Fingerprint {
    Fingerprint {
        source_record_id: format!("note-{index}.md"),
        content_hash: format!("sha256:{index:064x}"),
    }
}

/// A directory Reader that claims every Markdown file and yields one record per claim.
/// `source_records` stays empty because detection and fingerprinting consume only canonical records; this
/// double is not a faithful Reader output and never runs through engine validation.
struct DirectoryReader;

impl Reader for DirectoryReader {
    /// Names the synthetic system recorded in the registry.
    fn id(&self) -> &'static str {
        "synthetic-directory"
    }
    /// Keeps the adapter identity deterministic.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Declares a live directory, so its canonical path may bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
    }
    /// Claims every Markdown file in the inventory.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .keys()
            .filter(|path| path.ends_with(".md"))
            .map(|path| Claim {
                path: path.clone(),
                layer: "body".into(),
                registered_only: false,
            })
            .collect()
    }
    /// Returns one record per claim, identified by the claimed path.
    fn read(&self, claim: &Claim, _source: &SourceFs) -> Result<ReaderOutput> {
        let mut record = support::canonical();
        record.source_record_id = claim.path.clone();
        Ok(ReaderOutput {
            source_records: Vec::new(),
            records: vec![record],
            anomalies: Vec::new(),
            registered_count: 0,
            deleted_count: 0,
            source_unavailable: Vec::new(),
        })
    }
}

/// A Reader that declares a directory-shaped export bundle, the Gemini Takeout case where guessing from the
/// on-disk shape would classify a one-shot download as a live directory.
struct BundleReader;

impl Reader for BundleReader {
    /// Names the synthetic export system.
    fn id(&self) -> &'static str {
        "synthetic-bundle"
    }
    /// Keeps the adapter identity deterministic.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Declares an export bundle, so no path binding may identify this satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::ExportBundle
    }
    /// Claims every JSON file in the inventory.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .keys()
            .filter(|path| path.ends_with(".json"))
            .map(|path| Claim {
                path: path.clone(),
                layer: "body".into(),
                registered_only: false,
            })
            .collect()
    }
    /// Returns one record per claim, identified by the claimed path.
    fn read(&self, claim: &Claim, _source: &SourceFs) -> Result<ReaderOutput> {
        let mut record = support::canonical();
        record.source_record_id = claim.path.clone();
        Ok(ReaderOutput {
            source_records: Vec::new(),
            records: vec![record],
            anomalies: Vec::new(),
            registered_count: 0,
            deleted_count: 0,
            source_unavailable: Vec::new(),
        })
    }
}

/// Creates a synthetic source tree and returns its canonicalized root, matching what the loader derives.
fn source_tree(files: &[&str]) -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    for name in files {
        let path = directory.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "Synthetic memory.\n").unwrap();
    }
    let root = fs::canonicalize(directory.path()).unwrap();
    (directory, root)
}

/// Checks the derived ID against independently recomputed vectors and the issued-shape contract.
/// A timestamp-free derivation is what keeps the plan and apply phases in agreement, so stability across calls
/// and per-path distinctness are part of the same requirement.
#[test]
fn derived_ids_match_independent_vectors_and_stay_stable() {
    assert_eq!(satellite::derive_id(VAULT), VAULT_ID);
    assert_eq!(satellite::derive_id(VAULT), VAULT_ID);
    assert_eq!(satellite::derive_id("/synthetic/vault2"), VAULT2_ID);
    assert_eq!(satellite::derive_id(OTHER), OTHER_ID);
    assert_eq!(satellite::derive_id(BUNDLE), BUNDLE_ID);
    for id in [VAULT_ID, VAULT2_ID, OTHER_ID, BUNDLE_ID] {
        assert_eq!(id.len(), 8);
        assert!(satellite::valid_satellite_id(id), "{id}");
    }
}

/// Checks the ID alphabet: eight lowercase Base32 characters, rejecting look-alike digits and wrong lengths.
#[test]
fn issued_id_shape_rejects_lookalikes_and_wrong_lengths() {
    for valid in ["rlaqxsde", "22222222", "aaaaaaa7", "765432ab"] {
        assert!(satellite::valid_satellite_id(valid), "{valid}");
    }
    for invalid in [
        "",
        "rlaqxsd",
        "rlaqxsde2",
        "RLAQXSDE",
        "rlaqxs01",
        "rlaqxs89",
        "rlaqx-de",
        "rlaqxsdé",
    ] {
        assert!(!satellite::valid_satellite_id(invalid), "{invalid}");
    }
}

/// Checks the pinned resolution order: an explicit ID wins, then a registry path match, then a derived
/// candidate. An unregistered directory path asks for relocation detection instead of guessing an identity,
/// and `new` on a registered path is refused so one memory chain is never split in two.
#[test]
fn resolution_prefers_explicit_id_then_registry_path_then_derivation() {
    let entries = vec![
        entry(VAULT_ID, "vault", Some(VAULT)),
        entry(VAULT2_ID, "bundle", None),
    ];
    let vault = detection(VAULT, "markdown", SourceKind::Directory);
    // Second step: the registered path binding identifies the satellite without any explicit choice.
    assert_eq!(
        satellite::resolve(&entries, None, None, &vault, None).unwrap(),
        Resolution::Registered {
            id: VAULT_ID.into(),
            label: "vault".into(),
            rebind: false
        }
    );
    // First step: an explicit ID wins over the path binding and rebinds a moved directory satellite.
    let moved = detection(OTHER, "markdown", SourceKind::Directory);
    assert_eq!(
        satellite::resolve(&entries, Some(VAULT_ID), None, &moved, None).unwrap(),
        Resolution::Registered {
            id: VAULT_ID.into(),
            label: "vault".into(),
            rebind: true
        }
    );
    // A label is display-only: it replaces the registry label without touching the issued ID.
    assert_eq!(
        satellite::resolve(&entries, Some(VAULT_ID), Some("Renamed"), &moved, None).unwrap(),
        Resolution::Registered {
            id: VAULT_ID.into(),
            label: "Renamed".into(),
            rebind: true
        }
    );
    // Third step: an unregistered path derives a candidate and waits for relocation detection.
    assert_eq!(
        satellite::resolve(&entries, None, None, &moved, None).unwrap(),
        Resolution::NeedsDetection(satellite::SatelliteCandidate {
            id: OTHER_ID.into(),
            label: "other".into(),
            system: "markdown".into(),
            path: Some(OTHER.into()),
        })
    );
    // Detection found nothing, so the candidate is issued; registration still waits for an approved apply.
    assert!(matches!(
        satellite::resolve(&entries, None, None, &moved, Some(&[])).unwrap(),
        Resolution::New(_)
    ));
    let error = satellite::resolve(&entries, Some("new"), None, &vault, None).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("already registered to satellite {VAULT_ID}")),
        "{error:#}"
    );
    let error = satellite::resolve(&entries, Some("aaaaaaa2"), None, &vault, None).unwrap_err();
    assert!(
        error.to_string().contains("is not registered in this home"),
        "{error:#}"
    );
    assert!(error.to_string().contains(VAULT_ID), "{error:#}");
    let error = satellite::resolve(&entries, Some("not-an-id"), None, &vault, None).unwrap_err();
    assert!(
        error.to_string().contains("8-character base32 ID"),
        "{error:#}"
    );
}

/// Checks that an export bundle always needs an explicit satellite and never registers a path, while a
/// registered bundle satellite keeps no path binding even when this run reads from a directory.
#[test]
fn export_bundles_require_an_explicit_satellite_and_bind_no_path() {
    let entries = vec![entry(BUNDLE_ID, "chatgpt", None)];
    let bundle = detection(BUNDLE, "chatgpt", SourceKind::ExportBundle);
    let error = satellite::resolve(&entries, None, None, &bundle, None).unwrap_err();
    assert!(
        error.to_string().contains("need an explicit --satellite"),
        "{error:#}"
    );
    // A fresh bundle path issues a satellite with no path binding, even without any registry entry.
    assert_eq!(
        satellite::resolve(&[], Some("new"), None, &bundle, None).unwrap(),
        Resolution::New(satellite::SatelliteCandidate {
            id: BUNDLE_ID.into(),
            label: "chatgpt-export.zip".into(),
            system: "chatgpt".into(),
            path: None,
        })
    );
    // Re-downloading the same archive into the same location keeps the already issued satellite.
    assert_eq!(
        satellite::resolve(&entries, Some("new"), None, &bundle, None)
            .unwrap_err()
            .to_string(),
        format!(
            "Derived satellite ID {BUNDLE_ID} is already registered to an export bundle; refusing to issue the same ID twice. Use --satellite {BUNDLE_ID} to continue that satellite"
        )
    );
    // A directory source cannot give a bundle satellite a path binding it never had.
    let directory = detection(VAULT, "markdown", SourceKind::Directory);
    assert_eq!(
        satellite::resolve(&entries, Some(BUNDLE_ID), None, &directory, None).unwrap(),
        Resolution::Registered {
            id: BUNDLE_ID.into(),
            label: "chatgpt".into(),
            rebind: false
        }
    );
    // Issuing a second satellite for a path whose derived ID is already taken is refused, not renumbered.
    let taken = vec![entry(
        OTHER_ID,
        "other satellite",
        Some("/synthetic/elsewhere"),
    )];
    let error = satellite::resolve(
        &taken,
        Some("new"),
        None,
        &detection(OTHER, "markdown", SourceKind::Directory),
        None,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("refusing to issue the same ID twice"),
        "{error:#}"
    );
}

/// Checks that the registry append enforces uniqueness against every existing entry rather than trusting the
/// derivation's collision odds, and that it rejects shapes the schema would also reject.
#[test]
fn registry_append_refuses_duplicate_ids_paths_and_invalid_shapes() {
    let mut entries: Vec<SatelliteEntry> = Vec::new();
    satellite::register(&mut entries, entry(VAULT_ID, "vault", Some(VAULT))).unwrap();
    assert_eq!(entries.len(), 1);
    // A bundle satellite carries no path, so it cannot collide with a directory binding.
    satellite::register(&mut entries, entry(BUNDLE_ID, "chatgpt", None)).unwrap();
    assert_eq!(entries[1].path, None);
    let duplicate_id =
        satellite::register(&mut entries, entry(VAULT_ID, "copy", Some(OTHER))).unwrap_err();
    assert!(
        duplicate_id
            .to_string()
            .contains("refusing to issue the same ID twice"),
        "{duplicate_id:#}"
    );
    let duplicate_path =
        satellite::register(&mut entries, entry(OTHER_ID, "copy", Some(VAULT))).unwrap_err();
    assert!(
        duplicate_path
            .to_string()
            .contains("is already registered to satellite"),
        "{duplicate_path:#}"
    );
    assert_eq!(entries.len(), 2);
    for invalid in [
        entry("SHORT", "vault", None),
        entry(VAULT2_ID, "", None),
        SatelliteEntry {
            id: VAULT2_ID.into(),
            label: "vault".into(),
            path: None,
            system: "".into(),
            created_at: support::TIME.into(),
        },
    ] {
        assert!(satellite::register(&mut entries, invalid).is_err());
    }
    assert_eq!(entries.len(), 2);
}

/// Checks both relocation thresholds: at least five matched records and at least 60% of this round.
/// A single match requires an identical `source_record_id` and `content_hash`, so a record whose body changed
/// is not a match, and a satellite without a valid receipt never matches.
#[test]
fn relocation_detection_requires_both_the_floor_and_the_ratio() {
    let entries = vec![
        entry(VAULT_ID, "vault", Some(VAULT)),
        entry(VAULT2_ID, "no receipts", Some("/synthetic/vault2")),
    ];
    let history: BTreeMap<String, Vec<Fingerprint>> =
        [(VAULT_ID.to_owned(), (0..6).map(fingerprint).collect())].into();
    let suspects = |current: &[Fingerprint]| -> Vec<RelocationCandidate> {
        satellite::relocation_candidates(&entries, &history, current)
    };
    // 6/6 matches: above the floor and above the ratio.
    let all: Vec<Fingerprint> = (0..6).map(fingerprint).collect();
    assert_eq!(
        suspects(&all),
        vec![RelocationCandidate {
            satellite_id: VAULT_ID.into(),
            label: "vault".into(),
            matched: 6,
            total: 6,
            ratio: 1.0
        }]
    );
    // 6/8 = 0.75: still suspected, with this round's record count as the denominator.
    let grown: Vec<Fingerprint> = (0..8).map(fingerprint).collect();
    assert_eq!(suspects(&grown)[0].matched, 6);
    assert_eq!(suspects(&grown)[0].total, 8);
    // 6/11 = 0.55: the floor is met but the ratio is not, so a mostly new source stays a new satellite.
    let diluted: Vec<Fingerprint> = (0..11).map(fingerprint).collect();
    assert!(suspects(&diluted).is_empty());
    // 4/4 = 1.0: the ratio is met but fewer than five records match, so a small source never triggers.
    let small: Vec<Fingerprint> = (0..4).map(fingerprint).collect();
    assert!(suspects(&small).is_empty());
    assert!(suspects(&[]).is_empty());
    // Same record IDs but changed bodies: neither field alone is a match.
    let rewritten: Vec<Fingerprint> = (0..6)
        .map(|index| Fingerprint {
            source_record_id: format!("note-{index}.md"),
            content_hash: format!("sha256:{:064x}", index + 100),
        })
        .collect();
    assert!(suspects(&rewritten).is_empty());
}

/// Checks registry persistence: a written configuration reads back unchanged, a direct-mode configuration
/// without a registry stays valid, an absent file is not an error, and a corrupt or schema-violating file is a
/// refused execution basis rather than a silent empty registry.
#[test]
fn registry_persistence_roundtrips_and_refuses_corruption() {
    let temporary = TempDir::new().unwrap();
    let directory = fs::canonicalize(temporary.path()).unwrap();
    let path = satellite::home_config_path(&directory);
    assert_eq!(satellite::read_config(&path).unwrap(), None);
    let config = Config {
        schema_version: SCHEMA_VERSION.into(),
        gate_policy: GatePolicy {
            secrets: GateAction::Block,
            high_risk_pii: GateAction::Pass,
            rule_allowlist: vec!["github-pat".into()],
            origin: PolicyOrigin::UserChoice,
            user_selected: true,
        },
        home: Some(HomeConfig {
            format: HomeFormat::Okf,
            okf_version: "0.2".into(),
        }),
        satellites: Some(vec![entry(VAULT_ID, "vault", Some(VAULT))]),
    };
    satellite::write_config(&path, &config).unwrap();
    assert_eq!(satellite::read_config(&path).unwrap(), Some(config.clone()));
    // A direct-mode user configuration has no registry and no home declaration, and stays valid.
    let direct = Config {
        schema_version: SCHEMA_VERSION.into(),
        gate_policy: config.gate_policy.clone(),
        home: None,
        satellites: None,
    };
    let direct_path = directory.join("user-config.toml");
    satellite::write_config(&direct_path, &direct).unwrap();
    assert_eq!(
        satellite::read_config(&direct_path)
            .unwrap()
            .unwrap()
            .satellites,
        None
    );
    // An invalid configuration is refused before it can replace a readable registry.
    let invalid = Config {
        satellites: Some(vec![entry("NOT-VALID", "vault", None)]),
        ..config.clone()
    };
    assert!(satellite::write_config(&path, &invalid).is_err());
    assert_eq!(satellite::read_config(&path).unwrap(), Some(config));
    fs::write(&direct_path, "schema_version = ").unwrap();
    let error = satellite::read_config(&direct_path).unwrap_err();
    assert!(
        format!("{error:#}").contains("Invalid configuration TOML"),
        "{error:#}"
    );
    // A structurally complete registry whose ID shape violates the schema is also a refused execution basis,
    // so a hand-edited registry cannot quietly become an empty one.
    fs::write(
        &direct_path,
        fs::read_to_string(&path)
            .unwrap()
            .replace(VAULT_ID, "TOOLONG99"),
    )
    .unwrap();
    let error = satellite::read_config(&direct_path).unwrap_err();
    assert!(format!("{error:#}").contains("config schema"), "{error:#}");
}

/// Checks receipt placement and history reading: receipts are filed under their own satellite's chain, the
/// newest valid receipt supplies the fingerprints, and a receipt filed under the wrong satellite or a corrupt
/// file refuses the run instead of being skipped, because a missed receipt would hide a real move.
#[test]
fn receipt_chains_are_filed_per_satellite_and_read_newest_first() {
    let temporary = TempDir::new().unwrap();
    // Receipt writes refuse symlinked ancestors, so the home is the canonicalized directory the CLI would use.
    let home = fs::canonicalize(temporary.path()).unwrap();
    let home = home.as_path();
    let entries = vec![
        entry(VAULT_ID, "vault", Some(VAULT)),
        entry(VAULT2_ID, "second", Some("/synthetic/vault2")),
    ];
    assert!(
        satellite::latest_receipts(home, &entries)
            .unwrap()
            .is_empty()
    );
    // Two rounds for one satellite and one for another; the older round must not win.
    let rounds = [
        (
            VAULT_ID,
            "run-old",
            "2026-10-05T12:00:00Z",
            (0..6).map(fingerprint).collect::<Vec<_>>(),
        ),
        (
            VAULT_ID,
            "run-new",
            "2026-10-06T12:00:00Z",
            (0..8).map(fingerprint).collect::<Vec<_>>(),
        ),
        (
            VAULT2_ID,
            "run-only",
            "2026-10-06T12:00:00Z",
            (0..6).map(fingerprint).collect::<Vec<_>>(),
        ),
    ];
    let mut receipts = Vec::new();
    for (id, run, created, fingerprints) in rounds {
        receipts.push(receipt(id, run, created, fingerprints));
    }
    let saved = satellite::save_receipts(home, &receipts).unwrap();
    assert_eq!(
        saved,
        vec![
            satellite::receipts_dir(home, VAULT_ID).join("run-old.json"),
            satellite::receipts_dir(home, VAULT_ID).join("run-new.json"),
            satellite::receipts_dir(home, VAULT2_ID).join("run-only.json"),
        ]
    );
    for path in &saved {
        assert!(path.is_file());
    }
    let latest = satellite::latest_receipts(home, &entries).unwrap();
    assert_eq!(
        latest[VAULT_ID],
        (0..8).map(fingerprint).collect::<Vec<_>>()
    );
    assert_eq!(
        latest[VAULT2_ID],
        (0..6).map(fingerprint).collect::<Vec<_>>()
    );
    // A receipt cannot be filed without the satellite that owns its chain.
    let mut orphan = receipt(VAULT_ID, "run-orphan", support::TIME, Vec::new());
    orphan.source.satellite = None;
    assert!(satellite::save_receipts(home, &[orphan]).is_err());
    // A receipt carrying another satellite's ID is a refused basis, not a skipped file.
    let foreign = receipt(VAULT2_ID, "run-foreign", support::TIME, Vec::new());
    fs::write(
        satellite::receipts_dir(home, VAULT_ID).join("run-foreign.json"),
        serde_json::to_vec(&foreign).unwrap(),
    )
    .unwrap();
    let error = satellite::latest_receipts(home, &entries).unwrap_err();
    assert!(
        error.to_string().contains("belongs to another satellite"),
        "{error:#}"
    );
    fs::remove_file(satellite::receipts_dir(home, VAULT_ID).join("run-foreign.json")).unwrap();
    // A corrupt receipt in the chain is also refused rather than silently dropped.
    fs::write(
        satellite::receipts_dir(home, VAULT_ID).join("run-corrupt.json"),
        b"{",
    )
    .unwrap();
    let error = satellite::latest_receipts(home, &entries).unwrap_err();
    assert!(
        format!("{error:#}").contains("Invalid receipt JSON or fields"),
        "{error:#}"
    );
}

/// Checks that the source system and shape come from Reader declarations rather than the on-disk shape, and
/// that this round's fingerprints are projected from parsed records without any satellite binding.
#[test]
fn detection_and_fingerprints_come_from_reader_claims() {
    let (_notes, notes_root) = source_tree(&["a.md", "nested/b.md", "ignored.bin"]);
    let readers: Vec<Box<dyn Reader>> = vec![Box::new(DirectoryReader), Box::new(BundleReader)];
    let detected = satellite::detect_source(&readers, &notes_root).unwrap();
    assert_eq!(detected.canonical_path, notes_root);
    assert_eq!(detected.system, "synthetic-directory");
    assert_eq!(detected.kind, SourceKind::Directory);
    assert_eq!(
        satellite::default_label(&detected),
        notes_root.file_name().unwrap().to_string_lossy()
    );
    // A directory-shaped export bundle is classified by its Reader's declaration, not by its shape.
    let (_takeout, takeout_root) = source_tree(&["takeout/memory.json"]);
    let bundle_readers: Vec<Box<dyn Reader>> = vec![Box::new(BundleReader)];
    let detected = satellite::detect_source(&bundle_readers, &takeout_root).unwrap();
    assert_eq!(detected.system, "synthetic-bundle");
    assert_eq!(detected.kind, SourceKind::ExportBundle);
    // The default label is the source location's final name: a bundle archive's file name, a directory's name.
    assert_eq!(
        satellite::default_label(&detected),
        takeout_root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(
        satellite::default_label(&detection(BUNDLE, "chatgpt", SourceKind::ExportBundle)),
        "chatgpt-export.zip"
    );
    // Several claiming Readers are reported as mixed, and any bundle declaration wins the shape.
    let (_both_tree, both_root) = source_tree(&["note.md", "takeout/memory.json"]);
    let both = satellite::detect_source(&readers, &both_root).unwrap();
    assert_eq!(both.system, "mixed");
    assert_eq!(both.kind, SourceKind::ExportBundle);
    // Fingerprints are distinct identity/body pairs read without a satellite identity.
    let mut registry = Registry::default();
    registry.register_reader(DirectoryReader).unwrap();
    let fingerprints = satellite::current_fingerprints(&registry, &notes_root).unwrap();
    assert_eq!(
        fingerprints
            .iter()
            .map(|fingerprint| fingerprint.source_record_id.clone())
            .collect::<Vec<_>>(),
        vec!["a.md".to_owned(), "nested/b.md".to_owned()]
    );
    assert!(
        fingerprints
            .iter()
            .all(|fingerprint| fingerprint.content_hash == support::HASH)
    );
}

/// Builds a schema-valid receipt for one satellite's round; entries carry the supplied evidence pairs.
/// The populated support shape supplies every other required field, so only identity, time, and evidence vary.
fn receipt(
    satellite_id: &str,
    run_id: &str,
    created_at: &str,
    fingerprints: Vec<Fingerprint>,
) -> ReceiptReport {
    let template = support::receipt(true);
    let entry_template = template.entries[0].clone();
    let mut report = template;
    report.run_id = run_id.into();
    report.created_at = created_at.into();
    report.source.satellite = Some(SatelliteSpec {
        id: satellite_id.into(),
        label: Some(satellite_id.into()),
    });
    report.entries = fingerprints
        .into_iter()
        .map(|fingerprint| {
            let mut entry = entry_template.clone();
            entry.source_record_id = fingerprint.source_record_id;
            entry.content_hash = fingerprint.content_hash;
            entry
        })
        .collect();
    report
}
