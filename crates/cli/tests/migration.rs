//! Exercises real CLI plan/approval/receipt artifacts and target bytes using isolated synthetic sources.
//! Groups tests by report lifecycle, pre-write refusal, coverage/privacy, historical evidence, and ZIP integration.
//! Helpers import the implementation-linked schemas; the small Python check is not full independent conformance.
//! Reading order is not execution order, and these tests do not cover all post-write or receipt-save failures.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use jsonschema::{Draft, Registry};
use serde_json::Value;
use tempfile::TempDir;

// Test support: synthetic CLI invocations, byte snapshots, and local schema validation.

/// Runs the built CLI with captured output and stage logs, without a terminal approval prompt.
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(args)
        .env("LOG_LEVEL", "info")
        .output()
        .unwrap()
}

/// Captures relative file paths and exact bytes to check target side effects, not just command status.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    /// Recursively captures this isolated fixture's files; it is not a production snapshot backend.
    fn visit(root: &Path, path: &Path, output: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, output);
            } else {
                output.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut output = BTreeMap::new();
    if root.exists() {
        visit(root, root, &mut output);
    }
    output
}

/// Creates two synthetic Markdown records with distinct newline forms and an initially absent target.
fn fixture() -> TempDir {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(
        directory.path().join("source/a.md"),
        "# Synthetic\n\n保持原文。\r\n",
    )
    .unwrap();
    fs::write(directory.path().join("source/b.md"), "No trailing newline").unwrap();
    directory
}

/// Saves a named plan with default settings without approving writes.
fn plan(directory: &TempDir, name: &str) -> Output {
    plan_options(directory, name, &[])
}

/// Plans into this fixture's target, optionally selecting policy or prior receipt evidence.
fn plan_options(directory: &TempDir, name: &str, options: &[&str]) -> Output {
    let source = directory.path().join("source");
    let target = format!("okf:{}", directory.path().join("target").display());
    let report = directory.path().join(name);
    let mut args = vec![
        "plan",
        source.to_str().unwrap(),
        "--to",
        &target,
        "--report",
        report.to_str().unwrap(),
    ];
    args.extend_from_slice(options);
    cli(&args)
}

/// Explicitly approves the named fixture plan; the CLI saves approval and, if completed, a receipt.
fn apply(directory: &TempDir, name: &str) -> Output {
    cli(&[
        "apply",
        directory.path().join(name).to_str().unwrap(),
        "--yes",
    ])
}

/// Reads an actual CLI-produced JSON artifact rather than a hand-built successful report.
fn document(directory: &TempDir, name: &str) -> Value {
    serde_json::from_slice(&fs::read(directory.path().join(name)).unwrap()).unwrap()
}

/// Checks structure and formats against all five local schemas, not digest correctness or truth of a write.
fn assert_schema(name: &str, document: &Value) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    let mut registry = Registry::new();
    let mut selected = Value::Null;
    for schema_name in [
        "canonical-record",
        "plan-report",
        "receipt-report",
        "approval-receipt",
        "config",
    ] {
        let schema: Value = serde_json::from_slice(
            &fs::read(root.join(format!("{schema_name}.schema.json"))).unwrap(),
        )
        .unwrap();
        if schema_name == name {
            selected = schema.clone();
        }
        let id = schema["$id"].as_str().unwrap().to_owned();
        registry = registry.add(id, schema).unwrap();
    }
    let registry = registry.prepare().unwrap();
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_registry(&registry)
        .should_validate_formats(true)
        .build(&selected)
        .unwrap();
    let errors: Vec<_> = validator
        .iter_errors(document)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

// Report lifecycle and digest stability: proposed work is separate from actual write evidence.

/// Checks a valid plan's coverage while proving all existing target bytes remain unchanged.
#[test]
fn plan_leaves_existing_target_bytes_unchanged() {
    let directory = fixture();
    let target = directory.path().join("target");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("sentinel.md"), "Synthetic target sentinel").unwrap();
    let before = snapshot(&target);
    let output = plan(&directory, "plan.json");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(snapshot(&target), before);
    let report: Value =
        serde_json::from_slice(&fs::read(directory.path().join("plan.json")).unwrap()).unwrap();
    assert_schema("plan-report", &report);
    assert_eq!(report["entries"].as_array().unwrap().len(), 2);
}

/// Checks actual approval/receipt shape, supported read-back success, preserved source bytes, and stage logs.
#[test]
fn approved_apply_produces_verified_schema_valid_receipt() {
    let directory = fixture();
    assert!(plan(&directory, "plan.json").status.success());
    let output = cli(&[
        "apply",
        directory.path().join("plan.json").to_str().unwrap(),
        "--yes",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value =
        serde_json::from_slice(&fs::read(directory.path().join("plan.receipt.json")).unwrap())
            .unwrap();
    assert_schema("receipt-report", &receipt);
    let approval: Value =
        serde_json::from_slice(&fs::read(directory.path().join("plan.approval.json")).unwrap())
            .unwrap();
    assert_schema("approval-receipt", &approval);
    for entry in receipt["entries"].as_array().unwrap() {
        assert_eq!(entry["verification"]["status"], "verified");
        let source = fs::read(
            directory
                .path()
                .join("source")
                .join(entry["source_locator"].as_str().unwrap()),
        )
        .unwrap();
        let target = fs::read(
            directory
                .path()
                .join("target")
                .join(format!("{}.md", entry["target_id"].as_str().unwrap())),
        )
        .unwrap();
        assert!(target.ends_with(&source));
    }
    let logs = String::from_utf8(output.stderr).unwrap();
    for stage in 1..=9 {
        assert!(logs.contains(&format!("[S{stage}]")));
    }
}

/// Checks repeated planning preserves execution inputs and digest despite fresh report metadata.
#[test]
fn unchanged_source_produces_same_digest() {
    let directory = fixture();
    assert!(plan(&directory, "first.json").status.success());
    assert!(plan(&directory, "second.json").status.success());
    let first: Value =
        serde_json::from_slice(&fs::read(directory.path().join("first.json")).unwrap()).unwrap();
    let second: Value =
        serde_json::from_slice(&fs::read(directory.path().join("second.json")).unwrap()).unwrap();
    assert_eq!(first["plan_digest"], second["plan_digest"]);
    assert_eq!(first["digest_inputs"], second["digest_inputs"]);
}

/// Recomputes this fixture's digest in Python without importing Rust; this subset does not cover all JCS cases.
#[test]
fn python_recomputes_digest_without_importing_engine() {
    let directory = fixture();
    assert!(plan(&directory, "plan.json").status.success());
    let output = Command::new("python3").arg("-c").arg(
        "import hashlib,json,sys; p=json.load(open(sys.argv[1])); b=json.dumps(p['digest_inputs'],sort_keys=True,separators=(',',':'),ensure_ascii=False).encode(); assert p['plan_digest']=='sha256:'+hashlib.sha256(b).hexdigest()"
    ).arg(directory.path().join("plan.json")).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Pre-write refusal: consent, execution-basis freshness, and report paths are checked before target writes.

/// Checks stale-source refusal before target creation and without a saved receipt; approval-file state is not asserted.
#[test]
fn changed_source_is_refused_before_target_write() {
    let directory = fixture();
    assert!(plan(&directory, "plan.json").status.success());
    fs::write(
        directory.path().join("source/a.md"),
        "Changed synthetic memory",
    )
    .unwrap();
    let output = cli(&[
        "apply",
        directory.path().join("plan.json").to_str().unwrap(),
        "--yes",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest mismatch"));
    assert!(!directory.path().join("target").exists());
    assert!(!directory.path().join("plan.receipt.json").exists());
}

/// Checks absent noninteractive consent produces neither target writes nor a local approval artifact.
#[test]
fn noninteractive_apply_requires_explicit_approval() {
    let directory = fixture();
    assert!(plan(&directory, "plan.json").status.success());
    let output = cli(&[
        "apply",
        directory.path().join("plan.json").to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Noninteractive apply requires explicit --yes")
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    assert!(!directory.path().join("target").exists());
    assert!(!directory.path().join("plan.approval.json").exists());
}

/// Checks an output-report path inside the target is refused before creating that target.
#[test]
fn reports_cannot_write_into_source_or_target() {
    let directory = fixture();
    let output = cli(&[
        "plan",
        directory.path().join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", directory.path().join("target").display()),
        "--report",
        directory.path().join("target/plan.json").to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("Report must be outside source and target directories")
    );
    assert!(!directory.path().join("target").exists());
}

/// Checks a target edit after planning invalidates the approved basis and preserves actual target bytes.
#[test]
fn target_change_after_approval_is_rejected_without_overwriting() {
    let directory = fixture();
    assert!(plan(&directory, "first.json").status.success());
    assert!(apply(&directory, "first.json").status.success());
    fs::write(
        directory.path().join("source/a.md"),
        "Approved source update",
    )
    .unwrap();
    let previous = directory.path().join("first.receipt.json");
    assert!(
        plan_options(
            &directory,
            "update.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let receipt = document(&directory, "first.receipt.json");
    let entry = receipt["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "a.md")
        .unwrap();
    let path = directory
        .path()
        .join("target")
        .join(format!("{}.md", entry["target_id"].as_str().unwrap()));
    let target = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{target}\nConcurrent user edit")).unwrap();
    let before = snapshot(&directory.path().join("target"));
    let output = apply(&directory, "update.json");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("digest mismatch"));
    assert_eq!(snapshot(&directory.path().join("target")), before);
}

// Coverage and privacy: reports disclose gaps and policy without exposing detected values.

/// Checks all supported synthetic signatures are reported and masked across pass, block, and allowlist modes.
/// Checks policy-specific export counts; masking reports does not mean pass-policy target content is redacted.
#[test]
fn all_secret_signatures_stay_out_of_reports_and_output_in_every_policy() {
    let secrets = [
        ("github-pat", format!("ghp_TEST{}", "A".repeat(32))),
        (
            "github-fine-grained-pat",
            format!("github_pat_TEST{}", "A".repeat(78)),
        ),
        (
            "anthropic-api-key",
            format!("sk-ant-api03-TEST{}AA", "A".repeat(89)),
        ),
        ("aws-access-token", format!("AKIATEST{}", "A".repeat(12))),
        (
            "openai-api-key",
            format!("sk-TEST{}T3BlbkFJTEST{}", "A".repeat(16), "A".repeat(16)),
        ),
        (
            "private-key",
            format!(
                "-----BEGIN PRIVATE KEY-----\nTEST{}\n-----END PRIVATE KEY-----",
                "A".repeat(64)
            ),
        ),
        ("password-assignment", "TEST_SYNTHETIC_PASSWORD".into()),
    ];
    for mode in ["pass", "block", "allowlist"] {
        let directory = fixture();
        let body = secrets
            .iter()
            .map(|(rule, value)| {
                if *rule == "password-assignment" {
                    format!("password={value}")
                } else {
                    value.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(directory.path().join("source/secrets.md"), &body).unwrap();
        let mut options = vec![
            "--secret-policy",
            if mode == "pass" { "pass" } else { "block" },
        ];
        if mode == "allowlist" {
            for (rule, _) in &secrets {
                options.extend(["--allow-rule", rule]);
            }
        }
        let plan_output = plan_options(&directory, "plan.json", &options);
        assert!(
            plan_output.status.success(),
            "{}",
            String::from_utf8_lossy(&plan_output.stderr)
        );
        let report = document(&directory, "plan.json");
        assert_schema("plan-report", &report);
        let entry = report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["source_record_id"] == "secrets.md")
            .unwrap();
        assert_eq!(
            entry["sensitive_findings"].as_array().unwrap().len(),
            secrets.len()
        );
        assert_eq!(
            entry["disposition"]["status"],
            if mode == "block" {
                "rejected"
            } else {
                "accepted"
            }
        );
        let output = apply(&directory, "plan.json");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt = document(&directory, "plan.receipt.json");
        assert_schema("receipt-report", &receipt);
        let mut observable = Vec::new();
        for output in [plan_output, output] {
            observable.extend_from_slice(&output.stdout);
            observable.extend_from_slice(&output.stderr);
        }
        for name in ["plan.json", "plan.receipt.json", "plan.approval.json"] {
            observable.extend(fs::read(directory.path().join(name)).unwrap());
        }
        fs::write(directory.path().join("synthetic.log"), &observable).unwrap();
        let observable =
            String::from_utf8(fs::read(directory.path().join("synthetic.log")).unwrap()).unwrap();
        for (_, secret) in &secrets {
            assert!(!observable.contains(secret), "Secret leaked in mode {mode}");
        }
        let target_count = snapshot(&directory.path().join("target/memories")).len();
        assert_eq!(target_count, if mode == "block" { 2 } else { 3 });
    }
}

/// Checks default policy provenance and the warning that an untracked nonempty target lacks deletion protection.
#[test]
fn default_policy_and_nonempty_target_warning_are_explicit() {
    let directory = fixture();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(directory.path().join("target/existing.md"), "Synthetic").unwrap();
    let output = plan(&directory, "plan.json");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Policy comes from defaults, not a user choice")
    );
    let report = document(&directory, "plan.json");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap()
                .contains("deletion protection unavailable"))
    );
}

/// Checks unclaimed-file coverage and unsupported secret-reference omission while other records still migrate.
#[test]
fn unclaimed_files_and_secret_references_are_reported_without_exporting_values() {
    let directory = fixture();
    fs::write(directory.path().join("source/unknown.bin"), [0, 1, 2]).unwrap();
    let fake = format!("ghp_TEST{}", "A".repeat(32));
    fs::write(
        directory.path().join("source/reference.md"),
        format!("---\ntype: secretRef\npassword: TEST_METADATA_PASSWORD\n---\n{fake}"),
    )
    .unwrap();
    assert!(plan(&directory, "plan.json").status.success());
    let report = document(&directory, "plan.json");
    assert!(
        report["source_inventory"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "unknown.bin" && file["status"] == "unclaimed")
    );
    let entry = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "reference.md")
        .unwrap();
    assert_eq!(
        entry["disposition"]["reason"]["code"],
        "secret_reference_unsupported"
    );
    assert!(entry["sensitive_findings"].as_array().unwrap().len() >= 2);
    assert!(apply(&directory, "plan.json").status.success());
    assert_eq!(snapshot(&directory.path().join("target/memories")).len(), 2);
    assert!(
        !fs::read_to_string(directory.path().join("plan.json"))
            .unwrap()
            .contains(&fake)
    );
}

// Historical evidence: skipped runs carry earlier proof without claiming fresh verification.

/// Checks a no-new-write receipt preserves prior verified evidence and protects a later target deletion.
#[test]
fn empty_apply_preserves_deletion_protection_across_receipts() {
    let directory = fixture();
    assert!(plan(&directory, "first.json").status.success());
    assert!(apply(&directory, "first.json").status.success());
    let before = snapshot(&directory.path().join("target"));
    let previous = directory.path().join("first.receipt.json");
    assert!(
        plan_options(
            &directory,
            "second.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let report = document(&directory, "second.json");
    for entry in report["entries"].as_array().unwrap() {
        assert_eq!(entry["disposition"]["reason"]["code"], "already_migrated");
    }
    assert!(apply(&directory, "second.json").status.success());
    assert_eq!(snapshot(&directory.path().join("target")), before);
    let receipt = document(&directory, "second.receipt.json");
    for entry in receipt["entries"].as_array().unwrap() {
        assert!(entry.get("verification").is_none());
        assert_eq!(entry["prior_write"]["verification"]["status"], "verified");
    }
    let deleted = receipt["entries"][0]["prior_write"]["target_id"]
        .as_str()
        .unwrap();
    fs::remove_file(
        directory
            .path()
            .join("target")
            .join(format!("{deleted}.md")),
    )
    .unwrap();
    let previous = directory.path().join("second.receipt.json");
    assert!(
        plan_options(
            &directory,
            "third.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let third = document(&directory, "third.json");
    assert!(
        third["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["disposition"]["reason"]["code"] == "deleted_in_target")
    );
    assert!(apply(&directory, "third.json").status.success());
    assert!(
        !directory
            .path()
            .join("target")
            .join(format!("{deleted}.md"))
            .exists()
    );
}

/// Checks stable target IDs for source updates and unresolved treatment without writes after both sides change.
#[test]
fn source_updates_use_stable_target_ids_and_double_changes_stay_unresolved() {
    let directory = fixture();
    assert!(plan(&directory, "first.json").status.success());
    assert!(apply(&directory, "first.json").status.success());
    let first = document(&directory, "first.receipt.json");
    let entry = first["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "a.md")
        .unwrap();
    let target_id = entry["target_id"].as_str().unwrap();
    fs::write(directory.path().join("source/a.md"), "Synthetic update").unwrap();
    let previous = directory.path().join("first.receipt.json");
    assert!(
        plan_options(
            &directory,
            "update.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let output = apply(&directory, "update.json");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(snapshot(&directory.path().join("target/memories")).len(), 2);
    let path = directory
        .path()
        .join("target")
        .join(format!("{target_id}.md"));
    let target = fs::read_to_string(&path).unwrap();
    assert!(target.ends_with("Synthetic update"));
    fs::write(&path, format!("{target}\nUser target edit")).unwrap();
    fs::write(directory.path().join("source/a.md"), "Another source edit").unwrap();
    let before = snapshot(&directory.path().join("target"));
    let previous = directory.path().join("update.receipt.json");
    assert!(
        plan_options(
            &directory,
            "conflict.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let conflict = document(&directory, "conflict.json");
    let entry = conflict["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "a.md")
        .unwrap();
    assert_eq!(entry["disposition"]["reason"]["code"], "target_modified");
    assert!(apply(&directory, "conflict.json").status.success());
    assert_eq!(snapshot(&directory.path().join("target")), before);
}

/// Checks duplicate representative evidence is retained and does not resurrect a deleted target.
#[test]
fn duplicate_aliases_do_not_resurrect_deleted_survivors() {
    let directory = fixture();
    fs::write(
        directory.path().join("source/a.md"),
        "Same synthetic memory",
    )
    .unwrap();
    fs::write(
        directory.path().join("source/b.md"),
        "Same  synthetic\nmemory",
    )
    .unwrap();
    assert!(plan(&directory, "first.json").status.success());
    let report = document(&directory, "first.json");
    assert_eq!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["disposition"]["status"] == "accepted")
            .count(),
        1
    );
    assert!(apply(&directory, "first.json").status.success());
    let receipt = document(&directory, "first.receipt.json");
    assert!(
        receipt["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["prior_write"].is_object()
                || entry["duplicate_write"]["prior_write"].is_object())
    );
    let target = receipt["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["verification"]["status"] == "verified")
        .unwrap()["target_id"]
        .as_str()
        .unwrap();
    fs::remove_file(directory.path().join("target").join(format!("{target}.md"))).unwrap();
    let previous = directory.path().join("first.receipt.json");
    assert!(
        plan_options(
            &directory,
            "second.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let report = document(&directory, "second.json");
    assert!(
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["disposition"]["reason"]["code"] == "deleted_in_target")
    );
    assert!(apply(&directory, "second.json").status.success());
    assert!(snapshot(&directory.path().join("target/memories")).is_empty());
}

/// Checks source disappearance carries prior write evidence across receipts and protects a subsequently deleted target.
#[test]
fn temporarily_missing_source_record_keeps_historical_deletion_state() {
    let directory = fixture();
    assert!(plan(&directory, "first.json").status.success());
    assert!(apply(&directory, "first.json").status.success());
    let original = fs::read(directory.path().join("source/a.md")).unwrap();
    fs::remove_file(directory.path().join("source/a.md")).unwrap();
    let previous = directory.path().join("first.receipt.json");
    assert!(
        plan_options(
            &directory,
            "missing.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    assert!(apply(&directory, "missing.json").status.success());
    let receipt = document(&directory, "missing.receipt.json");
    let old = receipt["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "a.md")
        .unwrap();
    assert_eq!(old["disposition"]["reason"]["code"], "source_missing");
    let target = directory.path().join("target").join(format!(
        "{}.md",
        old["prior_write"]["target_id"].as_str().unwrap()
    ));
    fs::remove_file(&target).unwrap();
    fs::write(directory.path().join("source/a.md"), original).unwrap();
    let previous = directory.path().join("missing.receipt.json");
    assert!(
        plan_options(
            &directory,
            "returned.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let report = document(&directory, "returned.json");
    let entry = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["source_record_id"] == "a.md")
        .unwrap();
    assert_eq!(entry["disposition"]["reason"]["code"], "deleted_in_target");
    assert!(apply(&directory, "returned.json").status.success());
    assert!(!target.exists());
}

// ZIP integration: source preflight remains distinct from approved writes and receipt verification.

/// Checks safe ZIP input yields verified output while dangerous paths fail without target or traversal writes.
#[test]
fn approved_safe_zip_is_migrated_and_malicious_zip_never_touches_target() {
    use std::io::Write;
    for name in ["nested/synthetic.md", "../outside.md", "/absolute.md"] {
        let directory = fixture();
        let path = directory.path().join("synthetic.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"Synthetic ZIP memory").unwrap();
        zip.finish().unwrap();
        let output = cli(&[
            "plan",
            path.to_str().unwrap(),
            "--to",
            &format!("okf:{}", directory.path().join("target").display()),
            "--report",
            directory.path().join("plan.json").to_str().unwrap(),
        ]);
        if name.starts_with("nested") {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(apply(&directory, "plan.json").status.success());
            let receipt = document(&directory, "plan.receipt.json");
            assert_eq!(receipt["entries"][0]["verification"]["status"], "verified");
        } else {
            assert!(!output.status.success());
            assert!(!directory.path().join("target").exists());
            assert!(!directory.path().join("outside.md").exists());
        }
    }
}
