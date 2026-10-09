//! Checks M3 failure categories and actual side effects with synthetic Readers and the real CLI.
//! Groups source/inventory, canonical validation, secret policy, history and final-receipt failures.
//! All files and fault paths are isolated; no real credentials, permission assumptions or race sleeps are used.
//! These implementation-based tests are not independent conformance or a safe-retry/recovery contract.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use mem_adaptor_core::canonical::{CanonicalRecord, Embedding};
use mem_adaptor_core::engine::{Engine, canonical_id, content_hash, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::{Disposition, UnresolvedReason};
use mem_adaptor_writer_okf::OkfWriter;
use serde_json::{Value, json};
use tempfile::TempDir;

mod common;

// Support: observe exact bytes and run the same CLI that users invoke.

/// Creates a synthetic source and an existing unmanaged target sentinel to detect unintended writes.
fn fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(directory.path().join("source/note.md"), "Synthetic memory").unwrap();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(
        directory.path().join("target/user.txt"),
        b"Protected user bytes",
    )
    .unwrap();
    directory
}

/// Captures every relative target file and byte, not just the expected memory path.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            for (child, bytes) in snapshot(&path) {
                files.insert(format!("{name}/{child}"), bytes);
            }
        } else {
            files.insert(name.into(), fs::read(path).unwrap());
        }
    }
    files
}

/// Plans a chosen source with captured stdout and stage logs, without any interactive input.
/// The isolated configuration root keeps direct-mode runs away from the real user configuration.
fn plan(directory: &TempDir, source: &Path, report: &str, options: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(["plan", source.to_str().unwrap(), "--to"])
        .arg(format!("okf:{}", directory.path().join("target").display()))
        .arg("--report")
        .arg(directory.path().join(report))
        .args(options)
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home())
        .output()
        .unwrap()
}

/// Explicitly approves a fixture plan and optionally selects a deterministic failing receipt path.
fn apply(directory: &TempDir, report: &str, receipt: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"));
    command
        .arg("apply")
        .arg(directory.path().join(report))
        .arg("--yes");
    if let Some(receipt) = receipt {
        command.arg("--receipt").arg(receipt);
    }
    command
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home())
        .output()
        .unwrap()
}

/// Requires an ordinary CLI refusal, no panic or successful artifact announcement.
fn assert_failure(output: &Output, cause: &str) {
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(cause),
        "Expected non-sensitive cause: {cause}"
    );
    assert!(!stderr.contains("panicked"));
    assert!(!stdout.contains("Plan:") && !stdout.contains("Receipt:"));
}

/// Reads an actual generated report rather than constructing successful execution evidence by hand.
fn document(directory: &TempDir, name: &str) -> Value {
    serde_json::from_slice(&fs::read(directory.path().join(name)).unwrap()).unwrap()
}

// Source/inventory: failures must not become partial successful plans.

/// Stops on missing source, corrupt ZIP and claimed malformed JSON even beside a good Markdown file.
#[test]
fn cli_source_failures_preserve_targets_and_produce_no_plan() {
    for case in ["missing", "zip", "json"] {
        let directory = fixture();
        let root = directory.path();
        let source = match case {
            "missing" => root.join("missing"),
            "zip" => {
                fs::write(root.join("bad.zip"), b"not a ZIP").unwrap();
                root.join("bad.zip")
            }
            _ => {
                fs::write(root.join("source/memory.json"), b"{").unwrap();
                root.join("source")
            }
        };
        let before = snapshot(&root.join("target"));
        let output = plan(&directory, &source, "failed.json", &[]);
        assert_failure(&output, "Planning failed before target writes");
        assert!(String::from_utf8_lossy(&output.stderr).contains("target unchanged"));
        assert_eq!(snapshot(&root.join("target")), before);
        for name in ["failed.json", "failed.approval.json", "failed.receipt.json"] {
            assert!(!root.join(name).exists());
        }
    }
}

/// Checks the real CLI rejects child symlinks while preserving the outside sentinel.
#[cfg(unix)]
#[test]
fn cli_source_symlink_refuses_without_outside_or_target_changes() {
    let directory = fixture();
    let root = directory.path();
    fs::write(root.join("outside.md"), b"Outside synthetic bytes").unwrap();
    std::os::unix::fs::symlink(root.join("outside.md"), root.join("source/link.md")).unwrap();
    let before = snapshot(&root.join("target"));
    let output = plan(&directory, &root.join("source"), "failed.json", &[]);
    assert_failure(&output, "Source symlinks are not supported");
    assert_eq!(snapshot(&root.join("target")), before);
    assert_eq!(
        fs::read(root.join("outside.md")).unwrap(),
        b"Outside synthetic bytes"
    );
    assert!(!root.join("failed.json").exists());
}

/// Exercises duplicate, unsupported and bad-CRC archives through the CLI, not only the loader.
#[test]
fn cli_zip_failures_leave_target_and_report_set_unchanged() {
    for case in ["duplicate", "compression", "crc"] {
        let directory = fixture();
        let root = directory.path();
        let path = root.join("source.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for name in ["first.md", "other.md"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(b"Synthetic memory").unwrap();
        }
        zip.finish().unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let central = bytes
            .windows(4)
            .position(|part| part == b"PK\x01\x02")
            .unwrap();
        let cause = match case {
            "duplicate" => {
                let mut replacements = 0;
                for index in 0..=bytes.len() - 8 {
                    if &bytes[index..index + 8] == b"other.md" {
                        bytes[index..index + 8].copy_from_slice(b"first.md");
                        replacements += 1;
                    }
                }
                assert_eq!(replacements, 2);
                "Duplicate ZIP path"
            }
            "compression" => {
                bytes[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
                bytes[central + 10..central + 12].copy_from_slice(&u16::MAX.to_le_bytes());
                "compression method not supported"
            }
            _ => {
                bytes[central + 16] ^= 1;
                "Invalid checksum"
            }
        };
        fs::write(&path, bytes).unwrap();
        let before = snapshot(&root.join("target"));
        let output = plan(&directory, &path, "failed.json", &[]);
        assert_failure(&output, cause);
        assert_eq!(snapshot(&root.join("target")), before);
        for name in ["failed.json", "failed.approval.json", "failed.receipt.json"] {
            assert!(!root.join(name).exists());
        }
    }
}

#[derive(Clone, Copy)]
enum Fault {
    None,
    Schema,
    Identity,
    Hash,
    Dimension,
    Number,
    NoSource,
    Association,
    Duplicate,
    UnknownClaim,
    ReadIo,
}

struct FaultReader {
    id: &'static str,
    fault: Fault,
}

impl Reader for FaultReader {
    /// Gives independently registered Readers distinct IDs for competing-claim tests.
    fn id(&self) -> &'static str {
        self.id
    }
    /// Labels this fault-injection adapter as synthetic.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Fault fixtures are directories held in place for the run, so their path may bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
    }
    /// Claims one real fixture path or deliberately names a path absent from the source inventory.
    fn claim(&self, _: &FileInventory) -> Vec<Claim> {
        vec![Claim {
            path: if matches!(self.fault, Fault::UnknownClaim) {
                "missing.md"
            } else {
                "note.md"
            }
            .into(),
            layer: "memory".into(),
            registered_only: false,
        }]
    }
    /// Injects one ordinary I/O failure or invalid canonical claim, never a panic or target write.
    fn read(&self, claim: &Claim, _: &SourceFs) -> mem_adaptor_core::Result<ReaderOutput> {
        if matches!(self.fault, Fault::ReadIo) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Injected Reader read failure",
            )
            .into());
        }
        let vector: Value = serde_json::from_str(include_str!(
            "../../../schema/vectors/valid/canonical-minimal.json"
        ))
        .unwrap();
        let mut record: CanonicalRecord =
            serde_json::from_value(vector["document"].clone()).unwrap();
        record.source.system = "synthetic".into();
        record.source_record_id = "note".into();
        record.source_locator = claim.path.clone();
        record.canonical_id = canonical_id("synthetic", "", "note");
        record.content_hash = content_hash(record.content.as_bytes());
        let mut source =
            mem_adaptor_core::reader::source(&record, &claim.path, json!({"body": record.content}));
        mem_adaptor_core::reader::map(&mut source, "/body", "/content");
        match self.fault {
            Fault::Schema => record.source_record_id = String::new(),
            Fault::Identity => record.canonical_id = canonical_id("synthetic", "", "another"),
            Fault::Hash => record.content_hash = content_hash(b"another"),
            Fault::Dimension => {
                record.embedding = Some(Embedding {
                    model: "synthetic".into(),
                    dim: 2,
                    vector: Some(vec![0.1]),
                    normalized: None,
                })
            }
            Fault::Number => {
                record.source_extra = Some(
                    serde_json::from_value(json!({"unsafe": 9_007_199_254_740_992_u64})).unwrap(),
                )
            }
            Fault::Association => source.source_record_id = "another".into(),
            _ => {}
        }
        let mut output = mem_adaptor_core::reader::output();
        output.records.push(record.clone());
        if matches!(self.fault, Fault::Duplicate) {
            output.records.push(record);
        }
        if !matches!(self.fault, Fault::NoSource) {
            output.source_records.push(source);
        }
        Ok(output)
    }
}

/// Registers isolated fault Readers and a real Writer; planning refusal cannot alter the target sentinel.
fn fault_engine(directory: &TempDir, faults: &[Fault]) -> Engine {
    let mut registry = Registry::default();
    for (index, fault) in faults.iter().enumerate() {
        registry
            .register_reader(FaultReader {
                id: if index == 0 { "first" } else { "second" },
                fault: *fault,
            })
            .unwrap();
    }
    registry
        .register_writer(
            "home".into(),
            OkfWriter::new(directory.path().join("target")).unwrap(),
        )
        .unwrap();
    Engine { registry }
}

/// Selects explicit pass settings without asserting that detection is disabled.
fn policy() -> GatePolicy {
    GatePolicy {
        secrets: GateAction::Pass,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::UserChoice,
        user_selected: true,
    }
}

/// Rejects competing/unknown claims and preserves a Reader's underlying I/O kind, not a swallowed skip.
#[test]
fn reader_claim_and_read_failures_return_ordinary_errors_without_writes() {
    for (faults, cause, io) in [
        (
            vec![Fault::None, Fault::None],
            "File claimed by multiple Readers",
            false,
        ),
        (
            vec![Fault::UnknownClaim],
            "Reader claimed an unknown file",
            false,
        ),
        (vec![Fault::ReadIo], "Injected Reader read failure", true),
    ] {
        let directory = fixture();
        let before = snapshot(&directory.path().join("target"));
        let error = fault_engine(&directory, &faults)
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        assert!(format!("{error:#}").contains(cause));
        if io {
            assert_eq!(
                error.downcast_ref::<std::io::Error>().unwrap().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
        assert_eq!(snapshot(&directory.path().join("target")), before);
    }
}

// Canonical claims: structure is not enough to establish identity or execution evidence.

/// Separately checks schema, identity/hash, vector, number and source association refusals without writes.
#[test]
fn canonical_failures_are_explicit_and_do_not_write() {
    for (fault, cause) in [
        (Fault::Schema, "Schema"),
        (Fault::Identity, "Reader canonical identity mismatch"),
        (Fault::Hash, "Reader content hash mismatch"),
        (Fault::Dimension, "Embedding vector length"),
        (Fault::Number, "JCS safe range"),
        (Fault::NoSource, "Canonical record has no source record"),
        (Fault::Association, "Source identity association mismatch"),
        (Fault::Duplicate, "Duplicate canonical identity"),
    ] {
        let directory = fixture();
        let before = snapshot(&directory.path().join("target"));
        let error = fault_engine(&directory, &[fault])
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        assert!(
            error.to_string().contains(cause),
            "Expected non-sensitive category: {cause}; {error}"
        );
        assert_eq!(snapshot(&directory.path().join("target")), before);
    }
}

// Secret policy: a rejected record is an accurately reported disposition, not a failed command.

/// Covers the punctuation regression across real CLI policies, masked artifacts and raw target semantics.
#[test]
fn cli_punctuation_secret_policies_match_reports_and_target_bytes() {
    for (action, allow, expected, writes) in [
        ("pass", false, "passed", true),
        ("block", false, "blocked", false),
        ("block", true, "allowlisted", true),
    ] {
        let directory = fixture();
        let root = directory.path();
        let token = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
        fs::write(root.join("source/note.md"), format!("Synthetic ({token}),")).unwrap();
        let before = snapshot(&root.join("target"));
        let mut options = vec!["--secret-policy", action];
        if allow {
            options.extend(["--allow-rule", "openai-api-key"]);
        }
        let planned = plan(&directory, &root.join("source"), "plan.json", &options);
        // A blocked record is an accurately reported disposition, not a failed command, but the run is
        // not a complete success either: exit 3 keeps "nothing needs a decision" as exit 0.
        assert_eq!(
            common::exit_code(&planned),
            if writes {
                common::EXIT_OK
            } else {
                common::EXIT_INCOMPLETE
            },
            "{}",
            String::from_utf8_lossy(&planned.stderr)
        );
        assert_eq!(snapshot(&root.join("target")), before);
        let report = document(&directory, "plan.json");
        assert_eq!(
            report["entries"][0]["sensitive_findings"][0]["disposition"],
            expected
        );
        assert_eq!(
            report["entries"][0]["disposition"]["status"],
            if writes { "accepted" } else { "rejected" }
        );
        let applied = apply(&directory, "plan.json", None);
        assert_eq!(
            common::exit_code(&applied),
            if writes {
                common::EXIT_OK
            } else {
                common::EXIT_INCOMPLETE
            },
            "{}",
            String::from_utf8_lossy(&applied.stderr)
        );
        let receipt = document(&directory, "plan.receipt.json");
        assert_eq!(
            receipt["entries"][0]["sensitive_findings"][0]["disposition"],
            expected
        );
        for output in [&planned, &applied] {
            for bytes in [&output.stdout, &output.stderr] {
                assert!(!String::from_utf8_lossy(bytes).contains(&token));
            }
        }
        for name in ["plan.json", "plan.approval.json", "plan.receipt.json"] {
            assert!(!String::from_utf8_lossy(&fs::read(root.join(name)).unwrap()).contains(&token));
        }
        let after = snapshot(&root.join("target"));
        assert_eq!(
            after
                .values()
                .any(|bytes| String::from_utf8_lossy(bytes).contains(&token)),
            writes
        );
        if !writes {
            assert_eq!(after, before);
        }
    }
}

// Historical evidence: explicitly supplied invalid receipts must not degrade to an initial import.

/// Rejects missing/invalid/foreign history through both Result and CLI while preserving all prior artifacts.
#[test]
fn invalid_previous_receipts_refuse_without_target_or_credential_changes() {
    for case in [
        "missing",
        "json",
        "schema",
        "source",
        "target",
        "writer",
        "duplicate",
        "no_write_state",
    ] {
        let directory = fixture();
        let root = directory.path();
        assert!(
            plan(&directory, &root.join("source"), "first.json", &[])
                .status
                .success()
        );
        assert!(apply(&directory, "first.json", None).status.success());
        let originals: Vec<_> = ["first.json", "first.approval.json", "first.receipt.json"]
            .into_iter()
            .map(|name| (name, fs::read(root.join(name)).unwrap()))
            .collect();
        let previous = root.join("bad.receipt.json");
        let mut receipt = document(&directory, "first.receipt.json");
        let expected = match case {
            "missing" => "No such file",
            "json" => "Invalid previous receipt",
            "schema" => {
                receipt["schema_version"] = json!("invalid");
                "Schema"
            }
            "source" => {
                receipt["source"]["location"] = json!(root.join("foreign").to_str().unwrap());
                "another source"
            }
            "target" => {
                receipt["targets"][0]["location"] = json!(root.join("foreign").to_str().unwrap());
                "another target"
            }
            "writer" => {
                receipt["targets"][0]["writer"] = json!("ump");
                "another target"
            }
            "duplicate" => {
                let copy = receipt["entries"][0].clone();
                receipt["entries"].as_array_mut().unwrap().push(copy);
                "Duplicate identity"
            }
            _ => {
                receipt["entries"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("prior_write");
                "Schema receipt-report violation at /entries/0"
            }
        };
        if case == "json" {
            fs::write(&previous, b"{").unwrap();
        } else if case != "missing" {
            fs::write(&previous, serde_json::to_vec(&receipt).unwrap()).unwrap();
        }
        let before = snapshot(&root.join("target"));
        let mut registry = Registry::default();
        registry
            .register_reader(mem_adaptor_reader_markdown::MarkdownReader)
            .unwrap();
        registry
            .register_writer("home".into(), OkfWriter::new(root.join("target")).unwrap())
            .unwrap();
        let error = Engine { registry }
            .plan_with_previous(&root.join("source"), policy(), Some(&previous))
            .unwrap_err();
        if case == "missing" {
            assert_eq!(
                error.downcast_ref::<std::io::Error>().unwrap().kind(),
                std::io::ErrorKind::NotFound
            );
        } else {
            assert!(error.to_string().contains(expected), "{case}: {error}");
        }
        let output = plan(
            &directory,
            &root.join("source"),
            "second.json",
            &["--previous-receipt", previous.to_str().unwrap()],
        );
        assert_failure(&output, expected);
        assert_eq!(snapshot(&root.join("target")), before);
        for (name, bytes) in originals {
            assert_eq!(fs::read(root.join(name)).unwrap(), bytes);
        }
        for name in ["second.json", "second.approval.json", "second.receipt.json"] {
            assert!(!root.join(name).exists());
        }
    }
}

// Final receipt persistence: target writes do not imply a saved receipt or a safely repeatable command.

/// Verifies the report boundary retains its ordinary NotADirectory I/O category without overwriting the parent.
#[test]
fn report_save_parent_file_preserves_io_category_and_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let parent = directory.path().join("obstruction");
    fs::write(&parent, b"Protected report obstruction").unwrap();
    let error =
        write_json_new(&parent.join("receipt.json"), &json!({"synthetic": true})).unwrap_err();
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::NotADirectory
    );
    assert_eq!(fs::read(parent).unwrap(), b"Protected report obstruction");
}

/// Fails only at final receipt save after real verified writes, retaining approval and refusing fake success/retry.
#[test]
fn final_receipt_failure_reports_already_changed_target_without_success() {
    let directory = fixture();
    let root = directory.path();
    let token = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    let parent = root.join(format!("obstruction-{token},"));
    fs::write(&parent, b"Protected report obstruction").unwrap();
    assert!(
        plan(&directory, &root.join("source"), "plan.json", &[])
            .status
            .success()
    );
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    let before = snapshot(&root.join("target"));
    let failed_path = parent.join("receipt.json");
    let output = apply(&directory, "plan.json", Some(&failed_path));
    assert_failure(&output, "[S9] Final receipt save failed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains(&token));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&token));
    for message in [
        "Not a directory",
        "targets may already have changed",
        "approval was saved",
        "No reliable final receipt",
        "Inspect targets",
        "do not blindly retry",
    ] {
        assert!(
            stderr.contains(message),
            "Expected boundary message: {message}"
        );
    }
    assert_ne!(snapshot(&root.join("target")), before);
    assert_eq!(
        fs::read(root.join("target/user.txt")).unwrap(),
        b"Protected user bytes"
    );
    assert!(
        snapshot(&root.join("target"))
            .values()
            .any(|bytes| String::from_utf8_lossy(bytes).contains("Synthetic memory"))
    );
    let approval: ApprovalReceipt =
        serde_json::from_slice(&fs::read(root.join("plan.approval.json")).unwrap()).unwrap();
    assert_eq!(
        approval.plan_digest,
        document(&directory, "plan.json")["plan_digest"]
    );
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    assert_eq!(fs::read(&parent).unwrap(), b"Protected report obstruction");
    assert!(!failed_path.exists() && !root.join("plan.receipt.json").exists());
    // The written memory can be read, but is untracked without a receipt and must not be overwritten.
    let mut registry = Registry::default();
    registry
        .register_reader(mem_adaptor_reader_markdown::MarkdownReader)
        .unwrap();
    registry
        .register_writer("home".into(), OkfWriter::new(root.join("target")).unwrap())
        .unwrap();
    let next = Engine { registry }
        .plan(&root.join("source"), policy())
        .unwrap();
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetUntracked
        }
    );
    // A second attempt must preserve the first approval rather than replacing evidence.
    let approval_bytes = fs::read(root.join("plan.approval.json")).unwrap();
    let after = snapshot(&root.join("target"));
    let second = apply(&directory, "plan.json", Some(&failed_path));
    assert_failure(&second, "Output report already exists");
    assert_eq!(snapshot(&root.join("target")), after);
    assert_eq!(
        fs::read(root.join("plan.approval.json")).unwrap(),
        approval_bytes
    );
}
