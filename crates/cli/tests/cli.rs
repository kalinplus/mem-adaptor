use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
}

#[test]
fn version_matches_package() {
    let output = cli().arg("--version").output().unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("mem-adaptor {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_states_scaffold_limitations() {
    let output = cli().arg("--help").output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("Usage: mem-adaptor"));
    assert!(stdout.contains("--version"));
    assert!(stdout.contains("synthetic inputs only"));
    assert!(stdout.contains("plan"));
    assert!(stdout.contains("apply"));
    assert!(output.stderr.is_empty());
}

#[test]
fn no_arguments_show_help_without_migrating() {
    let output = cli().output().unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("Usage: mem-adaptor"));
    assert!(output.stdout.is_empty());
}

#[test]
fn missing_migration_arguments_are_not_silently_accepted() {
    for command in ["init", "plan", "apply"] {
        let output = cli().arg(command).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
