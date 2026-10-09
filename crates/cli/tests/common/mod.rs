//! Shared support for CLI integration tests: isolates the direct-mode user configuration so no test reads or
//! writes the developer's real `~/.config/mem-adaptor/config.toml` (DEC-1, DEC-19). Test binaries declare
//! `mod common;` and pass this path to every spawned CLI as `XDG_CONFIG_HOME`.

// This module is compiled into every CLI test binary, and not every binary uses every helper.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Output;

/// Documented CLI exit codes (m6-cli-proposal §3): 0 complete success, 3 completed with entries that
/// still need a human decision (rejected, unresolved or unverified), 4 a stale approved execution
/// basis, 1 input/schema/I/O failure, 2 usage error. Tests assert the exact code so a different
/// failure can never pass as the expected incomplete run.
pub const EXIT_OK: i32 = 0;
pub const EXIT_INCOMPLETE: i32 = 3;
pub const EXIT_BASIS: i32 = 4;
pub const EXIT_ERROR: i32 = 1;
pub const EXIT_USAGE: i32 = 2;

/// Returns the process exit code, or -1 when the process was killed by a signal instead of exiting.
pub fn exit_code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Asserts the documented code for a run whose report the test already parsed: 3 exactly when some
/// entry still needs a human decision. Omitted entries (duplicates, already migrated, home-modified,
/// deleted in target) are not failures. Basis mismatches and usage errors never reach this helper.
pub fn assert_plan_exit(output: &Output, report: &serde_json::Value) {
    assert_eq!(
        exit_code(output),
        expected_code(report),
        "exit code must follow the plan's own dispositions; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Asserts the documented code for an apply whose plan the test already parsed: rejected and unresolved
/// entries stay unwritten and keep the run at 3, while a fully written or fully omitted run exits 0.
pub fn assert_apply_exit(output: &Output, plan: &serde_json::Value) {
    assert_eq!(
        exit_code(output),
        expected_code(plan),
        "exit code must follow the plan's own dispositions; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The documented code for a parsed plan report: 3 when any entry is rejected or unresolved, or when a
/// written entry's verification does not match (the same rule the CLI applies to receipts).
fn expected_code(report: &serde_json::Value) -> i32 {
    let open = report["entries"].as_array().unwrap().iter().any(|entry| {
        matches!(
            entry["disposition"]["status"].as_str(),
            Some("rejected" | "unresolved")
        ) || entry["verification"]["status"] == "mismatch"
    });
    if open { EXIT_INCOMPLETE } else { EXIT_OK }
}

/// Returns an isolated `XDG_CONFIG_HOME` root unique to this test process. The directory is deliberately never
/// created, so every spawned CLI sees "no stored configuration" and takes the documented first-run default
/// instead of inheriting whatever a real user configuration happens to say. Tests that need a stored
/// configuration point `XDG_CONFIG_HOME` at their own temporary directory instead of using this helper.
pub fn config_home() -> PathBuf {
    std::env::temp_dir().join(format!("mem-adaptor-cli-tests-{}", std::process::id()))
}
