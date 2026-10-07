//! Tests synthetic native Writer outputs, historical protection, and approved byte proofs.
//! Native snapshots check semantics separately from adapter round-trips; no real memories or model calls.

use std::fs;
use std::path::Path;
use std::process::Command;

use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::{
    Engine, canonical_id, content_hash, record_hash, timestamp, write_json_new,
};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use mem_adaptor_core::reports::*;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use mem_adaptor_writer_ump::UmpWriter;
use serde_json::{Value, json};
use tempfile::TempDir;

struct SyntheticReader(CanonicalRecord);
impl Reader for SyntheticReader {
    fn id(&self) -> &'static str {
        "synthetic-writer"
    }
    fn version(&self) -> &'static str {
        "test"
    }
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .keys()
            .map(|path| Claim {
                path: path.clone(),
                layer: "memory".into(),
                registered_only: false,
            })
            .collect()
    }
    fn read(&self, claim: &Claim, _: &SourceFs) -> mem_adaptor_core::Result<ReaderOutput> {
        let mut output = normalize::output();
        let mut original = normalize::source(&self.0, &claim.path, json!({"body": self.0.content}));
        normalize::map(&mut original, "/body", "/content");
        output.source_records.push(original);
        output.records.push(self.0.clone());
        Ok(output)
    }
}

fn record() -> CanonicalRecord {
    let mut record = normalize::record(
        "synthetic-writer",
        "test",
        "original-id",
        "synthetic.json",
        "\n# 合成标题\n\nKeep this body.\r\n",
        EvidenceLevel::Measured,
    );
    record.source_kind = Some("preference".into());
    record.dna_class = DnaClass::Dna;
    record.scope = Scope::Project;
    record.scope_qualifier = Some("synthetic-project".into());
    record.owner_declared = Some("untrusted-owner".into());
    record.tags = Some(vec!["synthetic".into()]);
    record.updated_at = Some("2026-01-02T03:04:05Z".into());
    record.source_extra = Some(json!({"frontmatter": {"title": "Original synthetic title", "description": "Original description", "custom": [true, 2, null]}}).as_object().unwrap().clone());
    record
}

fn fixture() -> TempDir {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(directory.path().join("source/synthetic.json"), "{}").unwrap();
    directory
}

/// Registers one synthetic record and a normalized physical target for the chosen Writer.
fn engine(directory: &TempDir, record: CanonicalRecord, writer: &str) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(SyntheticReader(record)).unwrap();
    let path = directory.path().join("target");
    match writer {
        "okf" => registry
            .register_writer("home".into(), OkfWriter::new(path).unwrap())
            .unwrap(),
        "ump" => registry
            .register_writer("home".into(), UmpWriter::new(path).unwrap())
            .unwrap(),
        _ => unreachable!(),
    }
    Engine { registry }
}
fn policy() -> GatePolicy {
    GatePolicy {
        secrets: GateAction::Pass,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}
fn plan(engine: &Engine, directory: &TempDir, previous: Option<&Path>) -> PlanReport {
    engine
        .plan_with_previous(&directory.path().join("source"), policy(), previous)
        .unwrap()
}
fn apply(engine: &Engine, report: &PlanReport) -> mem_adaptor_core::Result<ReceiptReport> {
    engine.apply(
        report,
        &ApprovalReceipt {
            schema_version: "0.1.0".into(),
            receipt_id: "synthetic".into(),
            plan_digest: report.plan_digest.clone(),
            approved_at: timestamp().unwrap(),
            backend: "local".into(),
            approver: "synthetic".into(),
        },
        "plan".into(),
        "approval".into(),
    )
}
fn previous(directory: &TempDir, receipt: &ReceiptReport) -> std::path::PathBuf {
    let path = directory.path().join("previous.json");
    write_json_new(&path, receipt).unwrap();
    path
}
fn ump(directory: &TempDir) -> Vec<Value> {
    serde_json::from_slice(&fs::read(directory.path().join("target/records.ump.json")).unwrap())
        .unwrap()
}

/// Captures the complete regular-file set and bytes for fault tests without following links.
fn target_snapshot(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(!metadata.file_type().is_symlink());
            if metadata.is_dir() {
                directories.push(path);
            } else {
                assert!(metadata.is_file());
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

#[test]
fn ump_uses_official_schema_preserves_full_metadata_and_marks_migration_time() {
    let directory = fixture();
    let original = record();
    let engine = engine(&directory, original.clone(), "ump");
    let report = plan(&engine, &directory, None);
    assert!(!directory.path().join("target").exists());
    assert!(
        report.entries[0]
            .target_map
            .iter()
            .any(|mapping| mapping.rule == "explicit_target_migration_created_time")
    );
    let receipt = apply(&engine, &report).unwrap();
    assert_eq!(
        receipt.entries[0].verification,
        Some(Verification::Verified)
    );
    let native = ump(&directory);
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&serde_json::from_str::<Value>(mem_adaptor_writer_ump::SCHEMA).unwrap())
        .unwrap();
    assert!(validator.is_valid(&native[0]));
    assert_eq!(native[0]["kind"], "semantic");
    assert_eq!(native[0]["body"]["text"], original.content);
    assert_eq!(
        native[0]["body"]["structured"]["created_origin"],
        "target_migration"
    );
    assert!(
        native[0]["scope"]["owner"]
            .as_str()
            .unwrap()
            .starts_with("mem-adaptor:sha256:")
    );
    assert_ne!(native[0]["scope"]["owner"], "untrusted-owner");
    let restored = engine
        .registry
        .writer("home")
        .unwrap()
        .inspect(receipt.entries[0].target_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert!(restored.created_at.is_none());
    assert_eq!(
        record_hash(&restored).unwrap(),
        record_hash(&original).unwrap()
    );
    assert!(
        receipt.entries[0]
            .prior_write
            .as_ref()
            .unwrap()
            .target_hash
            .starts_with("sha256:")
    );
}

#[test]
fn kind_and_unicode_title_rules_are_deterministic_and_do_not_summarize() {
    let mut record = record();
    for (source, target) in [
        ("profile", "identity"),
        ("preference", "semantic"),
        ("instruction", "procedural"),
        ("project", "semantic"),
        ("tool", "semantic"),
        ("project_doc", "semantic"),
        ("working", "working"),
        ("episodic", "episodic"),
        ("unexpected", "semantic"),
    ] {
        record.source_kind = Some(source.into());
        assert_eq!(mem_adaptor_writer_ump::kind(&record), target);
    }
    record.content = format!("\n## {}\nsecond", "中".repeat(90));
    assert_eq!(mem_adaptor_core::writer::title(&record), "中".repeat(80));
    record.content.clear();
    assert_eq!(
        mem_adaptor_core::writer::title(&record),
        format!("Memory {}", record.canonical_id)
    );
}

/// Checks native envelope semantics independently and restores original canonical metadata through the Reader.
#[test]
fn okf_native_fields_index_and_log_are_honest_and_round_trip() {
    let directory = fixture();
    let original = record();
    let engine = engine(&directory, original.clone(), "okf");
    let report = plan(&engine, &directory, None);
    let receipt = apply(&engine, &report).unwrap();
    let target_id = receipt.entries[0].target_id.as_ref().unwrap();
    let text = fs::read_to_string(directory.path().join(format!("target/{target_id}.md"))).unwrap();
    let (frontmatter, body) = normalize::markdown_document(&text).unwrap();
    let frontmatter: Value = serde_saphyr::from_str(frontmatter.unwrap()).unwrap();
    assert_eq!(frontmatter["title"], "合成标题");
    assert_eq!(frontmatter["sources"][0]["resource"], "synthetic.json");
    assert!(frontmatter.get("generated").is_none());
    assert_eq!(
        frontmatter["sources"][0]["last_modified"],
        "2026-01-02T03:04:05Z"
    );
    assert!(frontmatter.get("description").is_none());
    assert!(frontmatter.get("verified").is_none());
    assert_eq!(body, original.content);
    let index = fs::read_to_string(directory.path().join("target/index.md")).unwrap();
    let (index_metadata, _) = normalize::markdown_document(&index).unwrap();
    assert_eq!(
        serde_saphyr::from_str::<Value>(index_metadata.unwrap()).unwrap(),
        json!({"okf_version": "0.2"})
    );
    assert!(index.contains("## project: synthetic-project"));
    assert!(index.contains(&format!("[合成标题]({target_id}.md)")));
    let log = fs::read_to_string(directory.path().join("target/log.md")).unwrap();
    assert!(log.contains("+1 ~0"));
    let source = SourceFs {
        root: directory.path().join("target"),
        files: [(format!("{target_id}.md"), text.into_bytes())]
            .into_iter()
            .collect(),
    };
    let output = MarkdownReader
        .read(&MarkdownReader.claim(&source.files)[0], &source)
        .unwrap();
    assert_eq!(
        record_hash(&output.records[0]).unwrap(),
        record_hash(&original).unwrap()
    );
    assert_eq!(
        output.records[0].source_extra.as_ref().unwrap()["frontmatter"]["title"],
        "Original synthetic title"
    );
}

#[test]
fn okf_vector_omission_is_reported_once_and_does_not_loop_updates() {
    let directory = fixture();
    let mut original = record();
    original.embedding = Some(Embedding {
        model: "synthetic-local".into(),
        dim: 2,
        vector: Some(vec![0.1, 0.2]),
        normalized: Some(false),
    });
    let engine = engine(&directory, original, "okf");
    let report = plan(&engine, &directory, None);
    assert!(matches!(
        report.entries[0].disposition,
        Disposition::Transformed { .. }
    ));
    assert_eq!(
        report.entries[0].reembed_plan.as_ref().unwrap().model,
        "synthetic-local"
    );
    let receipt = apply(&engine, &report).unwrap();
    let restored = engine
        .registry
        .writer("home")
        .unwrap()
        .inspect(receipt.entries[0].target_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert!(restored.embedding.as_ref().unwrap().vector.is_none());
    assert_eq!(restored.embedding.as_ref().unwrap().dim, 2);
    let previous = previous(&directory, &receipt);
    let second = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        second.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
    let index = fs::read(directory.path().join("target/index.md")).unwrap();
    let log = fs::read(directory.path().join("target/log.md")).unwrap();
    apply(&engine, &second).unwrap();
    assert_eq!(
        fs::read(directory.path().join("target/index.md")).unwrap(),
        index
    );
    assert_eq!(
        fs::read(directory.path().join("target/log.md")).unwrap(),
        log
    );
}

#[test]
fn ump_source_updates_preserve_target_creation_and_deleted_records_stay_deleted() {
    let directory = fixture();
    let original = record();
    let first_engine = engine(&directory, original.clone(), "ump");
    let first = plan(&first_engine, &directory, None);
    let receipt = apply(&first_engine, &first).unwrap();
    let created = ump(&directory)[0]["time"]["created"].clone();
    let old_receipt = previous(&directory, &receipt);
    let mut changed = original;
    changed.content.push_str("Changed synthetic text.");
    changed.content_hash = content_hash(changed.content.as_bytes());
    changed.created_at = Some("2026-02-01T00:00:00Z".into());
    let changed_engine = engine(&directory, changed, "ump");
    let update = plan(&changed_engine, &directory, Some(&old_receipt));
    let receipt = apply(&changed_engine, &update).unwrap();
    assert_eq!(ump(&directory)[0]["time"]["created"], created);
    assert_eq!(
        ump(&directory)[0]["body"]["structured"]["created_origin"],
        "target_migration"
    );
    fs::write(directory.path().join("target/records.ump.json"), "[]").unwrap();
    let next_receipt = directory.path().join("updated.json");
    write_json_new(&next_receipt, &receipt).unwrap();
    let deleted = plan(&changed_engine, &directory, Some(&next_receipt));
    assert_eq!(
        deleted.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::DeletedInTarget
        }
    );
    apply(&changed_engine, &deleted).unwrap();
    assert!(ump(&directory).is_empty());
}

/// Protects display-only edits through historical hashes and rejects native/bridge contradictions before planning.
#[test]
fn native_fields_changed_without_canonical_changes_block_history_updates() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        let engine = engine(&directory, record(), writer);
        let first = plan(&engine, &directory, None);
        let receipt = apply(&engine, &first).unwrap();
        let before_record = engine
            .registry
            .writer("home")
            .unwrap()
            .inspect(receipt.entries[0].target_id.as_ref().unwrap())
            .unwrap()
            .unwrap();
        if writer == "okf" {
            let path = directory.path().join(format!(
                "target/{}.md",
                receipt.entries[0].target_id.as_ref().unwrap()
            ));
            let text = fs::read_to_string(&path).unwrap();
            fs::write(
                path,
                text.replace("title: 合成标题", "title: User display title"),
            )
            .unwrap();
        } else {
            let mut records = ump(&directory);
            records[0]["kind"] = json!("working");
            fs::write(
                directory.path().join("target/records.ump.json"),
                serde_json::to_vec_pretty(&records).unwrap(),
            )
            .unwrap();
        }
        if writer == "ump" {
            let previous = previous(&directory, &receipt);
            let history_bytes = fs::read(&previous).unwrap();
            let before = target_snapshot(&directory.path().join("target"));
            let error = engine
                .registry
                .writer("home")
                .unwrap()
                .inspect(receipt.entries[0].target_id.as_ref().unwrap())
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("UMP native projection contradicts migration metadata")
            );
            let error = engine
                .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
                .unwrap_err();
            assert!(
                format!("{error:#}")
                    .contains("UMP native projection contradicts migration metadata")
            );
            assert_eq!(target_snapshot(&directory.path().join("target")), before);
            assert_eq!(fs::read(previous).unwrap(), history_bytes);
            continue;
        }
        let actual = engine
            .registry
            .writer("home")
            .unwrap()
            .inspect(receipt.entries[0].target_id.as_ref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            record_hash(&before_record).unwrap(),
            record_hash(&actual).unwrap()
        );
        let previous = previous(&directory, &receipt);
        let report = plan(&engine, &directory, Some(&previous));
        assert_eq!(
            report.entries[0].disposition,
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetModified
            }
        );
    }
}

/// Refuses changed shared approval inputs before invoking writes and preserves all injected target bytes.
#[test]
fn shared_artifact_changes_after_approval_fail_before_any_record_write() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        let original = record();
        let first_engine = engine(&directory, original.clone(), writer);
        let receipt = apply(&first_engine, &plan(&first_engine, &directory, None)).unwrap();
        let previous = previous(&directory, &receipt);
        let mut changed = original;
        changed.content.push_str("Source changed.");
        changed.content_hash = content_hash(changed.content.as_bytes());
        let engine = engine(&directory, changed, writer);
        let approved = plan(&engine, &directory, Some(&previous));
        let path = directory.path().join(if writer == "okf" {
            "target/index.md"
        } else {
            "target/records.ump.json"
        });
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\n ");
        fs::write(&path, bytes).unwrap();
        let before = target_snapshot(&directory.path().join("target"));
        let history = fs::read(&previous).unwrap();
        let error = apply(&engine, &approved).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("[S7]"), "{message}");
        assert!(message.contains("digest mismatch"), "{message}");
        assert_eq!(target_snapshot(&directory.path().join("target")), before);
        assert_eq!(fs::read(&previous).unwrap(), history);
    }
}

#[test]
fn unsupported_redaction_is_explicit_and_unmanaged_indices_are_never_overwritten() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        let mut original = record();
        original.consent = Some(Consent {
            memory_enabled: Some(true),
            exportable: Some(true),
            retention: None,
            redact: Some(vec!["$.private".into()]),
        });
        let engine = engine(&directory, original, writer);
        let report = plan(&engine, &directory, None);
        assert_eq!(
            report.entries[0].disposition,
            Disposition::Rejected {
                rule: "consent_redact_requires_processing".into()
            }
        );
        apply(&engine, &report).unwrap();
        assert!(!directory.path().join("target").exists());
    }
    let directory = fixture();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(
        directory.path().join("target/index.md"),
        "User index, do not overwrite.",
    )
    .unwrap();
    let engine = engine(&directory, record(), "okf");
    let report = plan(&engine, &directory, None);
    assert_eq!(
        report.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetUntracked
        }
    );
    apply(&engine, &report).unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("target/index.md")).unwrap(),
        "User index, do not overwrite."
    );
}

/// Exercises CLI export and checks the exact link-refusal boundary without touching the external payload.
#[test]
fn cli_exports_ump_and_rejects_symlinked_target_artifacts() {
    let directory = fixture();
    fs::remove_file(directory.path().join("source/synthetic.json")).unwrap();
    fs::write(
        directory.path().join("source/note.md"),
        "Synthetic CLI memory.\n",
    )
    .unwrap();
    let report = directory.path().join("plan.json");
    let target = format!("ump:{}", directory.path().join("target").display());
    let result = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args([
            "plan",
            directory.path().join("source").to_str().unwrap(),
            "--to",
            &target,
            "--report",
            report.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(["apply", report.to_str().unwrap(), "--yes"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(ump(&directory).len(), 1);
    #[cfg(unix)]
    {
        let blocked = fixture();
        let outside = blocked.path().join("outside.json");
        fs::write(&outside, "[]").unwrap();
        fs::create_dir(blocked.path().join("target")).unwrap();
        std::os::unix::fs::symlink(&outside, blocked.path().join("target/records.ump.json"))
            .unwrap();
        let engine = engine(&blocked, record(), "ump");
        let error = engine
            .plan(&blocked.path().join("source"), policy())
            .unwrap_err();
        assert!(format!("{error:#}").contains("Target artifact must not be a symlink"));
        assert!(
            fs::symlink_metadata(blocked.path().join("target/records.ump.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_dir(blocked.path().join("target")).unwrap().count(),
            1
        );
        assert_eq!(fs::read_to_string(outside).unwrap(), "[]");
    }
}

/// Independently checks the native vector extension before supplementing it with a canonical round-trip.
#[test]
fn source_created_time_and_embedding_vectors_round_trip_through_ump() {
    let directory = fixture();
    let mut original = record();
    original.created_at = Some("2026-01-01T01:02:03Z".into());
    original.observed_at = Some("2025-12-30T00:00:00Z".into());
    original.embedding = Some(Embedding {
        model: "synthetic-local".into(),
        dim: 2,
        vector: Some(vec![0.1, 0.2]),
        normalized: None,
    });
    original.canonical_id = canonical_id("synthetic-writer", &original.source_record_id);
    let engine = engine(&directory, original.clone(), "ump");
    let receipt = apply(&engine, &plan(&engine, &directory, None)).unwrap();
    let native = ump(&directory);
    assert_eq!(
        native[0]["body"]["structured"]["mem_adaptor"]["embedding"],
        json!({"model":"synthetic-local", "dim":2, "vector":[0.1,0.2]})
    );
    assert_eq!(native[0]["time"]["created"], original.created_at.unwrap());
    assert_eq!(native[0]["time"]["observed"], original.observed_at.unwrap());
    assert_eq!(
        native[0]["body"]["structured"]["created_origin"],
        "source_record"
    );
    let record = engine
        .registry
        .writer("home")
        .unwrap()
        .inspect(receipt.entries[0].target_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        record.embedding.as_ref().unwrap().vector,
        Some(vec![0.1, 0.2])
    );
}

/// Checks reviewed native bytes; only the log's migration date/time and UMP migration time are normalized.
#[test]
fn actual_native_outputs_match_reviewed_synthetic_snapshots() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m5");
    for writer in ["okf", "ump"] {
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join("source")).unwrap();
        fs::copy(
            fixtures.join("source/note.md"),
            directory.path().join("source/note.md"),
        )
        .unwrap();
        let mut registry = Registry::default();
        registry.register_reader(MarkdownReader).unwrap();
        let target = directory.path().join("target");
        if writer == "okf" {
            registry
                .register_writer("home".into(), OkfWriter::new(target.clone()).unwrap())
                .unwrap();
        } else {
            registry
                .register_writer("home".into(), UmpWriter::new(target.clone()).unwrap())
                .unwrap();
        }
        let engine = Engine { registry };
        let before = std::time::SystemTime::now();
        let receipt = apply(&engine, &plan(&engine, &directory, None)).unwrap();
        let after = std::time::SystemTime::now();
        if writer == "okf" {
            for (actual, golden) in [
                (
                    format!("{}.md", receipt.entries[0].target_id.as_ref().unwrap()),
                    "memory.okf.md",
                ),
                ("index.md".into(), "index.okf.md"),
            ] {
                assert_eq!(
                    fs::read(target.join(actual)).unwrap(),
                    fs::read(fixtures.join("snapshots").join(golden)).unwrap()
                );
            }
            let log = fs::read_to_string(target.join("log.md")).unwrap();
            let normalized = log
                .lines()
                .map(|line| {
                    if line.starts_with("## ") {
                        "## 2026-01-02".into()
                    } else if let Some(entry) = line.strip_prefix("- [") {
                        let (_, description) = entry.split_once("] ").unwrap();
                        format!("- [03:04:05Z] {description}")
                    } else {
                        line.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            assert_eq!(
                normalized,
                fs::read_to_string(fixtures.join("snapshots/log.okf.md")).unwrap()
            );
        } else {
            let mut actual = ump(&directory);
            let created = time::OffsetDateTime::parse(
                actual[0]["time"]["created"].as_str().unwrap(),
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap();
            let before = time::OffsetDateTime::from(before);
            let after = time::OffsetDateTime::from(after);
            assert!(before <= created && created <= after);
            actual[0]["time"]["created"] = json!("2026-01-02T03:04:05Z");
            let expected: Vec<Value> = serde_json::from_slice(
                &fs::read(fixtures.join("snapshots/records.ump.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, expected);
        }
    }
}

/// Preserves semantically valid native user edits as target_modified before any further migration.
#[test]
fn native_creation_and_log_edits_before_planning_are_never_overwritten() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        let original = record();
        let first_engine = engine(&directory, original.clone(), writer);
        let receipt = apply(&first_engine, &plan(&first_engine, &directory, None)).unwrap();
        let previous = previous(&directory, &receipt);
        let path = directory.path().join(if writer == "okf" {
            "target/log.md"
        } else {
            "target/records.ump.json"
        });
        if writer == "okf" {
            let text = fs::read_to_string(&path).unwrap();
            fs::write(&path, format!("{text}\n- User note.\n")).unwrap();
        } else {
            let mut native = ump(&directory);
            native[0]["time"]["created"] = json!("2026-03-01T00:00:00Z");
            fs::write(&path, serde_json::to_vec_pretty(&native).unwrap()).unwrap();
        }
        let before = fs::read(&path).unwrap();
        let mut changed = original;
        changed.content.push_str("Source update.");
        changed.content_hash = content_hash(changed.content.as_bytes());
        let engine = engine(&directory, changed, writer);
        let report = plan(&engine, &directory, Some(&previous));
        assert_eq!(
            report.entries[0].disposition,
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetModified
            }
        );
        apply(&engine, &report).unwrap();
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}

/// Rejects edited managed native fields without modifying source input or creating target artifacts.
#[test]
fn generated_okf_metadata_changes_are_not_silently_hidden_on_import() {
    let directory = fixture();
    let mut original = record();
    original.provenance.actor_kind = ActorKind::User;
    original.provenance.actor = "Synthetic author".into();
    let engine = engine(&directory, original, "okf");
    let receipt = apply(&engine, &plan(&engine, &directory, None)).unwrap();
    let path = format!("{}.md", receipt.entries[0].target_id.as_ref().unwrap());
    let text = fs::read_to_string(directory.path().join("target").join(&path)).unwrap();
    for edited in [
        text.replace(
            "resource: synthetic.json",
            "resource: different-source.json",
        ),
        text.replacen("synthetic\n", "different-tag\n", 1),
        text.replacen(
            "at: \"2026-01-02T03:04:05Z\"",
            "at: \"2026-03-01T00:00:00Z\"",
            1,
        ),
    ] {
        assert_ne!(edited, text);
        let source = SourceFs {
            root: "/synthetic".into(),
            files: [(path.clone(), edited.into_bytes())].into_iter().collect(),
        };
        let before = target_snapshot(&directory.path().join("target"));
        let input = source.files.clone();
        let error = match MarkdownReader.read(&MarkdownReader.claim(&source.files)[0], &source) {
            Err(error) => error,
            Ok(_) => panic!("Edited native projection was imported"),
        };
        assert!(
            format!("{error:#}").contains("OKF native projection differs from canonical metadata")
        );
        assert_eq!(source.files, input);
        assert_eq!(target_snapshot(&directory.path().join("target")), before);
        assert!(!directory.path().join("plan.approval.json").exists());
        assert!(!directory.path().join("plan.receipt.json").exists());
    }
}

#[test]
fn unrelated_target_records_survive_and_unrelated_markdown_is_not_indexed() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        fs::create_dir_all(directory.path().join("target/memories")).unwrap();
        let unrelated = json!({
            "ump":"1.0", "id":"urn:ump:foreignrecord", "kind":"semantic",
            "body":{"text":"Synthetic unrelated memory."},
            "scope":{"owner":"foreign-owner","visibility":"private"},
            "time":{"created":"2026-01-01T00:00:00Z"}
        });
        if writer == "okf" {
            fs::write(
                directory.path().join("target/memories/user-note.md"),
                "User-owned note.\n",
            )
            .unwrap();
        } else {
            fs::write(
                directory.path().join("target/records.ump.json"),
                serde_json::to_vec(&vec![unrelated.clone()]).unwrap(),
            )
            .unwrap();
        }
        let engine = engine(&directory, record(), writer);
        apply(&engine, &plan(&engine, &directory, None)).unwrap();
        if writer == "okf" {
            assert_eq!(
                fs::read_to_string(directory.path().join("target/memories/user-note.md")).unwrap(),
                "User-owned note.\n"
            );
            assert!(
                !fs::read_to_string(directory.path().join("target/index.md"))
                    .unwrap()
                    .contains("user-note")
            );
        } else {
            assert!(ump(&directory).contains(&unrelated));
            assert_eq!(ump(&directory).len(), 2);
        }
    }
}

/// Injects changes around actual approved writes and verifies byte-proof refusals with the observed target state.
/// Checks each injected byte change remains intact, all file paths/user bytes and prior evidence survive, and no receipt succeeds.
#[test]
fn writer_output_proofs_reject_changes_before_write_after_write_and_after_read_back() {
    type Snapshot = std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>;
    struct RacingWriter(
        Box<dyn Writer>,
        u8,
        std::rc::Rc<std::cell::RefCell<Option<Snapshot>>>,
    );
    impl RacingWriter {
        /// Records the exact injected state so later checks cannot hide extra engine-side replacements.
        fn change_shared(&self) -> mem_adaptor_core::Result<()> {
            let path = self.location().join(self.shared_artifact_paths()[0]);
            let mut bytes = fs::read(&path)?;
            bytes.extend_from_slice(b"\n ");
            fs::write(path, bytes)?;
            *self.2.borrow_mut() = Some(target_snapshot(self.location()));
            Ok(())
        }
    }
    impl Writer for RacingWriter {
        fn id(&self) -> &'static str {
            self.0.id()
        }
        fn version(&self) -> &'static str {
            self.0.version()
        }
        fn location(&self) -> &Path {
            self.0.location()
        }
        fn capabilities(&self) -> Capabilities {
            self.0.capabilities()
        }
        /// Preserves the wrapped Writer's target inspection errors during fault injection.
        fn plan(
            &self,
            record: &CanonicalRecord,
            previous: Option<&ReceiptEntry>,
        ) -> mem_adaptor_core::Result<Planned> {
            self.0.plan(record, previous)
        }
        fn write(
            &self,
            batch: &[Planned],
            token: &mem_adaptor_core::engine::WriteToken,
        ) -> mem_adaptor_core::Result<WriteResult> {
            if self.1 == 0 {
                self.change_shared()?;
            }
            let result = self.0.write(batch, token)?;
            if self.1 == 1 {
                self.change_shared()?;
            }
            Ok(result)
        }
        fn read_back(&self, written: &[Written]) -> mem_adaptor_core::Result<Vec<ReadBack>> {
            let result = self.0.read_back(written)?;
            if self.1 == 2 {
                self.change_shared()?;
            }
            Ok(result)
        }
        fn inspect(&self, id: &str) -> mem_adaptor_core::Result<Option<CanonicalRecord>> {
            self.0.inspect(id)
        }
        fn target_hash(&self, id: &str) -> mem_adaptor_core::Result<Option<String>> {
            self.0.target_hash(id)
        }
        fn artifacts(&self, ids: &[String]) -> mem_adaptor_core::Result<Vec<TargetArtifact>> {
            self.0.artifacts(ids)
        }
        fn shared_artifact_paths(&self) -> &'static [&'static str] {
            self.0.shared_artifact_paths()
        }
    }
    for writer in ["okf", "ump"] {
        for stage in 0..3 {
            let directory = fixture();
            let original = record();
            let initial = engine(&directory, original.clone(), writer);
            let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
            let history = previous(&directory, &receipt);
            fs::write(
                directory.path().join("target/user.txt"),
                b"Protected user bytes",
            )
            .unwrap();
            let before_files = target_snapshot(&directory.path().join("target"));
            let history_bytes = fs::read(&history).unwrap();
            let mut changed = original.clone();
            changed.content.push_str("New source text.");
            changed.content_hash = content_hash(changed.content.as_bytes());
            let mut registry = Registry::default();
            registry.register_reader(SyntheticReader(changed)).unwrap();
            let target = directory.path().join("target");
            let inner: Box<dyn Writer> = if writer == "okf" {
                Box::new(OkfWriter::new(target).unwrap())
            } else {
                Box::new(UmpWriter::new(target).unwrap())
            };
            let injected = std::rc::Rc::new(std::cell::RefCell::new(None));
            registry
                .register_writer("home".into(), RacingWriter(inner, stage, injected.clone()))
                .unwrap();
            let engine = Engine { registry };
            let approved = plan(&engine, &directory, Some(&history));
            let error = match apply(&engine, &approved) {
                Err(error) => error,
                Ok(_) => panic!("Raced target was overwritten"),
            };
            assert!(format!("{error:#}").contains(if stage == 0 {
                "WriteToken target artifact differs from approval"
            } else {
                "Writer output changed before"
            }));
            let after_files = target_snapshot(&directory.path().join("target"));
            assert_eq!(after_files, *injected.borrow().as_ref().unwrap());
            assert_eq!(
                after_files.keys().collect::<Vec<_>>(),
                before_files.keys().collect::<Vec<_>>()
            );
            assert_eq!(
                fs::read(directory.path().join("target/user.txt")).unwrap(),
                b"Protected user bytes"
            );
            assert_eq!(fs::read(&history).unwrap(), history_bytes);
            if stage == 0 {
                for (path, bytes) in &before_files {
                    let expected = if path.to_str().unwrap()
                        == initial
                            .registry
                            .writer("home")
                            .unwrap()
                            .shared_artifact_paths()[0]
                    {
                        [bytes.as_slice(), b"\n "].concat()
                    } else {
                        bytes.clone()
                    };
                    assert_eq!(after_files[path], expected);
                }
            }
            let actual = initial
                .registry
                .writer("home")
                .unwrap()
                .inspect(receipt.entries[0].target_id.as_ref().unwrap())
                .unwrap()
                .unwrap();
            if stage == 0 {
                assert_eq!(
                    record_hash(&actual).unwrap(),
                    record_hash(&original).unwrap()
                );
            } else {
                assert!(actual.content.ends_with("New source text."));
            }
        }
    }
}

#[test]
fn skipped_receipts_cannot_launder_modified_shared_artifacts() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        let original = record();
        let initial = engine(&directory, original.clone(), writer);
        let first = apply(&initial, &plan(&initial, &directory, None)).unwrap();
        let history = previous(&directory, &first);
        let path = directory.path().join(if writer == "okf" {
            "target/index.md"
        } else {
            "target/records.ump.json"
        });
        let mut bytes = fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\n ");
        fs::write(&path, &bytes).unwrap();
        let blocked = plan(&initial, &directory, Some(&history));
        assert_eq!(
            blocked.entries[0].disposition,
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetModified
            }
        );
        let skipped = apply(&initial, &blocked).unwrap();
        for path in initial
            .registry
            .writer("home")
            .unwrap()
            .shared_artifact_paths()
        {
            assert_eq!(
                skipped.targets[0]
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.path == *path),
                first.targets[0]
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.path == *path)
            );
        }
        let skipped_path = directory.path().join("skipped.json");
        write_json_new(&skipped_path, &skipped).unwrap();
        let mut changed = original;
        changed.content.push_str("New source content.");
        changed.content_hash = content_hash(changed.content.as_bytes());
        let engine = engine(&directory, changed, writer);
        let later = plan(&engine, &directory, Some(&skipped_path));
        assert_eq!(
            later.entries[0].disposition,
            Disposition::Unresolved {
                reason: UnresolvedReason::TargetModified
            }
        );
        apply(&engine, &later).unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn okf_preserves_an_existing_reembedding_plan_instead_of_replacing_it() {
    let directory = fixture();
    let mut original = record();
    original.embedding = Some(Embedding {
        model: "source-model".into(),
        dim: 2,
        vector: Some(vec![0.1, 0.2]),
        normalized: None,
    });
    original.reembed_plan = Some(ReembedPlan {
        canonical_ids: vec![original.canonical_id.clone()],
        model: "approved-new-model".into(),
        dim: 4,
        quality_impact: "Existing explicit plan.".into(),
    });
    let expected = original.reembed_plan.clone();
    let engine = engine(&directory, original, "okf");
    let report = plan(&engine, &directory, None);
    assert_eq!(report.entries[0].reembed_plan, expected);
    let receipt = apply(&engine, &report).unwrap();
    let actual = engine
        .registry
        .writer("home")
        .unwrap()
        .inspect(receipt.entries[0].target_id.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(actual.reembed_plan, expected);
    assert!(actual.embedding.as_ref().unwrap().vector.is_none());
    let history = previous(&directory, &receipt);
    assert_eq!(
        plan(&engine, &directory, Some(&history)).entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
}
