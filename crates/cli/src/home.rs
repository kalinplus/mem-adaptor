//! Home-mode orchestration and configuration loading for the CLI (DEC-19, DEC-20).
//! `main.rs` parses arguments and dispatches; this module owns every `.mem-adaptor/` decision, the interactive
//! questions, and the post-apply registry convergence, so `mem-adaptor-core` keeps consuming only a
//! `SatelliteSpec` and never learns the home layout.
//! Planning stays read-only (DEC-11): the satellite registry is appended only after an approved apply, and a
//! refused run leaves both the registry and the receipt chain unchanged.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use mem_adaptor_core::engine::{SCHEMA_VERSION, timestamp};
use mem_adaptor_core::governance::{
    Config, GateAction, GatePolicy, HomeConfig, HomeFormat, PolicyOrigin, SatelliteEntry,
};
use mem_adaptor_core::okf;
use mem_adaptor_core::plugins::{Registry, SourceKind};
use mem_adaptor_core::reports::{ReceiptReport, SatelliteSpec};
use mem_adaptor_core::satellite::{self, RelocationCandidate, Resolution, SourceDetection};
use mem_adaptor_core::writer;

/// A loaded home: its directory and the validated configuration holding policy plus the satellite registry.
#[derive(Debug, Clone)]
pub struct Home {
    pub directory: PathBuf,
    pub config: Config,
}

/// Detects home mode from an already validated target (pinned decision 2): only an `okf:<dir>` target whose
/// `.mem-adaptor/config.toml` exists is a home. `ump:` targets and uninitialized directories stay direct mode,
/// so a home is never inferred from a target that cannot hold a registry.
pub fn detect(target_writer: &str, target: &Path) -> Option<PathBuf> {
    (target_writer == "okf" && satellite::home_config_path(target).is_file())
        .then(|| target.to_path_buf())
}

/// Loads and validates a home configuration. A missing or invalid file refuses the run instead of falling back
/// to first-run defaults: a silently empty registry would renumber satellites and split receipt chains.
pub fn load(directory: &Path) -> Result<Home> {
    let path = satellite::home_config_path(directory);
    let config = satellite::read_config(&path)?
        .with_context(|| format!("Home configuration is missing: {}", path.display()))?;
    Ok(Home {
        directory: directory.to_path_buf(),
        config,
    })
}

/// The engine-facing satellite identity plus the registry changes an approved apply must converge.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub spec: SatelliteSpec,
    /// Path binding to converge when a registered satellite's source moved; `None` when it is unchanged.
    pub rebind: Option<String>,
    /// True when this satellite is not registered yet, so a successful apply must issue and append it.
    pub issues: bool,
}

/// Resolves this run's satellite against the home registry and interrogates suspected relocation (DEC-20 5-6).
/// Detection reads source records only for an unregistered path that has receipt history to compare with, so a
/// registered-path run pays no extra read here. `prompt` receives the suspects and returns the satellite ID to
/// continue, or `None` to continue as a new satellite.
/// Reads only: nothing is written, and an unresolved satellite yields an error rather than a guessed identity.
pub fn resolve_satellite(
    home: &Home,
    registry: &Registry,
    source: &Path,
    explicit: Option<&str>,
    label: Option<&str>,
    prompt: &mut dyn FnMut(&[RelocationCandidate]) -> Result<Option<String>>,
) -> Result<Resolved> {
    let entries = home.config.satellites.as_deref().unwrap_or_default();
    let detection = satellite::detect_source(&registry.readers, source)?;
    // First pass asks for detection instead of guessing; the second pass receives a result and must decide.
    let resolution = match satellite::resolve(entries, explicit, label, &detection, None)? {
        Resolution::NeedsDetection(_) => {
            let receipts = satellite::latest_receipts(&home.directory, entries)?;
            let suspects = if receipts.is_empty() {
                Vec::new()
            } else {
                let current = satellite::current_fingerprints(registry, source)?;
                satellite::relocation_candidates(entries, &receipts, &current)
            };
            satellite::resolve(entries, explicit, label, &detection, Some(&suspects))?
        }
        other => other,
    };
    let current_path = detection.canonical_path.to_string_lossy().into_owned();
    match resolution {
        Resolution::Registered { id, label, rebind } => Ok(Resolved {
            spec: SatelliteSpec {
                id,
                label: Some(label),
            },
            rebind: rebind.then_some(current_path),
            issues: false,
        }),
        Resolution::New(candidate) => Ok(Resolved {
            spec: SatelliteSpec {
                id: candidate.id,
                label: Some(candidate.label),
            },
            rebind: None,
            issues: true,
        }),
        Resolution::Suspected {
            candidate,
            suspects,
            id_occupied,
        } => match prompt(&suspects)? {
            Some(id) => {
                let entry = satellite::find_by_id(entries, &id).with_context(|| {
                    format!("Chosen satellite {id} is not registered in this home")
                })?;
                // A bundle satellite keeps no path binding, so continuing it does not bind this directory.
                let rebind = entry
                    .path
                    .as_ref()
                    .filter(|path| path.as_str() != current_path)
                    .map(|_| current_path.clone());
                Ok(Resolved {
                    spec: SatelliteSpec {
                        id,
                        label: Some(label.unwrap_or(&entry.label).into()),
                    },
                    rebind,
                    issues: false,
                })
            }
            None => {
                ensure!(
                    !id_occupied,
                    "Derived satellite ID {} is already registered to another satellite; refusing to issue the same ID twice. Use --satellite {} to continue that satellite",
                    candidate.id,
                    candidate.id
                );
                Ok(Resolved {
                    spec: SatelliteSpec {
                        id: candidate.id,
                        label: Some(candidate.label),
                    },
                    rebind: None,
                    issues: true,
                })
            }
        },
        Resolution::NeedsDetection(_) => bail!(
            "Satellite resolution did not decide after relocation detection; this is an internal error, and neither the registry nor any target was written"
        ),
    }
}

/// Non-interactive relocation policy (DEC-20 item 6): refuse rather than guess a chain or split one satellite's
/// memories across two IDs. Names the suspects and the explicit escape hatch so a script can state the choice.
pub fn refuse_relocation(suspects: &[RelocationCandidate]) -> Result<Option<String>> {
    let listed = suspects
        .iter()
        .map(|suspect| {
            format!(
                "{} ({}) matches {}/{} records",
                suspect.satellite_id, suspect.label, suspect.matched, suspect.total
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    bail!(
        "This source path is not registered but matches registered satellites: {listed}. It may be a moved satellite or a reused path, so continuing silently could split one memory chain or mix two. Re-run in a terminal to choose, or state the choice now: --satellite <ID> rebinds that satellite, --satellite new issues a new one"
    )
}

/// Pre-write check for an approved plan's satellite identity against the home it will be applied to: the
/// satellite must either already be registered (a rebind or a settled chain) or still derive from the plan's
/// source path (a satellite issued by that plan). A plan that satisfies neither would pass the engine and
/// only fail at registry convergence after targets were written, stranding the receipt, so it is refused
/// here instead. Read-only: the registry is not touched.
pub fn validate_plan_satellite(
    home: &Home,
    registry: &Registry,
    spec: &SatelliteSpec,
    source: &Path,
) -> Result<()> {
    let entries = home.config.satellites.as_deref().unwrap_or_default();
    if entries.iter().any(|entry| entry.id == spec.id) {
        return Ok(());
    }
    let detection = satellite::detect_source(&registry.readers, source)?;
    ensure!(
        satellite::derive_id(&detection.canonical_path.to_string_lossy()) == spec.id,
        "Plan satellite {} is neither registered in this home nor derivable from the plan's source path, so the registry could not converge after writing. Plan again from this home",
        spec.id
    );
    Ok(())
}

/// Converges the home registry after a successful apply (DEC-20 items 3-4): issues and appends a satellite that
/// is not registered yet, rebinds a moved directory path, and records this run's display label.
/// Called only after the engine completed; the registry is never written during planning (DEC-11).
/// An unregistered satellite must still derive from this run's source path; a mismatch means the source moved or
/// the plan went stale between plan and apply, and the registry is left untouched.
/// A failure after the engine wrote targets leaves the registration missing, which the caller must report as a
/// partially completed run rather than a success.
pub fn converge_registry(
    home: &mut Home,
    registry: &Registry,
    spec: &SatelliteSpec,
    source: &Path,
) -> Result<()> {
    let detection = satellite::detect_source(&registry.readers, source)?;
    let entries = home.config.satellites.get_or_insert_with(Vec::new);
    if !entries.iter().any(|entry| entry.id == spec.id) {
        let derived = satellite::derive_id(&detection.canonical_path.to_string_lossy());
        ensure!(
            derived == spec.id,
            "Plan satellite {} does not derive from this source path (derived {}); the source moved or the plan is stale, so the registry was not changed. Plan again",
            spec.id,
            derived
        );
        satellite::register(
            entries,
            SatelliteEntry {
                id: spec.id.clone(),
                label: spec
                    .label
                    .clone()
                    .unwrap_or_else(|| satellite::default_label(&detection)),
                path: registered_path(&detection),
                system: detection.system.clone(),
                created_at: timestamp()?,
            },
        )?;
    }
    let entry = entries
        .iter_mut()
        .find(|entry| entry.id == spec.id)
        .with_context(|| format!("Satellite {} vanished from the registry", spec.id))?;
    // The display label is mutable and never hashed, so this run's label converges into the registry.
    if let Some(label) = &spec.label {
        entry.label = label.clone();
    }
    // Only a directory satellite carries a rebindable path; a bundle never gains or loses one here.
    if let Some(path) = registered_path(&detection)
        && entry.path.is_some()
    {
        entry.path = Some(path);
    }
    satellite::write_config(&satellite::home_config_path(&home.directory), &home.config)
}

/// Files this round's receipts in the satellite's chain inside the home (DEC-19, DEC-20 item 7), returning the
/// written paths. Used when no explicit `--receipt` overrides the default home location; a failure here follows
/// target writes and registry convergence, so the caller reports it as a partially completed run.
pub fn file_receipts(home: &Home, receipts: &[ReceiptReport]) -> Result<Vec<PathBuf>> {
    satellite::save_receipts(&home.directory, receipts)
}

/// Returns the canonical path a directory source binds, and `None` for an export bundle (DEC-20 item 4).
fn registered_path(detection: &SourceDetection) -> Option<String> {
    (detection.kind == SourceKind::Directory)
        .then(|| detection.canonical_path.to_string_lossy().into_owned())
}

/// What `init` created and preserved, reported explicitly so nothing about the home changes silently.
#[derive(Debug, Clone, PartialEq)]
pub struct InitOutcome {
    pub created: Vec<PathBuf>,
    pub preserved: Vec<PathBuf>,
    /// The secret action recorded in the home configuration after this call.
    pub secrets: GateAction,
    /// Where that action came from: a user choice at init, or the shipped default.
    pub origin: PolicyOrigin,
    /// True when `--force` rewrote an existing configuration file.
    pub rewrote_config: bool,
}

/// Initializes a home directory: the DEC-19 skeleton plus the gate-policy choice (pinned decision 3).
/// Default mode only completes what is missing (`index.md`, `log.md`, `.mem-adaptor/config.toml`,
/// `.mem-adaptor/receipts/`); every existing file is preserved byte for byte, no key is changed, and the policy
/// is not re-asked. `--force` may rewrite only `.mem-adaptor/config.toml`, re-asking the policy while carrying
/// the existing satellite registry over unchanged; `index.md`, `log.md`, and `memories/` are never touched.
/// An existing index or log that the OKF Writer could not append to refuses the run before anything is created,
/// so init never leaves a home that fails at its first apply. A corrupt configuration refuses both modes:
/// without a readable registry, `--force` could not honour its preservation requirement.
pub fn init(
    directory: &Path,
    force: bool,
    interactive: bool,
    read_line: &mut dyn FnMut(&str) -> Result<String>,
) -> Result<InitOutcome> {
    ensure!(
        !directory.exists() || directory.is_dir(),
        "Home path exists and is not a directory: {}",
        directory.display()
    );
    let config_path = satellite::home_config_path(directory);
    let existing = satellite::read_config(&config_path)?;
    let index_path = directory.join("index.md");
    let log_path = directory.join("log.md");
    // Verify the home shape before creating anything, so a refused init leaves no half-created home behind.
    if let Some(text) = read_optional(&index_path)? {
        ensure!(
            okf::owned_index(&text)?,
            "Existing index.md is not a mem-adaptor/OKF owned index, so the OKF writer would refuse to append to it. Move it aside or choose another home"
        );
    }
    if let Some(text) = read_optional(&log_path)? {
        ensure!(
            okf::owned_log(&text)?.is_some(),
            "Existing log.md is not a mem-adaptor owned log, so the OKF writer would refuse to append to it. Move it aside or choose another home"
        );
    }
    let (secrets, origin) = match &existing {
        Some(config) if !force => (config.gate_policy.secrets, config.gate_policy.origin),
        _ => gate_policy_choice(interactive, read_line)?,
    };
    let mut created = Vec::new();
    let mut preserved = Vec::new();
    fs::create_dir_all(directory)?;
    if index_path.exists() {
        preserved.push(index_path);
    } else {
        // Identical to the OKF Writer's own header, so the first apply appends instead of refusing the file.
        let index = format!(
            "---\nokf_version: '0.2'\n---\n{}\n# Memory index\n",
            okf::INDEX_MARKER
        );
        writer::atomic_file(directory, "index.md", index.as_bytes(), None)?;
        created.push(directory.join("index.md"));
    }
    if log_path.exists() {
        preserved.push(log_path);
    } else {
        // The initial entry keeps the log parseable: an owned log without a date group is rejected, and the
        // Writer would then refuse the first apply instead of appending.
        let now = timestamp()?;
        let (date, time) = now.split_once('T').context("Invalid init timestamp")?;
        let clock = format!("{}Z", time.trim_end_matches('Z').split('.').next().unwrap());
        let log = format!(
            "{}\n# Directory Update Log\n\n## {date}\n\n- [{clock}] mem-adaptor init: home initialized\n",
            okf::LOG_MARKER
        );
        writer::atomic_file(directory, "log.md", log.as_bytes(), None)?;
        created.push(directory.join("log.md"));
    }
    let receipts_root = satellite::receipts_root(directory);
    if receipts_root.is_dir() {
        preserved.push(receipts_root);
    } else {
        fs::create_dir_all(&receipts_root)?;
        created.push(receipts_root);
    }
    let rewrote_config = existing.is_some() && force;
    match &existing {
        Some(config) if !force => preserved.push(config_path),
        Some(config) => {
            // `--force` re-asks the policy but must not lose the issued registry, the allowlist, or the home
            // declaration; only the secret action and its provenance change. The rewritten file is reported
            // through `rewrote_config`, not as a preserved file, because its policy bytes did change.
            satellite::write_config(
                &config_path,
                &Config {
                    schema_version: SCHEMA_VERSION.into(),
                    gate_policy: policy(secrets, origin, config.gate_policy.rule_allowlist.clone()),
                    home: config.home.clone().or_else(|| Some(okf_home())),
                    satellites: config.satellites.clone(),
                },
            )?;
        }
        None => {
            satellite::write_config(
                &config_path,
                &Config {
                    schema_version: SCHEMA_VERSION.into(),
                    gate_policy: policy(secrets, origin, Vec::new()),
                    home: Some(okf_home()),
                    satellites: None,
                },
            )?;
            created.push(config_path);
        }
    }
    Ok(InitOutcome {
        created,
        preserved,
        secrets,
        origin,
        rewrote_config,
    })
}

/// The default home declaration written by `init`: an OKF 0.2 directory, matching the index it creates.
fn okf_home() -> HomeConfig {
    HomeConfig {
        format: HomeFormat::Okf,
        okf_version: "0.2".into(),
    }
}

/// Builds a gate policy around one chosen secret action; PII checking stays reserved for D2-3.
pub fn policy(secrets: GateAction, origin: PolicyOrigin, allow_rule: Vec<String>) -> GatePolicy {
    GatePolicy {
        secrets,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: allow_rule,
        origin,
        user_selected: origin == PolicyOrigin::UserChoice,
    }
}

/// Asks the gate-policy question shared by `init` and a direct-mode first run (DEC-1).
/// The default is pass and its consequences are stated; a non-interactive run takes that default without asking
/// and records `origin=default`, so scripts and CI never hang on a question nobody can answer and never get a
/// default presented as their own choice. Callers report the resulting provenance.
/// `read_line` prints the prompt and returns one answer line, letting tests inject an answer without a TTY.
pub fn gate_policy_choice(
    interactive: bool,
    read_line: &mut dyn FnMut(&str) -> Result<String>,
) -> Result<(GateAction, PolicyOrigin)> {
    const PROMPT: &str = "Secret gate policy: every finding is always reported. \"pass\" writes findings \
through unchanged (secrets reach the target, and anything sent to a remote target cannot be recalled); \
\"block\" refuses the run. Choose [pass/block] (default pass): ";
    if !interactive {
        return Ok((GateAction::Pass, PolicyOrigin::Default));
    }
    match read_line(PROMPT)?.trim().to_ascii_lowercase().as_str() {
        "" | "pass" => Ok((GateAction::Pass, PolicyOrigin::UserChoice)),
        "block" => Ok((GateAction::Block, PolicyOrigin::UserChoice)),
        other => bail!("Unknown secret policy {other}; choose pass or block"),
    }
}

/// Applies this run's explicit CLI gate choices on top of a stored policy (home or user level) without writing
/// anything back. The result is marked as a user choice so the report never presents a one-run override as the
/// stored configuration, and the stored file keeps its own value for later runs (DEC-1).
pub fn override_policy(
    mut policy: GatePolicy,
    secrets: Option<GateAction>,
    allow_rule: Vec<String>,
) -> GatePolicy {
    if let Some(action) = secrets {
        policy.secrets = action;
        policy.origin = PolicyOrigin::UserChoice;
        policy.user_selected = true;
    }
    if !allow_rule.is_empty() {
        policy.rule_allowlist = allow_rule;
        policy.origin = PolicyOrigin::UserChoice;
        policy.user_selected = true;
    }
    policy
}

/// Location of the direct-mode user configuration: `$XDG_CONFIG_HOME/mem-adaptor/config.toml`, falling back to
/// `$HOME/.config/mem-adaptor/config.toml`. `XDG_CONFIG_HOME` is the injection point that keeps tests away from
/// a real user configuration (DEC-19). `None` means no home directory is known, and the run then uses its
/// effective policy without persisting anything.
pub fn user_config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("mem-adaptor").join("config.toml"))
}

/// Supplies the direct-mode gate policy and reports the configuration file it persisted, if any (DEC-1).
/// `path` is the injectable user-configuration location (`user_config_path` in production, a temporary
/// directory in tests). A stored configuration is reused; an explicit CLI choice overrides it for this run only
/// and is marked as a user choice in the report. A first interactive run asks and persists the answer, so the
/// policy is chosen once instead of being rediscovered on every run; a non-interactive first run takes the
/// documented default for this run only and writes nothing, so scripts and CI never modify a user's
/// configuration behind their back. An unreadable stored configuration is an error, not a silent fallback.
pub fn direct_policy(
    path: Option<PathBuf>,
    secrets: Option<GateAction>,
    allow_rule: Vec<String>,
    interactive: bool,
    read_line: &mut dyn FnMut(&str) -> Result<String>,
) -> Result<(GatePolicy, Option<PathBuf>)> {
    let stored = match &path {
        Some(path) => satellite::read_config(path)?,
        None => None,
    };
    if let Some(config) = &stored {
        return Ok((
            override_policy(config.gate_policy.clone(), secrets, allow_rule),
            None,
        ));
    }
    let (action, origin) = match secrets {
        Some(action) => (action, PolicyOrigin::UserChoice),
        None => gate_policy_choice(interactive, read_line)?,
    };
    let policy = policy(action, origin, allow_rule);
    // Persist only an answer this run actually asked for; an unasked default stays local to this run.
    let written = match (&path, origin) {
        (Some(path), PolicyOrigin::UserChoice) => {
            satellite::write_config(
                path,
                &Config {
                    schema_version: SCHEMA_VERSION.into(),
                    gate_policy: policy.clone(),
                    home: None,
                    satellites: None,
                },
            )?;
            Some(path.clone())
        }
        _ => None,
    };
    Ok((policy, written))
}

/// Reads a file that may legitimately be absent; anything else propagates as an ordinary read error.
fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("Cannot read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    //! Covers the CLI-side home behaviors with injected answers and isolated directories: `init` skeleton and
    //! preservation rules, direct-mode policy persistence, and satellite resolution including the interactive
    //! relocation choice. Receipt chains are built by patching the committed schema vector, so the tests never
    //! hand-write report fields; a setup assertion proves the chain is actually readable before the behavior
    //! under test runs. Home directories are canonicalized because atomic writes refuse symlinked ancestors
    //! and macOS tempdirs live behind /var -> /private/var.

    use super::*;
    use mem_adaptor_core::plugins::Registry;
    use mem_adaptor_core::satellite::{self, Fingerprint};
    use mem_adaptor_reader_markdown::MarkdownReader;
    use serde_json::json;
    use tempfile::TempDir;

    /// An answer function that fails the test if anything asks the user a question.
    fn never_ask(question: &str) -> Result<String> {
        bail!("unexpected question: {question}")
    }

    /// Reads the committed valid receipt vector's document as the template every chain receipt is patched from.
    fn receipt_template() -> serde_json::Value {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/vectors/valid/receipt.json");
        let vector: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        vector["document"].clone()
    }

    /// Patches the template into one receipt owned by `satellite_id` carrying exactly `fingerprints`.
    fn chain_receipt(
        satellite_id: &str,
        run_id: &str,
        created_at: &str,
        fingerprints: &[Fingerprint],
    ) -> serde_json::Value {
        let mut receipt = receipt_template();
        receipt["run_id"] = json!(run_id);
        receipt["created_at"] = json!(created_at);
        receipt["source"]["satellite"] = json!({"id": satellite_id, "label": satellite_id});
        let template = &receipt["entries"][0];
        receipt["entries"] = fingerprints
            .iter()
            .map(|fingerprint| {
                let mut entry = template.clone();
                entry["source_record_id"] = json!(fingerprint.source_record_id);
                entry["content_hash"] = json!(fingerprint.content_hash);
                entry
            })
            .collect::<Vec<_>>()
            .into();
        receipt
    }

    /// Writes one receipt into the satellite's chain directory, the shape `latest_receipts` reads back.
    fn file_chain_receipt(home: &Path, satellite_id: &str, receipt: &serde_json::Value) {
        fs::create_dir_all(satellite::receipts_dir(home, satellite_id)).unwrap();
        fs::write(
            satellite::receipts_dir(home, satellite_id)
                .join(format!("{}.json", receipt["run_id"].as_str().unwrap())),
            serde_json::to_vec(receipt).unwrap(),
        )
        .unwrap();
    }

    /// Builds a source directory of `count` Markdown files and a registry whose Markdown reader claims them.
    fn markdown_source(count: usize) -> (TempDir, Registry) {
        let directory = TempDir::new().unwrap();
        for index in 0..count {
            fs::write(
                directory.path().join(format!("note-{index}.md")),
                format!("# Note {index}\n\nbody {index}"),
            )
            .unwrap();
        }
        let mut registry = Registry::default();
        registry.register_reader(MarkdownReader).unwrap();
        (directory, registry)
    }

    /// Writes a home configuration holding exactly the given registry entries into a canonical directory.
    fn canonical_home(satellites: Vec<SatelliteEntry>) -> (TempDir, PathBuf) {
        let temporary = TempDir::new().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        fs::create_dir_all(home.join(".mem-adaptor")).unwrap();
        let config = Config {
            schema_version: SCHEMA_VERSION.into(),
            gate_policy: policy(GateAction::Pass, PolicyOrigin::Default, Vec::new()),
            home: Some(okf_home()),
            satellites: (!satellites.is_empty()).then_some(satellites),
        };
        satellite::write_config(&satellite::home_config_path(&home), &config).unwrap();
        (temporary, home)
    }

    /// Builds one registry entry bound to `path` under a fixed label.
    fn bound_entry(id: &str, path: &Path) -> SatelliteEntry {
        SatelliteEntry {
            id: id.into(),
            label: "vault".into(),
            path: Some(path.to_string_lossy().into_owned()),
            system: "markdown".into(),
            created_at: "2026-10-01T12:00:00Z".into(),
        }
    }

    /// Snapshots a file's bytes so a later comparison proves nothing rewrote it.
    fn bytes_of(path: &Path) -> Vec<u8> {
        fs::read(path).unwrap()
    }

    #[test]
    fn init_creates_the_skeleton_and_records_an_interactive_choice() {
        let (temporary, home) = canonical_home(Vec::new());
        fs::remove_dir_all(&home).unwrap(); // exercise the create-from-nothing path on a canonical path
        let outcome = init(&home, false, true, &mut |question| {
            assert!(question.contains("pass") && question.contains("block"));
            Ok("block".into())
        })
        .unwrap();
        assert_eq!(outcome.secrets, GateAction::Block);
        assert_eq!(outcome.origin, PolicyOrigin::UserChoice);
        assert!(!outcome.rewrote_config);
        assert_eq!(outcome.preserved, Vec::<PathBuf>::new());
        let created: Vec<_> = outcome
            .created
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            created,
            vec!["index.md", "log.md", "receipts", "config.toml"]
        );
        let index = String::from_utf8(bytes_of(&home.join("index.md"))).unwrap();
        assert!(index.contains("# Memory index"));
        let config = satellite::read_config(&satellite::home_config_path(&home))
            .unwrap()
            .unwrap();
        assert_eq!(config.gate_policy.secrets, GateAction::Block);
        assert_eq!(config.gate_policy.origin, PolicyOrigin::UserChoice);
        assert_eq!(config.home, Some(okf_home()));
        assert_eq!(config.satellites, None);
        drop(temporary);
    }

    #[test]
    fn init_without_a_terminal_takes_the_default_without_asking() {
        let (temporary, home) = canonical_home(Vec::new());
        fs::remove_dir_all(&home).unwrap();
        let outcome = init(&home, false, false, &mut never_ask).unwrap();
        assert_eq!(outcome.secrets, GateAction::Pass);
        assert_eq!(outcome.origin, PolicyOrigin::Default);
        let config = satellite::read_config(&satellite::home_config_path(&home))
            .unwrap()
            .unwrap();
        assert_eq!(config.gate_policy.origin, PolicyOrigin::Default);
        assert!(!config.gate_policy.user_selected);
        drop(temporary);
    }

    #[test]
    fn a_second_init_preserves_every_file_and_never_reasks() {
        let (temporary, home) = canonical_home(Vec::new());
        fs::remove_dir_all(&home).unwrap();
        init(&home, false, true, &mut |_| Ok("block".into())).unwrap();
        let before = [
            bytes_of(&home.join("index.md")),
            bytes_of(&home.join("log.md")),
            bytes_of(&satellite::home_config_path(&home)),
        ];
        let outcome = init(&home, false, true, &mut never_ask).unwrap();
        assert_eq!(outcome.created, Vec::<PathBuf>::new());
        assert!(!outcome.rewrote_config);
        assert_eq!(outcome.secrets, GateAction::Block);
        let after = [
            bytes_of(&home.join("index.md")),
            bytes_of(&home.join("log.md")),
            bytes_of(&satellite::home_config_path(&home)),
        ];
        assert_eq!(before, after);
        drop(temporary);
    }

    #[test]
    fn forced_init_reasks_the_policy_but_carries_the_registry_forward() {
        let (temporary, home) = canonical_home(Vec::new());
        fs::remove_dir_all(&home).unwrap();
        init(&home, false, true, &mut |_| Ok("pass".into())).unwrap();
        let mut config = satellite::read_config(&satellite::home_config_path(&home))
            .unwrap()
            .unwrap();
        let registry = vec![bound_entry("abcd2345", Path::new("/synthetic/vault"))];
        config.satellites = Some(registry.clone());
        config.gate_policy.rule_allowlist = vec!["synthetic-rule".into()];
        satellite::write_config(&satellite::home_config_path(&home), &config).unwrap();
        let index_before = bytes_of(&home.join("index.md"));
        let log_before = bytes_of(&home.join("log.md"));
        let outcome = init(&home, true, true, &mut |_| Ok("block".into())).unwrap();
        assert!(outcome.rewrote_config);
        assert_eq!(outcome.secrets, GateAction::Block);
        assert_eq!(bytes_of(&home.join("index.md")), index_before);
        assert_eq!(bytes_of(&home.join("log.md")), log_before);
        let config = satellite::read_config(&satellite::home_config_path(&home))
            .unwrap()
            .unwrap();
        assert_eq!(config.satellites, Some(registry));
        assert_eq!(config.gate_policy.rule_allowlist, vec!["synthetic-rule"]);
        drop(temporary);
    }

    #[test]
    fn init_refuses_a_foreign_index_and_creates_no_control_directory() {
        let temporary = TempDir::new().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        fs::write(home.join("index.md"), "# Someone else's index").unwrap();
        let before = bytes_of(&home.join("index.md"));
        let error = init(&home, false, true, &mut |_| Ok("pass".into())).unwrap_err();
        assert!(error.to_string().contains("owned index"), "{error:#}");
        assert!(!home.join(".mem-adaptor").exists());
        assert_eq!(bytes_of(&home.join("index.md")), before);
    }

    #[test]
    fn converge_registry_refuses_a_satellite_that_cannot_derive_from_the_source() {
        let (source, registry) = markdown_source(2);
        let (home_temporary, home_directory) = canonical_home(Vec::new());
        let mut home = Home {
            directory: home_directory.clone(),
            config: satellite::read_config(&satellite::home_config_path(&home_directory))
                .unwrap()
                .unwrap(),
        };
        let config_path = satellite::home_config_path(&home_directory);
        let before = bytes_of(&config_path);
        let error = converge_registry(
            &mut home,
            &registry,
            &SatelliteSpec {
                id: "zzzzzzzz".into(),
                label: Some("stale".into()),
            },
            source.path(),
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("does not derive from this source path"),
            "{message}"
        );
        // The refused convergence leaves the registry bytes exactly as they were.
        assert_eq!(bytes_of(&config_path), before);
        drop(home_temporary);
    }

    #[test]
    fn init_refuses_a_foreign_log_and_creates_no_control_directory() {
        let temporary = TempDir::new().unwrap();
        let home = fs::canonicalize(temporary.path()).unwrap();
        fs::write(home.join("log.md"), "# Someone else's log").unwrap();
        let before = bytes_of(&home.join("log.md"));
        let error = init(&home, false, true, &mut |_| Ok("pass".into())).unwrap_err();
        assert!(error.to_string().contains("owned log"), "{error:#}");
        assert!(!home.join(".mem-adaptor").exists());
        assert_eq!(bytes_of(&home.join("log.md")), before);
    }

    #[test]
    fn init_refuses_a_home_path_that_is_a_regular_file() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("not-a-home");
        fs::write(&path, "keep me").unwrap();
        let error = init(&path, false, true, &mut |_| Ok("pass".into())).unwrap_err();
        assert!(error.to_string().contains("not a directory"), "{error:#}");
        assert_eq!(bytes_of(&path), b"keep me");
    }

    #[test]
    fn an_unknown_policy_answer_is_rejected_with_the_two_choices_named() {
        let error = gate_policy_choice(true, &mut |_| Ok("maybe".into())).unwrap_err();
        assert!(
            error.to_string().contains("choose pass or block"),
            "{error:#}"
        );
    }

    #[test]
    fn a_first_interactive_direct_choice_is_persisted_as_user_configuration() {
        let root = TempDir::new().unwrap();
        let config_path = root.path().join("config.toml");
        let (policy, written) = direct_policy(
            Some(config_path.clone()),
            None,
            Vec::new(),
            true,
            &mut |_| Ok("block".into()),
        )
        .unwrap();
        assert_eq!(policy.secrets, GateAction::Block);
        assert_eq!(policy.origin, PolicyOrigin::UserChoice);
        assert_eq!(written, Some(config_path.clone()));
        let stored = satellite::read_config(&config_path).unwrap().unwrap();
        assert_eq!(stored.gate_policy.secrets, GateAction::Block);
        assert_eq!(stored.home, None);
        assert_eq!(stored.satellites, None);
    }

    #[test]
    fn a_stored_direct_policy_is_reused_and_run_overrides_stay_local() {
        let root = TempDir::new().unwrap();
        let config_path = root.path().join("config.toml");
        let stored = Config {
            schema_version: SCHEMA_VERSION.into(),
            gate_policy: policy(GateAction::Block, PolicyOrigin::UserChoice, Vec::new()),
            home: None,
            satellites: None,
        };
        satellite::write_config(&config_path, &stored).unwrap();
        let before = bytes_of(&config_path);
        let (policy, written) = direct_policy(
            Some(config_path.clone()),
            Some(GateAction::Pass),
            Vec::new(),
            true,
            &mut never_ask,
        )
        .unwrap();
        assert_eq!(policy.secrets, GateAction::Pass);
        assert_eq!(policy.origin, PolicyOrigin::UserChoice);
        assert_eq!(written, None);
        assert_eq!(bytes_of(&config_path), before);
    }

    #[test]
    fn a_noninteractive_first_direct_run_writes_no_configuration() {
        let root = TempDir::new().unwrap();
        let config_path = root.path().join("config.toml");
        let (policy, written) = direct_policy(
            Some(config_path.clone()),
            None,
            Vec::new(),
            false,
            &mut never_ask,
        )
        .unwrap();
        assert_eq!(policy.secrets, GateAction::Pass);
        assert_eq!(policy.origin, PolicyOrigin::Default);
        assert_eq!(written, None);
        assert!(!config_path.exists());
    }

    /// Shared setup for relocation choice: a registered satellite with receipt history at an old path and the
    /// same records now living at a new unregistered path, so resolution must stop at the interactive choice.
    /// Both tempdirs are returned so the home and source outlive the setup call.
    fn relocated_source() -> (TempDir, TempDir, Home, Registry) {
        let (source, registry) = markdown_source(6);
        // The registry binds the satellite to a path that no longer hosts the records.
        let old = source.path().join("old-location");
        let (home_temporary, home_directory) = canonical_home(vec![bound_entry("abcd2345", &old)]);
        let home = Home {
            directory: home_directory.clone(),
            config: satellite::read_config(&satellite::home_config_path(&home_directory))
                .unwrap()
                .unwrap(),
        };
        // The receipt chain holds the fingerprints of exactly the records now at the new path, so every
        // current record matches and the ratio reaches the suspicion threshold.
        let fingerprints = satellite::current_fingerprints(&registry, source.path()).unwrap();
        assert_eq!(fingerprints.len(), 6);
        file_chain_receipt(
            &home_directory,
            "abcd2345",
            &chain_receipt("abcd2345", "run-old", "2026-10-05T12:00:00Z", &fingerprints),
        );
        // Setup self-check: the chain must actually be readable, or the behavior under test never runs.
        let latest = match satellite::latest_receipts(
            &home_directory,
            home.config.satellites.as_deref().unwrap(),
        ) {
            Ok(latest) => latest,
            Err(error) => panic!("setup self-check: {error:#}"),
        };
        assert_eq!(latest.get("abcd2345").map(Vec::len), Some(6));
        (source, home_temporary, home, registry)
    }

    #[test]
    fn a_registered_path_resolves_without_asking_anything() {
        let (source, registry) = markdown_source(2);
        let (temporary, home_directory) = canonical_home(vec![bound_entry(
            "abcd2345",
            &fs::canonicalize(source.path()).unwrap(),
        )]);
        let home = Home {
            directory: home_directory.clone(),
            config: satellite::read_config(&satellite::home_config_path(&home_directory))
                .unwrap()
                .unwrap(),
        };
        let resolved = resolve_satellite(
            &home,
            &registry,
            source.path(),
            None,
            None,
            &mut |suspects| bail!("relocation prompt must not fire: {suspects:?}"),
        )
        .unwrap();
        assert_eq!(resolved.spec.id, "abcd2345");
        assert_eq!(resolved.spec.label.as_deref(), Some("vault"));
        assert_eq!(resolved.rebind, None);
        assert!(!resolved.issues);
        drop(temporary);
    }

    #[test]
    fn choosing_the_registered_satellite_rebinds_it_to_the_new_path() {
        let (source, _home_temporary, home, registry) = relocated_source();
        let resolved = resolve_satellite(
            &home,
            &registry,
            source.path(),
            None,
            None,
            &mut |suspects| {
                assert_eq!(suspects.len(), 1);
                assert_eq!(suspects[0].satellite_id, "abcd2345");
                assert_eq!(suspects[0].matched, 6);
                Ok(Some("abcd2345".into()))
            },
        )
        .unwrap();
        assert_eq!(resolved.spec.id, "abcd2345");
        assert!(!resolved.issues);
        assert_eq!(
            resolved.rebind.as_deref(),
            Some(
                fs::canonicalize(source.path())
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            )
        );
    }

    #[test]
    fn declining_the_match_issues_a_new_satellite_for_the_new_path() {
        let (source, _home_temporary, home, registry) = relocated_source();
        let resolved = resolve_satellite(
            &home,
            &registry,
            source.path(),
            None,
            Some("renamed"),
            &mut |_| Ok(None),
        )
        .unwrap();
        assert_eq!(
            resolved.spec.id,
            satellite::derive_id(&fs::canonicalize(source.path()).unwrap().to_string_lossy())
        );
        assert_eq!(resolved.spec.label.as_deref(), Some("renamed"));
        assert!(resolved.issues);
        assert_eq!(resolved.rebind, None);
    }

    #[test]
    fn an_unstated_relocation_choice_refuses_instead_of_guessing() {
        let (source, _home_temporary, home, registry) = relocated_source();
        let error = resolve_satellite(
            &home,
            &registry,
            source.path(),
            None,
            None,
            &mut refuse_relocation,
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("matches registered satellites"),
            "{message}"
        );
        assert!(message.contains("--satellite"), "{message}");
        // The refusal is read-only: the registry keeps its old binding and gains no satellite.
        let config = satellite::read_config(&satellite::home_config_path(&home.directory))
            .unwrap()
            .unwrap();
        assert_eq!(config.satellites.as_ref().map(Vec::len), Some(1));
    }
}
