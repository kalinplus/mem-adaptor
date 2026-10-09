//! Pins the documented CLI exit codes and the non-interactive apply contract (m6-cli-proposal §3).
//! Each case asserts the exact code and the target bytes, so a run cannot pass as one class of failure
//! while actually failing another way. The interactive prompts themselves are unreachable from these tests,
//! because a captured stdin is never a terminal: that path is verified by the tuistory run whose commands
//! and output are recorded in the delivery notes, and its single condition by the `interactive` unit test
//! in `crates/cli/src/main.rs`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

mod common;

/// Runs the built CLI with captured output, stage logs and an isolated user configuration.
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(args)
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home())
        .output()
        .unwrap()
}

/// Creates one synthetic Markdown record and an initially absent okf target.
fn fixture() -> TempDir {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(
        directory.path().join("source/a.md"),
        "# Synthetic\n\n原样保留。\n",
    )
    .unwrap();
    directory
}

/// Plans into this fixture's target and returns the plan path together with the command output.
fn plan(directory: &TempDir, name: &str) -> (std::path::PathBuf, Output) {
    let report = directory.path().join(name);
    let output = cli(&[
        "plan",
        directory.path().join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", directory.path().join("target").display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    (report, output)
}

/// Plans with an explicit previous receipt so the next round can see recorded history.
fn plan_with_previous(
    directory: &TempDir,
    name: &str,
    previous: &Path,
) -> (std::path::PathBuf, Output) {
    let report = directory.path().join(name);
    let output = cli(&[
        "plan",
        directory.path().join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", directory.path().join("target").display()),
        "--report",
        report.to_str().unwrap(),
        "--previous-receipt",
        previous.to_str().unwrap(),
    ]);
    (report, output)
}

/// Applies a saved plan with explicit non-interactive approval.
fn apply(plan: &Path) -> Output {
    cli(&["apply", plan.to_str().unwrap(), "--yes"])
}

/// Reads a JSON report as a generic value so assertions do not depend on Rust field names.
fn document(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Captures relative paths and exact bytes under one target so side effects are checked, not just status.
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    if root.exists() {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_str().unwrap().to_owned();
            if entry.path().is_dir() {
                for (nested, bytes) in snapshot(&entry.path()) {
                    files.push((format!("{name}/{nested}"), bytes));
                }
            } else {
                files.push((name, fs::read(entry.path()).unwrap()));
            }
        }
    }
    files.sort();
    files
}

/// Usage errors are the argument parser's own class and never reach the pipeline.
#[test]
fn usage_errors_exit_two() {
    let unknown = cli(&["nope"]);
    assert_eq!(common::exit_code(&unknown), common::EXIT_USAGE);
    let missing_arguments = cli(&["plan"]);
    assert_eq!(common::exit_code(&missing_arguments), common::EXIT_USAGE);
}

/// Input failures (a missing source, an unreadable plan) stay in the ordinary error class.
#[test]
fn input_failures_exit_one() {
    let directory = TempDir::new().unwrap();
    let missing = cli(&[
        "plan",
        directory.path().join("absent").to_str().unwrap(),
        "--to",
        &format!("okf:{}", directory.path().join("target").display()),
        "--report",
        directory.path().join("plan.json").to_str().unwrap(),
    ]);
    assert_eq!(common::exit_code(&missing), common::EXIT_ERROR);
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("Planning failed before target writes"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}

/// A tampered plan or a target that moved after approval is a stale execution basis: exit 4.
#[test]
fn stale_execution_basis_exits_four() {
    // A plan whose recorded digest no longer matches its own digest inputs.
    let directory = fixture();
    let (report, planned) = plan(&directory, "plan.json");
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    let mut tampered = document(&report);
    tampered["plan_digest"] = Value::String(format!("sha256:{}", "a".repeat(64)));
    fs::write(&report, serde_json::to_vec_pretty(&tampered).unwrap()).unwrap();
    let applied = apply(&report);
    assert_eq!(common::exit_code(&applied), common::EXIT_BASIS);
    assert!(
        String::from_utf8_lossy(&applied.stderr).contains("Plan digest mismatch"),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );

    // A source file that appeared after approval changes the manifest while leaving the planned
    // records and targets untouched, so this is the source-manifest class of basis mismatch.
    let directory = fixture();
    let (report, planned) = plan(&directory, "plan.json");
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    fs::write(directory.path().join("source/MEMORY.md"), "- [a](a.md)\n").unwrap();
    let applied = apply(&report);
    assert_eq!(common::exit_code(&applied), common::EXIT_BASIS);
    assert!(
        String::from_utf8_lossy(&applied.stderr).contains("Source manifest mismatch"),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
}

/// A complete migration exits 0; a second round that only skips already-migrated records also exits 0,
/// because omissions are not failures.
#[test]
fn complete_and_omitted_runs_exit_zero() {
    let directory = fixture();
    let (report, planned) = plan(&directory, "plan.json");
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    let applied = apply(&report);
    assert_eq!(common::exit_code(&applied), common::EXIT_OK);
    let receipt = document(&directory.path().join("plan.receipt.json"));
    assert!(
        receipt["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["disposition"]["status"] == "accepted"),
        "{}",
        serde_json::to_string(&receipt["entries"]).unwrap()
    );

    let (second, planned) = plan_with_previous(
        &directory,
        "second.json",
        &directory.path().join("plan.receipt.json"),
    );
    let second_report = document(&second);
    assert_eq!(
        second_report["entries"][0]["disposition"]["reason"]["code"],
        "already_migrated",
        "{}",
        serde_json::to_string(&second_report["entries"]).unwrap()
    );
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    let second_receipt = directory.path().join("second.receipt.json");
    let applied = apply(&second);
    assert_eq!(common::exit_code(&applied), common::EXIT_OK);
    assert!(second_receipt.exists());

    // The user's own edit to a managed note is a deliberate divergence: an omission, not a failure.
    let note = directory.path().join(format!(
        "target/memories/{}.md",
        document(&report)["entries"][0]["canonical_id"]
            .as_str()
            .unwrap()
    ));
    let text = fs::read_to_string(&note).unwrap();
    fs::write(&note, text.replacen("# Synthetic", "# 我的标题", 1)).unwrap();
    let (third, planned) = plan_with_previous(&directory, "third.json", &second_receipt);
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    let third_report = document(&third);
    assert_eq!(
        third_report["entries"][0]["disposition"]["reason"]["code"],
        "home_modified",
        "{}",
        serde_json::to_string(&third_report["entries"]).unwrap()
    );
}

/// An entry the tool cannot attribute stays unresolved: plan and apply both exit 3, the note keeps its
/// exact bytes, and the receipt carries the unresolved entry instead of claiming a migration.
#[test]
fn unresolved_entries_exit_three_and_stay_unwritten() {
    let directory = fixture();
    let (report, planned) = plan(&directory, "plan.json");
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    assert_eq!(common::exit_code(&apply(&report)), common::EXIT_OK);
    let first = document(&directory.path().join("plan.receipt.json"));
    let record = first["entries"][0]["canonical_id"].as_str().unwrap();
    let note = directory
        .path()
        .join(format!("target/memories/{record}.md"));

    // An unknown frontmatter key changes the file without matching any attributable pointer.
    let text = fs::read_to_string(&note).unwrap();
    fs::write(
        &note,
        text.replacen("---\n", "---\nneighbour_note: added by hand\n", 1),
    )
    .unwrap();
    let before = snapshot(&directory.path().join("target"));

    let (conflict, planned) = plan_with_previous(
        &directory,
        "second.json",
        &directory.path().join("plan.receipt.json"),
    );
    assert_eq!(common::exit_code(&planned), common::EXIT_INCOMPLETE);
    let second = document(&conflict);
    assert_eq!(
        second["entries"][0]["disposition"]["reason"]["code"],
        "target_modified",
        "{}",
        serde_json::to_string(&second["entries"]).unwrap()
    );
    assert!(
        String::from_utf8_lossy(&planned.stdout).contains("Exit 3:"),
        "{}",
        String::from_utf8_lossy(&planned.stdout)
    );
    // A plan cannot have written anything, so its explanation must not claim a changed target, while
    // the apply below may have written other entries.
    assert!(
        String::from_utf8_lossy(&planned.stdout).contains("This plan wrote no target bytes."),
        "{}",
        String::from_utf8_lossy(&planned.stdout)
    );

    let applied = apply(&conflict);
    assert_eq!(common::exit_code(&applied), common::EXIT_INCOMPLETE);
    assert!(
        String::from_utf8_lossy(&applied.stdout).contains("the target may be partially updated"),
        "{}",
        String::from_utf8_lossy(&applied.stdout)
    );
    assert_eq!(
        snapshot(&directory.path().join("target")),
        before,
        "an unresolved entry must not be written"
    );
    let receipt = document(&directory.path().join("second.receipt.json"));
    assert_eq!(
        receipt["entries"][0]["disposition"]["reason"]["code"],
        "target_modified"
    );
    assert!(receipt["entries"][0]["verification"].is_null());
}

/// A non-interactive apply without explicit approval refuses before any write instead of hanging or
/// guessing, and that refusal is an ordinary input error.
#[test]
fn noninteractive_apply_requires_explicit_approval() {
    let directory = fixture();
    let (report, planned) = plan(&directory, "plan.json");
    assert_eq!(common::exit_code(&planned), common::EXIT_OK);
    let before = snapshot(&directory.path().join("target"));
    let refused = cli(&["apply", report.to_str().unwrap()]);
    assert_eq!(common::exit_code(&refused), common::EXIT_ERROR);
    assert!(
        String::from_utf8_lossy(&refused.stderr)
            .contains("Noninteractive apply requires explicit --yes"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );
    assert_eq!(snapshot(&directory.path().join("target")), before);
    assert!(!directory.path().join("plan.approval.json").exists());
}
