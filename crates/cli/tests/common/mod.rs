//! Shared support for CLI integration tests: isolates the direct-mode user configuration so no test reads or
//! writes the developer's real `~/.config/mem-adaptor/config.toml` (DEC-1, DEC-19). Test binaries declare
//! `mod common;` and pass this path to every spawned CLI as `XDG_CONFIG_HOME`.

use std::path::PathBuf;

/// Returns an isolated `XDG_CONFIG_HOME` root unique to this test process. The directory is deliberately never
/// created, so every spawned CLI sees "no stored configuration" and takes the documented first-run default
/// instead of inheriting whatever a real user configuration happens to say. Tests that need a stored
/// configuration point `XDG_CONFIG_HOME` at their own temporary directory instead of using this helper.
pub fn config_home() -> PathBuf {
    std::env::temp_dir().join(format!("mem-adaptor-cli-tests-{}", std::process::id()))
}
