//! Pins the read-only `show` view (#43): both conflict candidates printed with provenance, hashes,
//! and gate-masked bodies; explicit outcomes for an unknown canonical id, an entry with no two
//! candidates, and a missing home file; and byte-for-byte unchanged source and home trees around
//! every run. Reports never carry bodies (DEC-1), so this local channel is the only body view.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

mod common;

/// Runs the built CLI with captured output and an isolated user configuration.
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(args)
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home())
        .output()
        .unwrap()
}

/// Captures relative paths and exact bytes under a tree so side effects are checked, not just status.
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

/// Returns the plan path a home-mode plan printed.
fn filed_plan(output: &Output) -> PathBuf {
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("Plan filed: "))
        .expect("home plan prints its filed path");
    PathBuf::from(line.trim_start_matches("Plan filed: "))
}

/// Rewrites a home note's body while keeping its frontmatter bytes untouched.
fn rewrite_body(path: &Path, body: &str) {
    let text = fs::read_to_string(path).unwrap();
    let mut parts = text.splitn(3, "---");
    let _lead = parts.next();
    let frontmatter = parts.next().expect("frontmatter block");
    fs::write(path, format!("---{frontmatter}---\n{body}")).unwrap();
}

/// Runs one satellite round into a fresh home, then edits both sides so the next plan reports exactly
/// one four-rule conflict cluster; returns the home, source, canonical id, and that conflict plan.
fn conflict(root: &Path, satellite_body: &str) -> (PathBuf, PathBuf, String, PathBuf) {
    let home = root.join("home");
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("note.md"), "# Title\n\nOriginal body.\n").unwrap();
    assert!(
        cli(&["init", home.to_str().unwrap()]).status.success(),
        "init must succeed"
    );
    let target = format!("okf:{}", home.display());
    let first = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--satellite",
        "new",
        "--label",
        "show",
    ]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_path = filed_plan(&first);
    let applied = cli(&["apply", first_path.to_str().unwrap(), "--yes"]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let memories: Vec<_> = fs::read_dir(home.join("memories"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(memories.len(), 1, "one record was written");
    let id = memories[0]
        .path()
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    rewrite_body(&home.join(format!("memories/{id}.md")), "Edited at home.\n");
    fs::write(
        source.join("note.md"),
        format!("# Title\n\n{satellite_body}"),
    )
    .unwrap();
    let next = cli(&["plan", source.to_str().unwrap(), "--to", &target]);
    assert_eq!(
        common::exit_code(&next),
        common::EXIT_INCOMPLETE,
        "a plan with an open conflict exits 3; stderr: {}",
        String::from_utf8_lossy(&next.stderr)
    );
    let plan_path = filed_plan(&next);
    let report: Value = serde_json::from_str(&fs::read_to_string(&plan_path).unwrap()).unwrap();
    assert_eq!(
        report["conflict_clusters"].as_array().map(Vec::len),
        Some(1),
        "the scenario must produce exactly one conflict cluster"
    );
    (home, source, id, plan_path)
}

/// Both candidates print with provenance, hashes, and masked bodies, and neither tree changes.
#[test]
fn show_prints_both_masked_candidates_without_touching_anything() {
    let root = TempDir::new().unwrap();
    let secret = format!("ghp_TEST{}", "A".repeat(32));
    let (home, source, id, plan) =
        conflict(root.path(), &format!("Edited by the satellite. {secret}\n"));
    let before_home = snapshot(&home);
    let before_source = snapshot(&source);
    let output = cli(&["show", plan.to_str().unwrap(), &id]);
    assert_eq!(
        common::exit_code(&output),
        common::EXIT_OK,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[satellite side] satellite "), "{stdout}");
    assert!(stdout.contains("(label: show)"), "{stdout}");
    assert!(stdout.contains("Edited by the satellite."), "{stdout}");
    assert!(stdout.contains("[secret]"), "{stdout}");
    assert!(
        !stdout.contains(&secret),
        "the planted secret must never print raw: {stdout}"
    );
    assert!(
        stdout.contains("[home side] home file memories/"),
        "{stdout}"
    );
    assert!(stdout.contains("Edited at home."), "{stdout}");
    // The view must carry exactly the hashes the plan declared for both candidates; the home file's
    // own envelope also carries hash fields, so presence of the declared values is the precise check.
    let report: Value = serde_json::from_str(&fs::read_to_string(&plan).unwrap()).unwrap();
    let candidates = report["conflict_clusters"][0]["candidates"]
        .as_array()
        .unwrap();
    assert_eq!(candidates.len(), 2);
    for candidate in candidates {
        assert!(
            stdout.contains(candidate["content_hash"].as_str().unwrap()),
            "declared content hash missing: {stdout}"
        );
        assert!(
            stdout.contains(candidate["record_hash"].as_str().unwrap()),
            "declared record hash missing: {stdout}"
        );
    }
    assert_eq!(snapshot(&home), before_home, "show never writes the home");
    assert_eq!(
        snapshot(&source),
        before_source,
        "show never writes the source"
    );
}

/// A canonical id the plan does not carry is an input error, not an empty view.
#[test]
fn show_reports_an_unknown_canonical_id_as_an_error() {
    let root = TempDir::new().unwrap();
    let (_home, _source, _id, plan) = conflict(root.path(), "Edited by the satellite.\n");
    let output = cli(&["show", plan.to_str().unwrap(), "not-a-plan-entry"]);
    assert_eq!(
        common::exit_code(&output),
        common::EXIT_ERROR,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not an entry of this plan"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// An entry outside any conflict cluster gets an explanation of its disposition, not a body view.
#[test]
fn show_explains_entries_without_two_candidates() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let source = root.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("a.md"), "# A\n\nfirst\n").unwrap();
    assert!(cli(&["init", home.to_str().unwrap()]).status.success());
    let target = format!("okf:{}", home.display());
    let first = cli(&[
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--satellite",
        "new",
    ]);
    assert!(first.status.success());
    let first_path = filed_plan(&first);
    assert!(
        cli(&["apply", first_path.to_str().unwrap(), "--yes"])
            .status
            .success()
    );
    fs::write(source.join("b.md"), "# B\n\nsecond\n").unwrap();
    let next = cli(&["plan", source.to_str().unwrap(), "--to", &target]);
    assert!(next.status.success());
    let plan_path = filed_plan(&next);
    let report: Value = serde_json::from_str(&fs::read_to_string(&plan_path).unwrap()).unwrap();
    let entry = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_locator"] == "b.md")
        .expect("the new record has a plan entry");
    let id = entry["canonical_id"].as_str().unwrap();
    let output = cli(&["show", plan_path.to_str().unwrap(), id]);
    assert_eq!(common::exit_code(&output), common::EXIT_OK);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no two candidates"),
        "the entry is not in a conflict: {stdout}"
    );
    assert!(stdout.contains("accepted"), "{stdout}");
}

/// A home file that disappeared after the plan is reported explicitly while the satellite side still
/// prints; no content is invented for the missing side.
#[test]
fn show_reports_a_missing_home_file_and_still_prints_the_satellite_side() {
    let root = TempDir::new().unwrap();
    let (home, _source, id, plan) = conflict(root.path(), "Edited by the satellite.\n");
    fs::remove_file(home.join(format!("memories/{id}.md"))).unwrap();
    let output = cli(&["show", plan.to_str().unwrap(), &id]);
    assert_eq!(
        common::exit_code(&output),
        common::EXIT_OK,
        "a missing file is a report, not a failure"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("body unavailable"), "{stdout}");
    assert!(
        stdout.contains("Edited by the satellite."),
        "the satellite side still prints: {stdout}"
    );
}
