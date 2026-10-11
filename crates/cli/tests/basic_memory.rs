//! End-to-end CLI coverage for the basic-memory target: parsing, native writes, and the
//! receipt-chained idempotent second round, through the real binary and an isolated config home.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

mod common;

/// Runs the built CLI with an isolated user configuration.
fn cli(args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"));
    command
        .args(args)
        .env("LOG_LEVEL", "warn")
        .env("XDG_CONFIG_HOME", common::config_home());
    command.output().unwrap()
}

/// Direct mode files the receipt next to the plan as `<stem>.receipt.json`.
fn filed_receipt(plan: &std::path::Path) -> PathBuf {
    plan.with_extension("receipt.json")
}

/// plan → apply writes native files with explicit permalinks; the receipt-chained second round is
/// a clean no-op and leaves every target byte unchanged.
#[test]
fn basic_memory_cli_round_trip_and_idempotent_second_round() {
    let root = tempfile::TempDir::new().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("bm");
    fs::create_dir(&source).unwrap();
    fs::write(
        source.join("kal.md"),
        "# Kal\n\nUses mem-adaptor for migration\n",
    )
    .unwrap();
    fs::write(source.join("chinese.md"), "# 中文记忆\n\n正文原样\n").unwrap();
    let to = format!("basic-memory:{}", target.display());

    let plan = root.path().join("plan.json");
    let output = cli(&[
        "plan",
        &source.to_string_lossy(),
        "--to",
        &to,
        "--report",
        &plan.to_string_lossy(),
    ]);
    assert_eq!(common::exit_code(&output), common::EXIT_OK);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2 records - 2 to write"), "{stdout}");

    let output = cli(&["apply", &plan.to_string_lossy(), "--yes"]);
    assert_eq!(common::exit_code(&output), common::EXIT_OK);
    let receipt = filed_receipt(&plan);

    let english = fs::read_to_string(target.join("kal.md")).unwrap();
    assert!(english.contains("permalink: kal\n"));
    assert!(english.contains("type: note\n"));
    assert!(english.ends_with("# Kal\n\nUses mem-adaptor for migration\n"));
    assert!(!english.contains("mem_adaptor"));
    // A Chinese heading has no ASCII slug, so that permalink falls back to the id-based name.
    let names: Vec<String> = fs::read_dir(&target)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.starts_with("note-"))
            .count(),
        1
    );

    let before: Vec<_> = names
        .iter()
        .map(|name| fs::read(target.join(name)).unwrap())
        .collect();
    let second = root.path().join("plan2.json");
    let output = cli(&[
        "plan",
        &source.to_string_lossy(),
        "--to",
        &to,
        "--report",
        &second.to_string_lossy(),
        "--previous-receipt",
        &receipt.to_string_lossy(),
    ]);
    assert_eq!(common::exit_code(&output), common::EXIT_OK);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("2 records - 0 to write, 2 omitted"),
        "second round must be a no-op: {stdout}"
    );
    let after: Vec<_> = names
        .iter()
        .map(|name| fs::read(target.join(name)).unwrap())
        .collect();
    assert_eq!(before, after, "target bytes stay unchanged");
}

/// An external edit to a written note refuses the next plan instead of overwriting it.
#[test]
fn basic_memory_cli_external_edit_refuses_overwrite() {
    let root = tempfile::TempDir::new().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("bm");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("kal.md"), "# Kal\n\nBody\n").unwrap();
    let to = format!("basic-memory:{}", target.display());
    let plan = root.path().join("plan.json");
    cli(&[
        "plan",
        &source.to_string_lossy(),
        "--to",
        &to,
        "--report",
        &plan.to_string_lossy(),
    ]);
    let output = cli(&["apply", &plan.to_string_lossy(), "--yes"]);
    assert_eq!(common::exit_code(&output), common::EXIT_OK);
    let receipt = filed_receipt(&plan);

    let path = target.join("kal.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("Body", "Edited in basic-memory")).unwrap();

    let second = root.path().join("plan2.json");
    let output = cli(&[
        "plan",
        &source.to_string_lossy(),
        "--to",
        &to,
        "--report",
        &second.to_string_lossy(),
        "--previous-receipt",
        &receipt.to_string_lossy(),
    ]);
    assert_eq!(common::exit_code(&output), common::EXIT_INCOMPLETE);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("1 unresolved"), "{stdout}");
    // The drifted bytes survive planning: nothing was written.
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("Edited in basic-memory")
    );
}
