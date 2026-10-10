//! Pins the batch entry points (#47): `sync` runs serial plan-then-apply per registered satellite so
//! every later plan reconciles against the shared artifacts the earlier apply just wrote (a batched
//! plan list would exit 4 on the second satellite), a failing satellite never stops the rest, and the
//! aggregate exit code is the most severe class seen. `discover` lists the built-in local memory
//! locations read-only, skips empty directories, and marks each candidate against the home's registry.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

mod common;

/// Runs the built CLI with an isolated user configuration and a fake HOME so the built-in discover
/// locations point at the test's own tree instead of the developer's real one.
fn cli(args: &[&str], fake_home: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"));
    command
        .args(args)
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home());
    if let Some(home) = fake_home {
        command.env("HOME", home);
    }
    command.output().unwrap()
}

/// Creates a memory directory under a fake HOME with the given markdown notes.
fn project_memory(fake_home: &Path, project: &str, notes: &[(&str, &str)]) -> PathBuf {
    let memory = fake_home.join(format!(".claude/projects/{project}/memory"));
    fs::create_dir_all(&memory).unwrap();
    for (file, body) in notes {
        fs::write(memory.join(file), format!("# {body}\n")).unwrap();
    }
    memory
}

/// Registers one satellite by running its first plan and apply, exactly as a user would.
fn register(home: &Path, source: &Path, fake_home: &Path, label: &str) {
    let target = format!("okf:{}", home.display());
    let plan = cli(
        &[
            "plan",
            source.to_str().unwrap(),
            "--to",
            &target,
            "--satellite",
            "new",
            "--label",
            label,
        ],
        Some(fake_home),
    );
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let filed = filed_plan(&plan);
    let apply = cli(
        &["apply", filed.to_str().unwrap(), "--yes"],
        Some(fake_home),
    );
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
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

/// Captures relative paths and exact bytes under a tree.
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

/// Rewrites a home note's body while keeping its frontmatter bytes untouched.
fn rewrite_body(path: &Path, body: &str) {
    let text = fs::read_to_string(path).unwrap();
    let mut parts = text.splitn(3, "---");
    let _lead = parts.next();
    let frontmatter = parts.next().expect("frontmatter block");
    fs::write(path, format!("---{frontmatter}---\n{body}")).unwrap();
}

/// Serial freshness (#47): two satellites with new records both summarise in one `sync --yes`; the
/// second satellite's plan is generated after the first apply, so a batched-plan implementation would
/// refuse its apply with exit 4 here. A second sync round is a clean no-op.
#[test]
fn two_satellites_sync_is_serial_and_fresh() {
    let root = TempDir::new().unwrap();
    let fake_home = root.path().join("fake-home");
    let home = root.path().join("home");
    let first = project_memory(&fake_home, "proj-a", &[("a1.md", "A one")]);
    let second = project_memory(&fake_home, "proj-b", &[("b1.md", "B one")]);
    assert!(
        cli(&["init", home.to_str().unwrap()], Some(&fake_home))
            .status
            .success()
    );
    register(&home, &first, &fake_home, "proj-a");
    register(&home, &second, &fake_home, "proj-b");
    fs::write(first.join("a2.md"), "# A two\n").unwrap();
    fs::write(second.join("b2.md"), "# B two\n").unwrap();

    let sync = cli(&["sync", home.to_str().unwrap(), "--yes"], Some(&fake_home));
    assert_eq!(
        common::exit_code(&sync),
        common::EXIT_OK,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&sync.stdout),
        String::from_utf8_lossy(&sync.stderr)
    );
    let stdout = String::from_utf8_lossy(&sync.stdout);
    assert!(stdout.contains("Satellite"), "{stdout}");
    assert_eq!(
        stdout.matches(" result: exit 0.").count(),
        2,
        "both satellites completed: {stdout}"
    );
    let memories = fs::read_to_string(home.join("index.md")).unwrap();
    assert!(memories.contains("A two"), "{memories}");
    assert!(memories.contains("B two"), "{memories}");

    let again = cli(&["sync", home.to_str().unwrap(), "--yes"], Some(&fake_home));
    assert_eq!(common::exit_code(&again), common::EXIT_OK);
    let again_out = String::from_utf8_lossy(&again.stdout);
    assert!(
        again_out.contains("Plan: 2 records") && again_out.contains("0 to write"),
        "second round is a clean no-op: {again_out}"
    );
}

/// A satellite whose source vanished is reported with exit 1 and the run continues with the rest;
/// the summary lists every satellite and the aggregate exit is 1.
#[test]
fn sync_continues_past_a_failing_satellite() {
    let root = TempDir::new().unwrap();
    let fake_home = root.path().join("fake-home");
    let home = root.path().join("home");
    let first = project_memory(&fake_home, "proj-a", &[("a1.md", "A one")]);
    let second = project_memory(&fake_home, "proj-b", &[("b1.md", "B one")]);
    let third = project_memory(&fake_home, "proj-c", &[("c1.md", "C one")]);
    assert!(
        cli(&["init", home.to_str().unwrap()], Some(&fake_home))
            .status
            .success()
    );
    register(&home, &first, &fake_home, "a");
    register(&home, &second, &fake_home, "b");
    register(&home, &third, &fake_home, "c");
    fs::remove_dir_all(&second).unwrap();
    fs::write(first.join("a2.md"), "# A two\n").unwrap();
    fs::write(third.join("c2.md"), "# C two\n").unwrap();

    let sync = cli(&["sync", home.to_str().unwrap(), "--yes"], Some(&fake_home));
    assert_eq!(
        common::exit_code(&sync),
        common::EXIT_ERROR,
        "one satellite's source is gone; stdout: {}",
        String::from_utf8_lossy(&sync.stdout)
    );
    let stdout = String::from_utf8_lossy(&sync.stdout);
    assert!(
        stdout.contains(" result: exit 1."),
        "the failing satellite is reported: {stdout}"
    );
    assert_eq!(
        stdout.matches(" result: exit 0.").count(),
        2,
        "the other satellites still ran: {stdout}"
    );
    assert!(stdout.contains("Sync summary: 3 satellite(s)."), "{stdout}");
    assert!(stdout.contains("Sync exit 1."), "{stdout}");
    let index = fs::read_to_string(home.join("index.md")).unwrap();
    assert!(
        index.contains("A two") && index.contains("C two"),
        "{index}"
    );
}

/// A conflict satellite is left unwritten with exit 3 (non-interactive records no verdict), the
/// following satellite still summarises, and the aggregate exit is 3.
#[test]
fn sync_conflict_leaves_unresolved_and_continues() {
    let root = TempDir::new().unwrap();
    let fake_home = root.path().join("fake-home");
    let home = root.path().join("home");
    let first = project_memory(&fake_home, "proj-a", &[("a1.md", "A one")]);
    let second = project_memory(&fake_home, "proj-b", &[("b1.md", "B one")]);
    assert!(
        cli(&["init", home.to_str().unwrap()], Some(&fake_home))
            .status
            .success()
    );
    register(&home, &first, &fake_home, "a");
    register(&home, &second, &fake_home, "b");
    // Both-edit conflict on satellite a's record: locate it by content, not directory order.
    let conflict_file = fs::read_dir(home.join("memories"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            fs::read_to_string(path)
                .is_ok_and(|text| text.contains("A one") && !text.contains("B one"))
        })
        .expect("satellite a's home file");
    rewrite_body(&conflict_file, "Edited at home.\n");
    // The byte baseline is the home-edited state: sync must leave exactly these bytes in place.
    let before = fs::read(&conflict_file).unwrap();
    fs::write(first.join("a1.md"), "# A one edited by satellite\n").unwrap();
    fs::write(second.join("b2.md"), "# B two\n").unwrap();

    let sync = cli(&["sync", home.to_str().unwrap(), "--yes"], Some(&fake_home));
    assert_eq!(common::exit_code(&sync), common::EXIT_INCOMPLETE);
    let stdout = String::from_utf8_lossy(&sync.stdout);
    assert!(
        stdout.contains(" result: exit 3.") && stdout.contains(" result: exit 0."),
        "conflict satellite stops at 3, the other completes: {stdout}"
    );
    assert!(
        stdout.contains("Sync exit 3."),
        "the aggregate is the most severe class: {stdout}"
    );
    // The conflicted record is untouched (home edit preserved, satellite value not written).
    let after = fs::read_to_string(&conflict_file).unwrap();
    assert!(
        after.contains("Edited at home.") && !after.contains("A one edited by satellite"),
        "the conflict stays unwritten: {after}"
    );
    // The conflicted record is byte-identical: neither body nor frontmatter was rewritten.
    assert_eq!(
        fs::read(&conflict_file).unwrap(),
        before,
        "conflicted home file must stay byte-identical"
    );
    let index = fs::read_to_string(home.join("index.md")).unwrap();
    assert!(index.contains("B two"), "{index}");
    // Non-interactive runs never answer conflicts: every receipt stays verdict-free.
    for receipt in walk_json(&home.join(".mem-adaptor/receipts")) {
        assert_eq!(
            receipt["verdicts"].as_array().map(Vec::len),
            Some(0),
            "a --yes sync must not record cluster decisions: {receipt}"
        );
    }
}

/// A home with no directory-bound satellites is a clean no-op with an explicit message.
#[test]
fn sync_without_satellites_is_a_clean_noop() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    assert!(
        cli(&["init", home.to_str().unwrap()], None)
            .status
            .success()
    );
    let sync = cli(&["sync", home.to_str().unwrap(), "--yes"], None);
    assert_eq!(common::exit_code(&sync), common::EXIT_OK);
    assert!(
        String::from_utf8_lossy(&sync.stdout).contains("No registered satellites to summarise."),
        "stdout: {}",
        String::from_utf8_lossy(&sync.stdout)
    );
}

/// discover lists the fake HOME's memory directories read-only: nonempty candidates with record
/// counts, empty directories skipped, extra paths honored, and registration status correct.
#[test]
fn discover_lists_candidates_readonly_and_marks_registration() {
    let root = TempDir::new().unwrap();
    let fake_home = root.path().join("fake-home");
    let home = root.path().join("home");
    let first = project_memory(&fake_home, "proj-a", &[("a1.md", "A one")]);
    project_memory(&fake_home, "proj-empty", &[]);
    fs::create_dir_all(fake_home.join(".claude/projects/proj-files-only")).unwrap();
    // Codex keeps its memories directly under ~/.codex/memories, with no per-project nesting.
    let codex = fake_home.join(".codex/memories");
    fs::create_dir_all(&codex).unwrap();
    fs::write(codex.join("cx1.md"), "# Codex one\n").unwrap();
    assert!(
        cli(&["init", home.to_str().unwrap()], Some(&fake_home))
            .status
            .success()
    );

    let before_home = snapshot(&home);
    let before_sources = snapshot(&fake_home);
    let output = cli(&["discover", home.to_str().unwrap()], Some(&fake_home));
    assert_eq!(common::exit_code(&output), common::EXIT_OK);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("[unregistered] {}", first.display())),
        "the nonempty candidate is listed: {stdout}"
    );
    assert!(
        stdout.contains(&format!("[unregistered] {}", codex.display())),
        "the codex directory is a direct candidate: {stdout}"
    );
    assert!(
        stdout.contains("1 candidate(s): 0 registered, 1 unregistered; nothing was written.")
            || stdout
                .contains("2 candidate(s): 0 registered, 2 unregistered; nothing was written."),
        "{stdout}"
    );
    assert_eq!(
        stdout.matches("— 1 records").count(),
        2,
        "both the claude and codex candidates report one record: {stdout}"
    );
    assert!(
        !stdout.contains("proj-empty"),
        "an empty memory directory is not a candidate: {stdout}"
    );
    assert!(
        !stdout.contains("proj-files-only"),
        "a project without a memory/ directory is not a candidate: {stdout}"
    );
    assert_eq!(
        snapshot(&home),
        before_home,
        "discover never writes the home"
    );
    assert_eq!(
        snapshot(&fake_home),
        before_sources,
        "discover never writes the scanned tree"
    );

    // After registration the same candidate is marked with its satellite id and label.
    register(&home, &first, &fake_home, "proj-a");
    let again = cli(&["discover", home.to_str().unwrap()], Some(&fake_home));
    assert_eq!(common::exit_code(&again), common::EXIT_OK);
    let stdout = String::from_utf8_lossy(&again.stdout);
    assert!(
        stdout.contains("[satellite ") && stdout.contains("(proj-a)]"),
        "registered candidates are marked: {stdout}"
    );
    assert!(
        stdout.contains("2 candidate(s): 1 registered, 1 unregistered"),
        "the claude satellite is registered and the codex dir is not: {stdout}"
    );

    // An extra path that does not exist is reported and skipped, not treated as a candidate.
    let missing = cli(
        &[
            "discover",
            home.to_str().unwrap(),
            root.path().join("no-such-dir").to_str().unwrap(),
        ],
        Some(&fake_home),
    );
    assert_eq!(common::exit_code(&missing), common::EXIT_OK);
    assert!(
        String::from_utf8_lossy(&missing.stdout).contains("Skipped")
            && String::from_utf8_lossy(&missing.stdout).contains("not an existing directory"),
        "stdout: {}",
        String::from_utf8_lossy(&missing.stdout)
    );
}

/// discover refuses a directory that is not a home instead of guessing.
#[test]
fn discover_refuses_a_directory_without_a_home_config() {
    let root = TempDir::new().unwrap();
    let plain = root.path().join("plain");
    fs::create_dir_all(&plain).unwrap();
    let output = cli(&["discover", plain.to_str().unwrap()], None);
    assert_eq!(common::exit_code(&output), common::EXIT_ERROR);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("run `mem-adaptor init"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Collects every JSON document under a receipts tree so assertions can check them all.
fn walk_json(root: &Path) -> Vec<Value> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                found.push(serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap());
            }
        }
    }
    found
}
