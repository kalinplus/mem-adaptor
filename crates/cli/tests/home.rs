//! End-to-end home-mode coverage through the real CLI binary: `init` output and layout, the
//! plan→apply lifecycle issuing a satellite and filing its receipt, path auto-match on the next
//! round, the non-interactive relocation refusal, and direct-mode policy persistence.
//! Interactive prompts cannot run under a spawned process, so the choice paths are covered by the
//! unit tests in `crates/cli/src/home.rs` with injected answers instead of here.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;
use tempfile::TempDir;

/// Runs the built CLI with captured output, an isolated user configuration, and no terminal input.
fn cli(args: &[&str]) -> Output {
    use std::process::Command;
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(args)
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home())
        .output()
        .unwrap()
}

/// Creates a source directory of `count` Markdown notes, the shape the Markdown reader claims.
fn source(root: &Path, count: usize) -> PathBuf {
    let directory = root.join("source");
    fs::create_dir_all(&directory).unwrap();
    for index in 0..count {
        fs::write(
            directory.join(format!("note-{index}.md")),
            format!("# Note {index}\n\nbody {index}"),
        )
        .unwrap();
    }
    directory
}

/// Reads the home's satellite registry from its configuration file.
fn registry(home: &Path) -> Vec<Value> {
    let text = fs::read_to_string(home.join(".mem-adaptor/config.toml")).unwrap();
    let parsed = toml::from_str::<toml::Value>(&text).unwrap();
    let entries = parsed
        .get("satellites")
        .and_then(|satellites| satellites.as_array())
        .map(|entries| serde_json::to_value(entries).unwrap())
        .unwrap_or(serde_json::Value::Null);
    entries.as_array().cloned().unwrap_or_default()
}

/// Extracts the single satellite printed by a plan run from its captured stdout.
fn satellite_from(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("Satellite: "))
        .expect("plan prints its satellite");
    line.trim_end_matches('.')
        .trim_start_matches("Satellite: ")
        .split(" (")
        .next()
        .unwrap()
        .to_owned()
}

/// Runs the full init → plan → apply cycle once and returns the home, source, and issued satellite ID.
fn lifecycle(root: &Path, label: &str) -> (PathBuf, PathBuf, String) {
    let home = root.join("home");
    let source = source(root, 6);
    let report = root.join("plan.json");
    let init = cli(&["init", home.to_str().unwrap()]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let stdout = String::from_utf8_lossy(&init.stdout);
    assert!(stdout.contains("WARNING: pushing this home to a public remote"));
    assert!(stdout.contains("Secret policy: pass (shipped default"));
    let config = home.join(".mem-adaptor/config.toml");
    let registry_before = fs::read(&config).unwrap();
    let target = format!("okf:{}", home.display());
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--satellite",
        "new",
        "--label",
        label,
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let satellite = satellite_from(&plan);
    assert!(
        String::from_utf8_lossy(&plan.stdout)
            .contains("issued here and registered after an approved apply")
    );
    // Planning is read-only (DEC-11): the registry bytes are unchanged and no receipt exists yet.
    assert_eq!(fs::read(&config).unwrap(), registry_before);
    assert!(
        !home
            .join(format!(".mem-adaptor/receipts/{satellite}"))
            .exists()
    );
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let apply_stdout = String::from_utf8_lossy(&apply.stdout);
    assert!(apply_stdout.contains("Receipt filed: "), "{apply_stdout}");
    (home.to_path_buf(), source, satellite)
}

#[test]
fn init_plan_apply_lifecycle_registers_satellite_and_files_receipts() {
    let root = TempDir::new().unwrap();
    let (home, source, satellite) = lifecycle(root.path(), "Vault");
    let registry = registry(&home);
    assert_eq!(registry.len(), 1);
    assert_eq!(registry[0]["id"].as_str().unwrap(), satellite);
    assert_eq!(registry[0]["label"].as_str().unwrap(), "Vault");
    assert_eq!(registry[0]["system"].as_str().unwrap(), "markdown");
    let registered = registry[0]["path"].as_str().unwrap();
    assert!(
        registered.ends_with("source"),
        "registry binds the source directory: {registered}"
    );
    assert!(fs::canonicalize(&source).unwrap().starts_with(registered));
    let chain = home.join(format!(".mem-adaptor/receipts/{satellite}"));
    let receipts: Vec<_> = fs::read_dir(&chain).unwrap().collect();
    assert_eq!(receipts.len(), 1);
    let receipt: Value =
        serde_json::from_slice(&fs::read(receipts[0].as_ref().unwrap().path()).unwrap()).unwrap();
    assert_eq!(
        receipt["source"]["satellite"]["id"].as_str().unwrap(),
        satellite
    );
    assert_eq!(receipt["entries"].as_array().unwrap().len(), 6);
}

#[test]
fn a_second_plan_matches_the_registered_path_without_satellite_flags() {
    let root = TempDir::new().unwrap();
    let (home, source, satellite) = lifecycle(root.path(), "Vault");
    let config = home.join(".mem-adaptor/config.toml");
    let registry_before = fs::read(&config).unwrap();
    let report = root.path().join("plan2.json");
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let stdout = String::from_utf8_lossy(&plan.stdout);
    assert!(
        stdout.contains(&format!("Satellite: {satellite} (Vault).")),
        "{stdout}"
    );
    assert!(!stdout.contains("issued here"));
    // Resolution read the registry without changing it.
    assert_eq!(fs::read(&config).unwrap(), registry_before);
}

#[test]
fn a_moved_source_is_refused_until_the_choice_is_stated_then_rebinds() {
    let root = TempDir::new().unwrap();
    let (home, source, satellite) = lifecycle(root.path(), "Vault");
    let moved = root.path().join("moved");
    fs::rename(&source, &moved).unwrap();
    let config = home.join(".mem-adaptor/config.toml");
    let registry_before = fs::read(&config).unwrap();
    let report = root.path().join("plan-moved.json");
    let refused = cli(&[
        "plan",
        moved.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("already-registered sources"), "{stderr}");
    assert!(stderr.contains("--satellite"), "{stderr}");
    // The refusal changed nothing.
    assert_eq!(fs::read(&config).unwrap(), registry_before);
    assert!(!report.exists());
    // Stating the choice rebinds the satellite to the new path after an approved apply; the display
    // label travels with the run (mutable, never hashed), so the same satellite can be renamed in passing.
    let chosen = cli(&[
        "plan",
        moved.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--satellite",
        &satellite,
        "--label",
        "Renamed vault",
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        chosen.status.success(),
        "{}",
        String::from_utf8_lossy(&chosen.stderr)
    );
    let stdout = String::from_utf8_lossy(&chosen.stdout);
    assert!(stdout.contains("(Renamed vault)"), "{stdout}");
    assert!(stdout.contains("replaces that binding"), "{stdout}");
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let registry = registry(&home);
    assert_eq!(registry.len(), 1);
    assert_eq!(registry[0]["label"].as_str().unwrap(), "Renamed vault");
    let registered = registry[0]["path"].as_str().unwrap();
    assert!(
        registered.ends_with("moved"),
        "rebound to the new path: {registered}"
    );
}

#[test]
fn a_direct_first_explicit_choice_is_persisted_and_later_overrides_stay_run_local() {
    let root = TempDir::new().unwrap();
    let config_root = root.path().join("user-config");
    fs::create_dir_all(&config_root).unwrap();
    let user_config = config_root.join("mem-adaptor/config.toml");
    let source = source(root.path(), 2);
    let target = format!("okf:{}", root.path().join("target").display());
    let first = root.path().join("first.json");
    let run = |report: &Path, policy: Option<&str>| {
        let mut args = vec![
            "plan",
            source.to_str().unwrap(),
            "--to",
            &target,
            "--report",
            report.to_str().unwrap(),
        ];
        if let Some(policy) = policy {
            args.extend(["--secret-policy", policy]);
        }
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_mem-adaptor"));
        command
            .args(&args)
            .env("LOG_LEVEL", "info")
            .env("XDG_CONFIG_HOME", config_root.to_str().unwrap());
        command.output().unwrap()
    };
    let output = run(&first, Some("block"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Saved the gate policy"));
    assert!(user_config.exists(), "first explicit choice persists");
    let stored = fs::read_to_string(&user_config).unwrap();
    assert!(stored.contains("block"));
    // A later run-level override does not rewrite the stored policy.
    let second = root.path().join("second.json");
    let output = run(&second, Some("pass"));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value = serde_json::from_slice(&fs::read(second).unwrap()).unwrap();
    assert_eq!(document["gate_policy"]["secrets"].as_str().unwrap(), "pass");
    assert_eq!(
        document["gate_policy"]["origin"].as_str().unwrap(),
        "user_choice"
    );
    assert_eq!(fs::read_to_string(&user_config).unwrap(), stored);
}

#[test]
fn satellite_flags_are_validated_before_any_work() {
    let root = TempDir::new().unwrap();
    let source = source(root.path(), 1);
    let target = format!("okf:{}", root.path().join("target").display());
    let report = root.path().join("plan.json");
    let label_only = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--label",
        "x",
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(!label_only.status.success());
    assert!(String::from_utf8_lossy(&label_only.stderr).contains("--label requires --satellite"));
    let new_without_home = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--satellite",
        "new",
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(!new_without_home.status.success());
    assert!(String::from_utf8_lossy(&new_without_home.stderr).contains("needs a home registry"));
    assert!(!report.exists());
}

#[test]
fn a_source_moved_between_plan_and_apply_is_refused_and_never_touches_the_registry() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let source = source(root.path(), 3);
    let report = root.path().join("plan.json");
    let init = cli(&["init", home.to_str().unwrap()]);
    assert!(init.status.success());
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--satellite",
        "new",
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let config = home.join(".mem-adaptor/config.toml");
    let registry_before = fs::read(&config).unwrap();
    // Apply re-opens the plan's own source location, so a moved directory fails the engine's execution-basis
    // check before any write; the registry's own stale-plan guard sits behind that refusal as defense in
    // depth for the identity check itself.
    fs::rename(&source, root.path().join("moved")).unwrap();
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    // The approved basis is gone, which is the stale-basis class rather than an ordinary input error.
    assert_eq!(
        common::exit_code(&apply),
        common::EXIT_BASIS,
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let stderr = String::from_utf8_lossy(&apply.stderr);
    assert!(stderr.contains("[S7]"), "{stderr}");
    assert!(stderr.contains("Cannot open source"), "{stderr}");
    // The refused run leaves the registry exactly as it was: no satellite issued, no receipt filed.
    assert_eq!(fs::read(&config).unwrap(), registry_before);
    assert!(
        !home
            .join(".mem-adaptor/receipts")
            .join(satellite_from(&plan))
            .exists()
    );
}

/// Snapshots a directory tree (relative path → bytes) so refusals can prove nothing was written.
fn tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, path: &Path, output: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), output);
            }
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            output.insert(relative, fs::read(path).unwrap());
        }
    }
    let mut output = std::collections::BTreeMap::new();
    visit(root, root, &mut output);
    output
}

#[test]
fn a_home_plan_refuses_when_the_home_configuration_disappeared() {
    let root = TempDir::new().unwrap();
    let (home, source, _satellite) = lifecycle(root.path(), "Vault");
    let report = root.path().join("plan2.json");
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    // Removing the registry removes the execution basis: applying as a direct migration would
    // strand this round's receipt outside the satellite's chain, so the run must refuse.
    fs::remove_file(home.join(".mem-adaptor/config.toml")).unwrap();
    let target_before = tree(&home);
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    assert!(!apply.status.success());
    let stderr = String::from_utf8_lossy(&apply.stderr);
    assert!(stderr.contains("[S7]"), "{stderr}");
    assert!(stderr.contains("no .mem-adaptor/config.toml"), "{stderr}");
    // Refused before approval and writes: no target byte changed, no approval, no receipt.
    assert_eq!(tree(&home), target_before);
    assert!(!report.with_extension("approval.json").exists());
    assert!(!report.with_extension("receipt.json").exists());
}

#[test]
fn a_direct_plan_refuses_when_its_target_became_a_home() {
    let root = TempDir::new().unwrap();
    let source = source(root.path(), 2);
    let target = root.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let report = root.path().join("plan.json");
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", target.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let init = cli(&["init", target.to_str().unwrap()]);
    assert!(init.status.success());
    let target_before = tree(&target);
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    assert!(!apply.status.success());
    let stderr = String::from_utf8_lossy(&apply.stderr);
    assert!(stderr.contains("target is now a home"), "{stderr}");
    assert_eq!(tree(&target), target_before);
}

#[test]
fn a_direct_plan_with_an_explicit_satellite_refuses_after_the_target_became_a_home() {
    let root = TempDir::new().unwrap();
    let source = source(root.path(), 2);
    let target = root.path().join("target");
    fs::create_dir_all(&target).unwrap();
    let report = root.path().join("plan.json");
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", target.display()),
        "--satellite",
        "aaaaaaa2",
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let init = cli(&["init", target.to_str().unwrap()]);
    assert!(init.status.success());
    let target_before = tree(&target);
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    // The identity is neither registered in the new home nor derivable from the source, so the
    // refusal happens before writes instead of failing registry convergence afterwards.
    assert!(!apply.status.success());
    let stderr = String::from_utf8_lossy(&apply.stderr);
    assert!(stderr.contains("neither registered"), "{stderr}");
    assert_eq!(tree(&target), target_before);
    assert!(!report.with_extension("approval.json").exists());
}

#[test]
fn a_failed_home_receipt_save_reports_partial_completion() {
    let root = TempDir::new().unwrap();
    let (home, _source, satellite) = lifecycle(root.path(), "Vault");
    let report = root.path().join("plan2.json");
    let plan = cli(&[
        "plan",
        root.path().join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(plan.status.success());
    // Making the satellite's receipt directory read-only after planning breaks only the final receipt
    // save: the history receipts stay readable, so execution-basis checks pass, targets are written, the
    // registry converges, and the failure is reported as happening after both. Permission bits would not
    // stop a root user; this suite never runs as root, so the injection stays reliable here.
    let receipts = home.join(format!(".mem-adaptor/receipts/{satellite}"));
    let mut permissions = fs::metadata(&receipts).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o555);
    fs::set_permissions(&receipts, permissions).unwrap();
    let apply = cli(&["apply", report.to_str().unwrap(), "--yes"]);
    assert!(!apply.status.success());
    let stderr = String::from_utf8_lossy(&apply.stderr);
    assert!(stderr.contains("[S9]"), "{stderr}");
    assert!(stderr.contains("Home receipt save failed"), "{stderr}");
    // Partial completion is observable: the satellite is registered and targets were written.
    assert_eq!(registry(&home)[0]["id"].as_str().unwrap(), satellite);
    assert!(home.join("memories").is_dir());
    // Restore write permission so the temporary directory can be cleaned up.
    let mut permissions = fs::metadata(&receipts).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&receipts, permissions).unwrap();
}

#[test]
fn an_explicit_receipt_override_in_home_mode_warns_and_leaves_the_chain_alone() {
    let root = TempDir::new().unwrap();
    let (home, _source, satellite) = lifecycle(root.path(), "Vault");
    let report = root.path().join("plan2.json");
    let plan = cli(&[
        "plan",
        root.path().join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(plan.status.success());
    let chain = home.join(format!(".mem-adaptor/receipts/{satellite}"));
    let chain_before = tree(&chain);
    assert_eq!(
        chain_before.len(),
        1,
        "the first apply filed exactly one receipt"
    );
    let receipt = root.path().join("override.json");
    let apply = cli(&[
        "apply",
        report.to_str().unwrap(),
        "--yes",
        "--receipt",
        receipt.to_str().unwrap(),
    ]);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    let stdout = String::from_utf8_lossy(&apply.stdout);
    assert!(
        stdout.contains("overrides the home receipt chain"),
        "{stdout}"
    );
    assert!(receipt.is_file());
    // The chain is untouched by the overridden run.
    assert_eq!(tree(&chain), chain_before);
}

/// Extracts the default plan path a home-mode run printed, so tests can apply the filed plan.
fn filed_plan(output: &Output) -> PathBuf {
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("Plan filed: "))
        .expect("home plan prints its filed path");
    PathBuf::from(line.trim_start_matches("Plan filed: "))
}

/// Checks the A -> B -> A acceptance (#33): two satellites converge into one home with plans and receipts
/// filed under the control directory by default, history is picked from the home without flags, and each
/// satellite's next round reconciles shared products against the latest verified basis instead of mistaking
/// the other satellite's write for tampering.
#[test]
fn a_to_b_to_a_convergence_files_plans_by_default_and_never_rejects_legitimate_writes() {
    let root = TempDir::new().unwrap();
    let (home, source_a, satellite_a) = lifecycle(root.path(), "Vault A");
    let target = format!("okf:{}", home.display());

    // Satellite B is a second source; its plan files under <home>/.mem-adaptor/plans/<id>/ by default.
    let source_b = root.path().join("source-b");
    fs::create_dir_all(&source_b).unwrap();
    fs::write(source_b.join("b-note.md"), "# B note\n\nbody b").unwrap();
    let plan_b = cli(&["plan", source_b.to_str().unwrap(), "--to", &target]);
    assert!(
        plan_b.status.success(),
        "{}",
        String::from_utf8_lossy(&plan_b.stderr)
    );
    let satellite_b = satellite_from(&plan_b);
    assert_ne!(satellite_a, satellite_b);
    let plan_b_path = filed_plan(&plan_b);
    let canonical_home = fs::canonicalize(&home).unwrap();
    assert_eq!(
        plan_b_path,
        canonical_home
            .join(format!(".mem-adaptor/plans/{satellite_b}"))
            .join(plan_b_path.file_name().unwrap())
    );
    let apply_b = cli(&["apply", plan_b_path.to_str().unwrap(), "--yes"]);
    assert!(
        apply_b.status.success(),
        "{}",
        String::from_utf8_lossy(&apply_b.stderr)
    );
    let index = fs::read_to_string(home.join("index.md")).unwrap();
    assert!(index.contains("Note 0"), "{index}");
    assert!(index.contains("B note"), "{index}");

    // A re-plans with no flags at all: the previous receipt and shared basis come from the home.
    let plan_a2 = cli(&["plan", source_a.to_str().unwrap(), "--to", &target]);
    assert!(
        plan_a2.status.success(),
        "{}",
        String::from_utf8_lossy(&plan_a2.stderr)
    );
    let plan_a2_path = filed_plan(&plan_a2);
    let report: Value = serde_json::from_str(&fs::read_to_string(&plan_a2_path).unwrap()).unwrap();
    let basis = report["shared_basis_ref"].as_str().unwrap();
    assert!(
        basis.contains(&format!(".mem-adaptor/receipts/{satellite_b}")),
        "the latest verified write is B's receipt: {basis}"
    );
    assert!(report["digest_inputs"]["shared_basis_hash"].is_string());
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["disposition"]["status"] == "omitted"),
        "A's own records are already migrated, not unresolved: {}",
        serde_json::to_string(&report["entries"]).unwrap()
    );
    let apply_a2 = cli(&["apply", plan_a2_path.to_str().unwrap(), "--yes"]);
    assert!(
        apply_a2.status.success(),
        "{}",
        String::from_utf8_lossy(&apply_a2.stderr)
    );
    // Both chains exist per satellite and the index still lists both satellites' notes.
    assert!(
        home.join(format!(".mem-adaptor/receipts/{satellite_a}"))
            .is_dir()
    );
    let index = fs::read_to_string(home.join("index.md")).unwrap();
    assert!(
        index.contains("Note 0") && index.contains("B note"),
        "{index}"
    );
}

/// Checks #33: a user edit of a shared artifact is never overwritten. The next plan of any satellite
/// keeps its entries unresolved, apply performs no target change, and the edit survives verbatim.
#[test]
fn a_user_edited_shared_index_is_refused_and_survives_verbatim() {
    let root = TempDir::new().unwrap();
    let (home, source, _satellite) = lifecycle(root.path(), "Vault");
    let index = home.join("index.md");
    let edited = format!("{}\nuser edit\n", fs::read_to_string(&index).unwrap());
    fs::write(&index, &edited).unwrap();
    let before = tree(&home);

    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
    ]);
    // A shared-artifact divergence leaves every would-write entry unresolved, which is exit 3 and not
    // a failed command: the plan is complete and the target is untouched.
    assert_eq!(
        common::exit_code(&plan),
        common::EXIT_INCOMPLETE,
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let path = filed_plan(&plan);
    let report: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["disposition"]["status"] == "unresolved"
                && entry["disposition"]["reason"]["code"] == "target_modified"),
        "{}",
        serde_json::to_string(&report["entries"]).unwrap()
    );
    let apply = cli(&["apply", path.to_str().unwrap(), "--yes"]);
    // The unresolved entries stay unwritten and are carried into the receipt, so the run completes
    // with exit 3 instead of claiming a complete migration.
    assert_eq!(
        common::exit_code(&apply),
        common::EXIT_INCOMPLETE,
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    // Nothing in the home's data area changed: the user's edit survives byte for byte.
    let after = tree(&home);
    for (path, bytes) in &before {
        if path.contains(".mem-adaptor") {
            continue;
        }
        assert_eq!(after.get(path), Some(bytes), "unchanged: {path}");
    }
    assert_eq!(fs::read_to_string(&index).unwrap(), edited);
}

/// M6 end to end (issue #35): one home lifecycle where each four-rule outcome is produced by the real CLI.
/// init → satellite plan → apply → no change → satellite edit → home edit → both edited (conflict), with the
/// documented exit code asserted at every step so a silently changed disposition cannot pass.
#[test]
fn home_lifecycle_produces_each_four_rule_outcome_once() {
    let root = TempDir::new().unwrap();
    let (home, source, _satellite) = lifecycle(root.path(), "Vault");
    let target = format!("okf:{}", home.display());

    // Rule 1: nothing changed on either side.
    let unchanged = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        root.path().join("unchanged.json").to_str().unwrap(),
    ]);
    assert_eq!(
        common::exit_code(&unchanged),
        common::EXIT_OK,
        "{}",
        String::from_utf8_lossy(&unchanged.stderr)
    );
    let report: Value =
        serde_json::from_slice(&fs::read(root.path().join("unchanged.json")).unwrap()).unwrap();
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["disposition"]["reason"]["code"] == "already_migrated"),
        "{}",
        serde_json::to_string(&report["entries"]).unwrap()
    );

    // Rule 2: only the satellite changed, so an update is planned and applied.
    fs::write(source.join("note-0.md"), "# Note 0\n\n卫星侧更新后的正文").unwrap();
    let update = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        root.path().join("update.json").to_str().unwrap(),
    ]);
    assert_eq!(common::exit_code(&update), common::EXIT_OK);
    let update_report: Value =
        serde_json::from_slice(&fs::read(root.path().join("update.json")).unwrap()).unwrap();
    let changed = update_report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "note-0.md")
        .unwrap();
    assert_eq!(changed["disposition"]["status"], "accepted");
    assert_eq!(
        common::exit_code(&cli(&[
            "apply",
            root.path().join("update.json").to_str().unwrap(),
            "--yes"
        ])),
        common::EXIT_OK
    );

    // Rule 3: only the home changed, which is an omission and therefore not a failure.
    let edited = home.join(format!(
        "memories/{}.md",
        changed["canonical_id"].as_str().unwrap()
    ));
    let text = fs::read_to_string(&edited).unwrap();
    fs::write(&edited, text.replacen("# Note 0", "# 家里的标题", 1)).unwrap();
    let home_only = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        root.path().join("home_only.json").to_str().unwrap(),
    ]);
    assert_eq!(common::exit_code(&home_only), common::EXIT_OK);
    let home_only_report: Value =
        serde_json::from_slice(&fs::read(root.path().join("home_only.json")).unwrap()).unwrap();
    let omitted = home_only_report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["canonical_id"] == changed["canonical_id"])
        .unwrap();
    assert_eq!(omitted["disposition"]["reason"]["code"], "home_modified");

    // Rule 4: both sides changed differently, so the entry is a conflict and the run is not a success.
    fs::write(source.join("note-0.md"), "# Note 0\n\n卫星侧又一次更新").unwrap();
    let conflict = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        root.path().join("conflict.json").to_str().unwrap(),
    ]);
    assert_eq!(common::exit_code(&conflict), common::EXIT_INCOMPLETE);
    let conflict_report: Value =
        serde_json::from_slice(&fs::read(root.path().join("conflict.json")).unwrap()).unwrap();
    let entry = conflict_report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["canonical_id"] == changed["canonical_id"])
        .unwrap();
    assert_eq!(entry["disposition"]["reason"]["code"], "conflict");
    assert_eq!(
        conflict_report["conflict_clusters"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        String::from_utf8_lossy(&conflict.stdout).contains("Conflict cluster"),
        "{}",
        String::from_utf8_lossy(&conflict.stdout)
    );
    // A non-interactive apply leaves the conflict unwritten and reports the same class.
    let before = fs::read(&edited).unwrap();
    let applied = cli(&[
        "apply",
        root.path().join("conflict.json").to_str().unwrap(),
        "--yes",
    ]);
    assert_eq!(common::exit_code(&applied), common::EXIT_INCOMPLETE);
    assert_eq!(fs::read(&edited).unwrap(), before);
}

/// Checks #33: when the shared-basis receipt becomes unreadable after planning, apply refuses before any
/// target write — approval was recorded, but the home's data area keeps its exact bytes.
#[test]
fn a_broken_shared_basis_refuses_before_target_writes() {
    let root = TempDir::new().unwrap();
    let (home, source, satellite) = lifecycle(root.path(), "Vault");
    // A second round plan carries a shared basis; breaking the receipts root afterwards makes that
    // basis unreadable.
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &format!("okf:{}", home.display()),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let path = filed_plan(&plan);
    let before = tree(&home);
    fs::remove_dir_all(home.join(".mem-adaptor/receipts")).unwrap();
    fs::write(home.join(".mem-adaptor/receipts"), b"not a directory").unwrap();
    let apply = cli(&["apply", path.to_str().unwrap(), "--yes"]);
    assert!(!apply.status.success());
    let stderr = String::from_utf8_lossy(&apply.stderr);
    assert!(stderr.contains("[S7]"), "{stderr}");
    // No target byte changed and the satellite's registration survives with no new receipt.
    let after = tree(&home);
    for (path, bytes) in &before {
        if path.contains(".mem-adaptor/receipts") {
            continue;
        }
        assert_eq!(after.get(path), Some(bytes), "unchanged: {path}");
    }
    assert_eq!(registry(&home)[0]["id"].as_str().unwrap(), satellite);
}

/// Checks #33: the control directory is never a memory source. Reading the home itself as a source (a
/// move-the-home migration) claims the managed memory files but nothing under `.mem-adaptor/`.
#[test]
fn a_report_inside_the_home_control_directory_is_accepted_but_one_inside_the_home_is_not() {
    let root = TempDir::new().unwrap();
    let (home, source, _satellite) = lifecycle(root.path(), "Vault");
    let target = format!("okf:{}", home.display());
    // The control directory is inside the home, so it carries the same exemption the apply side grants.
    let explicit = home.join(".mem-adaptor/explicit-plan.json");
    let plan = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        explicit.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    assert!(explicit.exists());
    // Anywhere else inside the home is memory content and stays refused.
    let stray = home.join("stray-plan.json");
    let refused = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        stray.to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("Report must be outside source and target directories"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert!(!stray.exists());
}

#[test]
fn the_control_directory_is_never_a_memory_source() {
    let root = TempDir::new().unwrap();
    let (home, _source, _satellite) = lifecycle(root.path(), "Vault");
    fs::create_dir_all(home.join(".mem-adaptor/notes")).unwrap();
    fs::write(
        home.join(".mem-adaptor/notes/stray.md"),
        "# Stray\n\nshould not be claimed",
    )
    .unwrap();
    // The ChatGPT Reader claims any `*.chatgpt.md` path, so this file would be claimed if the control
    // directory were not excluded from claim input as well as from the inventory.
    fs::write(
        home.join(".mem-adaptor/notes/stray.chatgpt.md"),
        "# Stray\n\nshould not be claimed",
    )
    .unwrap();
    let report = root.path().join("plan-control.json");
    let target = root.path().join("elsewhere");
    let plan = cli(&[
        "plan",
        home.to_str().unwrap(),
        "--to",
        &format!("ump:{}", target.display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    let files: Vec<&str> = parsed["source_inventory"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| file["path"].as_str())
        .collect();
    assert!(
        files.iter().all(|path| !path.starts_with(".mem-adaptor/")),
        "{files:?}"
    );
    assert!(
        files.iter().any(|path| path.starts_with("memories/")),
        "{files:?}"
    );
}
