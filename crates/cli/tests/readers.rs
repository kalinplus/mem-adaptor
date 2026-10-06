//! Tests source normalization, inventory, metadata integrity, and CLI/home round trips using synthetic inputs.
//! Reader outputs are checked before engine planning; approved writes use isolated temporary OKF targets.
//! Sections group test purposes for reading, not execution order; no real exports, model calls, or independent conformance.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::{Engine, canonical_id, record_hash, timestamp};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::*;
use mem_adaptor_reader_chatgpt::ChatgptReader;
use mem_adaptor_reader_claude::ClaudeReader;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use serde_json::{Value, json};
use tempfile::TempDir;

// Test support: synthetic inventories, isolated filesystems, and explicit in-process approval.

/// Converts inline synthetic text into an in-memory inventory without reading a user's files.
fn files(entries: &[(&str, &str)]) -> FileInventory {
    entries
        .iter()
        .map(|(path, text)| ((*path).into(), text.as_bytes().to_vec()))
        .collect()
}

/// Runs the supplied Reader's claims against in-memory files; parsing failures are test failures.
fn read(reader: &dyn Reader, files: FileInventory) -> Vec<ReaderOutput> {
    let source = SourceFs {
        root: "/synthetic/source".into(),
        files,
    };
    reader
        .claim(&source.files)
        .iter()
        .map(|claim| reader.read(claim, &source).unwrap())
        .collect()
}

/// Loads the committed synthetic source fixture set; missing or malformed fixture paths fail test setup.
fn fixture_files(name: &str) -> FileInventory {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/m4")
        .join(name);
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_str().unwrap().into(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

/// Copies one fixture set into a temporary source directory, leaving the target absent until apply.
fn fixture(name: &str) -> TempDir {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    for (path, bytes) in fixture_files(name) {
        fs::write(directory.path().join("source").join(path), bytes).unwrap();
    }
    directory
}

/// Registers the three source Readers with a single OKF Writer at the test's isolated target path.
fn engine(target: &Path) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(MarkdownReader).unwrap();
    registry.register_reader(ChatgptReader).unwrap();
    registry.register_reader(ClaudeReader).unwrap();
    registry
        .register_writer("home".into(), OkfWriter::new(target.into()))
        .unwrap();
    Engine { registry }
}

/// Selects the tested secret disposition without claiming user choice or implementing the reserved PII policy.
fn policy(action: GateAction) -> GatePolicy {
    GatePolicy {
        secrets: action,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}

/// Applies an existing plan with matching synthetic approval, writing only the temporary target.
/// Returns an in-memory receipt; plan/approval references are labels, not persisted report files.
fn apply(engine: &Engine, plan: &PlanReport) -> ReceiptReport {
    engine
        .apply(
            plan,
            &ApprovalReceipt {
                schema_version: "0.1.0".into(),
                receipt_id: "synthetic-approval".into(),
                plan_digest: plan.plan_digest.clone(),
                approved_at: timestamp().unwrap(),
                backend: "local".into(),
                approver: "synthetic-user".into(),
            },
            "synthetic-plan.json".into(),
            "synthetic-approval.json".into(),
        )
        .unwrap()
}

// Source normalization: preserve body and optional semantics under each Reader's explicit rules.

/// Checks exact Markdown body/newline preservation, protected kind, tags, typed unknown metadata, and index counts.
#[test]
fn markdown_preserves_body_and_typed_unknown_metadata() {
    let output = read(&MarkdownReader, fixture_files("markdown"));
    let record = output
        .iter()
        .flat_map(|output| &output.records)
        .next()
        .unwrap();
    assert_eq!(record.content, "# 合成记忆\n\nKeep this body unchanged.\n");
    assert_eq!(record.source_record_id, "note.md");
    assert_eq!(record.dna_class, DnaClass::Dna);
    assert_eq!(record.tags.as_ref().unwrap(), &["synthetic", "中文"]);
    let extra = record.source_extra.as_ref().unwrap();
    assert_eq!(
        extra["frontmatter"]["custom"]["values"],
        json!([2, null, "example"])
    );
    assert!(!extra.contains_key("body"));
    let index = output
        .iter()
        .find(|output| output.records.is_empty())
        .unwrap();
    assert_eq!(index.registered_count, 2);
    let mapped = output
        .iter()
        .find(|output| !output.records.is_empty())
        .unwrap();
    assert!(
        mapped.source_records[0]
            .field_map
            .iter()
            .any(|mapping| mapping.canonical_path == "/source_extra/frontmatter/custom/values")
    );
    let crlf = read(
        &MarkdownReader,
        files(&[(
            "body.md",
            "---\r\ntype: tool\r\n---\r\n保留\r\nno trailing newline",
        )]),
    );
    assert_eq!(crlf[0].records[0].content, "保留\r\nno trailing newline");
}

/// Checks project/session provenance and source modification time without inventing observation or validity times.
#[test]
fn claude_code_retains_session_provenance_without_inventing_fact_time() {
    let output = read(
        &MarkdownReader,
        files(&[
            (
                "project-a/feedback.md",
                "---\nname: Synthetic feedback\nmetadata:\n  node_type: memory\n  type: feedback\n  originSessionId: synthetic-session\n  modified: 2026-01-02T03:04:05.507Z\n  custom: [a, 3]\n---\nSame synthetic body.\n",
            ),
            (
                "project-b/feedback.md",
                "---\nmetadata:\n  node_type: memory\n  type: feedback\n---\nSame synthetic body.\n",
            ),
        ]),
    );
    let record = &output[0].records[0];
    assert_eq!(record.source.system, "claude_code");
    assert_eq!(record.scope, Scope::Project);
    assert_ne!(record.scope_qualifier, output[1].records[0].scope_qualifier);
    assert_eq!(record.source_kind.as_deref(), Some("feedback"));
    assert_eq!(record.dna_class, DnaClass::Standard);
    assert_eq!(record.evidence_level, EvidenceLevel::Inferred);
    assert_eq!(
        record.updated_at.as_deref(),
        Some("2026-01-02T03:04:05.507Z")
    );
    assert!(record.observed_at.is_none());
    assert!(record.valid_from.is_none());
    assert_eq!(
        record.provenance.evidence.as_ref().unwrap()[0].source_ref,
        "session:synthetic-session"
    );
    assert_eq!(
        record.source_extra.as_ref().unwrap()["frontmatter"]["metadata"]["custom"],
        json!(["a", 3])
    );
}

/// Checks original saved-memory IDs, source deletion counts, disabled-memory consent, and alternate JSON wrappers.
#[test]
fn chatgpt_saved_memories_keep_ids_and_disabled_consent_and_count_deletions() {
    let output = read(&ChatgptReader, fixture_files("chatgpt"));
    let saved = output
        .iter()
        .find(|output| output.deleted_count > 0)
        .unwrap();
    assert_eq!(saved.deleted_count, 1);
    assert_eq!(saved.records.len(), 2);
    let preference = saved
        .records
        .iter()
        .find(|record| record.source_record_id == "synthetic-preference")
        .unwrap();
    assert_eq!(
        preference.canonical_id,
        canonical_id("chatgpt", "synthetic-preference")
    );
    assert_eq!(preference.dna_class, DnaClass::Dna);
    assert_eq!(
        preference.created_at.as_deref(),
        Some("2026-01-02T03:04:05Z")
    );
    assert_eq!(
        preference.source_extra.as_ref().unwrap()["extra"]["nested"],
        json!([true, 2, null])
    );
    let disabled = saved
        .records
        .iter()
        .find(|record| record.source_record_id == "synthetic-disabled")
        .unwrap();
    assert_eq!(
        disabled.consent.as_ref().unwrap().memory_enabled,
        Some(false)
    );
    assert_eq!(disabled.source_locator, "memory.json#/memory/1");
    assert_eq!(saved.anomalies[0].code, "missing_memory_content");
    let array = read(
        &ChatgptReader,
        files(&[(
            "saved_memories.json",
            r#"[{"id":"original-id","text":"Synthetic text.","type":"tool"}]"#,
        )]),
    );
    assert_eq!(array[0].records[0].source_record_id, "original-id");
    assert_eq!(array[0].records[0].source_locator, "saved_memories.json#/0");
    assert_eq!(
        output
            .iter()
            .filter(|output| output.registered_count > 0)
            .count(),
        2
    );
}

/// Checks retained date text, line diagnostics, unknown-kind reporting, stable normalized IDs, and duplicate-line omission.
/// Date-only values must not become invented creation/observation timestamps.
#[test]
fn prompt_dates_are_preserved_but_never_promoted_to_rfc3339_fact_times() {
    let output = read(&ChatgptReader, fixture_files("chatgpt"));
    let prompt = output
        .iter()
        .find(|output| output.records.len() == 4)
        .unwrap();
    assert_eq!(
        prompt
            .anomalies
            .iter()
            .map(|anomaly| anomaly.code.as_str())
            .collect::<Vec<_>>(),
        [
            "invalid_prompt_date",
            "unknown_source_kind",
            "invalid_prompt_line"
        ]
    );
    assert_eq!(prompt.anomalies[2].line, Some(6));
    assert_eq!(
        prompt.records[0].source_extra.as_ref().unwrap()["date"],
        "2026-02-28"
    );
    assert!(
        prompt
            .records
            .iter()
            .all(|record| record.created_at.is_none() && record.observed_at.is_none())
    );
    assert_eq!(prompt.records[3].source_kind.as_deref(), Some("unexpected"));
    assert_eq!(prompt.records[3].evidence_level, EvidenceLevel::Inferred);
    assert!(
        prompt.source_records[3]
            .unmapped
            .iter()
            .any(|field| field.source_path == "/kind")
    );
    assert!(
        prompt
            .source_unavailable
            .iter()
            .any(|layer| layer.layer == "memory_summary")
    );
    let first = read(
        &ChatgptReader,
        files(&[(
            "extract.chatgpt.md",
            "[unknown] [tool] Synthetic stable text.\n",
        )]),
    );
    let reordered = read(
        &ChatgptReader,
        files(&[(
            "extract.chatgpt.md",
            "```text\n\n[unknown] [tool] Synthetic  stable text.\n```",
        )]),
    );
    assert_eq!(
        first[0].records[0].canonical_id,
        reordered[0].records[0].canonical_id
    );
    let duplicate = read(
        &ChatgptReader,
        files(&[(
            "extract.chatgpt.md",
            "[unknown] [tool] Same text.\n[unknown] [tool] Same  text.\n",
        )]),
    );
    assert_eq!(duplicate[0].records.len(), 1);
    assert_eq!(duplicate[0].anomalies[0].code, "duplicate_prompt_line");
}

/// Checks new memory_files precedence, including an empty array, while retaining complete file bodies and metadata.
/// A profile-shaped filename alone must not imply a protected kind or measured classification.
#[test]
fn claude_new_memory_files_win_even_when_empty_and_preserve_full_file_bytes() {
    let output = read(&ClaudeReader, fixture_files("claude"));
    let memories = output
        .iter()
        .find(|output| {
            output
                .records
                .iter()
                .any(|record| record.source_record_id == "synthetic-account:/profile.md")
        })
        .unwrap();
    assert_eq!(memories.records.len(), 2);
    let profile = &memories.records[0];
    assert_eq!(
        profile.content,
        "---\ntype: profile\nname: Synthetic identity\n---\nKeep the entire synthetic file.\r\n"
    );
    assert_eq!(profile.dna_class, DnaClass::Dna);
    assert_eq!(
        profile.source_extra.as_ref().unwrap()["account"]["account_extra"]["locale"],
        "synthetic"
    );
    assert_eq!(
        profile.source_extra.as_ref().unwrap()["file"]["file_extra"]["enabled"],
        true
    );
    assert!(
        !profile.source_extra.as_ref().unwrap()["file"]
            .as_object()
            .unwrap()
            .contains_key("content")
    );
    assert!(
        memories
            .anomalies
            .iter()
            .any(|anomaly| anomaly.code == "legacy_memory_fields_not_reimported")
    );
    let empty = read(
        &ClaudeReader,
        files(&[(
            "memories.json",
            r#"[{"account_uuid":"synthetic","memory_files":[],"conversations_memory":"Never import this legacy body."}]"#,
        )]),
    );
    assert!(empty[0].records.is_empty());
    assert_eq!(
        empty[0].anomalies[0].code,
        "legacy_memory_fields_not_reimported"
    );
    let unclassified = read(
        &ClaudeReader,
        files(&[(
            "memories.json",
            r#"[{"account_uuid":"synthetic","memory_files":[{"path":"/profile.md","content":"Do not classify by filename."}]}]"#,
        )]),
    );
    assert_eq!(unclassified[0].records[0].dna_class, DnaClass::Standard);
    assert_eq!(
        unclassified[0].records[0].evidence_level,
        EvidenceLevel::Inferred
    );
}

/// Checks legacy account/project blocks and project documents remain whole, scoped, and accompanied by source metadata.
#[test]
fn claude_legacy_blocks_and_project_documents_do_not_get_summarized_or_split() {
    let legacy = read(
        &ClaudeReader,
        files(&[(
            "memories.json",
            r#"[{"account_uuid":"synthetic","conversations_memory":"Whole account block.\nSecond line.","project_memories":{"project-a":"Whole project block.\nSecond line."},"custom":{"flag":true}}]"#,
        )]),
    );
    assert_eq!(legacy[0].records.len(), 2);
    assert_eq!(
        legacy[0].records[0].content,
        "Whole account block.\nSecond line."
    );
    assert_eq!(
        legacy[0].records[1].content,
        "Whole project block.\nSecond line."
    );
    assert_eq!(
        legacy[0].records[1].scope_qualifier.as_deref(),
        Some("project-a")
    );
    assert_eq!(
        legacy[0].records[0].source_extra.as_ref().unwrap()["custom"]["flag"],
        true
    );
    let projects = read(&ClaudeReader, fixture_files("claude"));
    let output = projects
        .iter()
        .find(|output| output.records.len() == 3)
        .unwrap();
    assert_eq!(output.records[0].source_record_id, "synthetic-document");
    assert_eq!(
        output.records[0].source_extra.as_ref().unwrap()["project"]["project_extra"]["color"],
        "blue"
    );
    assert_eq!(output.records[1].content, "");
    assert_eq!(output.records[2].dna_class, DnaClass::Dna);
    assert_eq!(
        output.records[2].content,
        "Follow these synthetic project instructions.\n"
    );
    assert!(
        output
            .records
            .iter()
            .all(|record| record.scope == Scope::Project)
    );
}

// Inventory: recognition and registration are not conversion into memory records.

/// Checks disjoint Reader claims for overlapping JSON filenames and refuses to guess the origin of isolated empty transcripts.
#[test]
fn shared_json_names_and_empty_transcripts_are_claimed_unambiguously() {
    let mut inventory = FileInventory::new();
    for (prefix, name) in [("chatgpt", "chatgpt"), ("claude", "claude")] {
        for (path, bytes) in fixture_files(name) {
            inventory.insert(format!("{prefix}/{path}"), bytes);
        }
        inventory.insert(format!("{prefix}/conversations.json"), b"[]".to_vec());
    }
    let mut claims = BTreeMap::new();
    for reader in [
        &ChatgptReader as &dyn Reader,
        &ClaudeReader,
        &MarkdownReader,
    ] {
        for claim in reader.claim(&inventory) {
            assert!(claims.insert(claim.path, reader.id()).is_none());
        }
    }
    assert_eq!(claims["chatgpt/conversations.json"], "chatgpt");
    assert_eq!(claims["claude/conversations.json"], "claude");
    let saved = files(&[(
        "memories.json",
        r#"[{"id":"original-id","content":"Synthetic saved memory."}]"#,
    )]);
    assert_eq!(ChatgptReader.claim(&saved).len(), 1);
    assert!(ClaudeReader.claim(&saved).is_empty());
    let unknown = files(&[("conversations.json", "[]")]);
    assert!(ChatgptReader.claim(&unknown).is_empty());
    assert!(ClaudeReader.claim(&unknown).is_empty());
}

/// Checks recognized OKF indices/logs are registration-only, runtime artifacts excluded, and ordinary notes not misclassified.
#[test]
fn okf_indices_and_runtime_logs_are_registered_without_becoming_memories() {
    let inventory = files(&[
        (
            "index.md",
            "---\ntype: Index\nokf_version: '0.2'\n---\n- [Synthetic](memories/note.md)\n",
        ),
        ("log.md", "# Synthetic log\n"),
        (".mem-adaptor/plan.md", "Never parse a runtime report."),
        ("memories/note.md", "Synthetic memory."),
    ]);
    let claims = MarkdownReader.claim(&inventory);
    assert_eq!(claims.len(), 3);
    assert_eq!(
        claims.iter().filter(|claim| claim.registered_only).count(),
        2
    );
    let false_index = files(&[
        (
            "index.md",
            "This ordinary note mentions okf_version: in its body.",
        ),
        ("log.md", "This is also an ordinary note."),
    ]);
    assert!(
        MarkdownReader
            .claim(&false_index)
            .iter()
            .all(|claim| !claim.registered_only)
    );
}

/// Checks empty and registration-only sources report no records while retaining warnings, counts, and unavailable-layer evidence.
#[test]
fn empty_sources_and_registered_only_exports_report_no_memory_records() {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    let engine = engine(&directory.path().join("target"));
    let report = engine
        .plan(&directory.path().join("source"), policy(GateAction::Pass))
        .unwrap();
    assert_eq!(report.source_inventory.state, InventoryState::Empty);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.starts_with("No memory records"))
    );
    fs::write(directory.path().join("source/user.json"), "{}").unwrap();
    fs::write(directory.path().join("source/conversations.json"), "[]").unwrap();
    let report = engine
        .plan(&directory.path().join("source"), policy(GateAction::Pass))
        .unwrap();
    assert_eq!(report.source.system, "chatgpt");
    assert_eq!(report.source_inventory.state, InventoryState::Empty);
    assert_eq!(report.source_inventory.files[0].registered_count, Some(0));
    assert_eq!(report.source_unavailable.len(), 3);
    assert!(report.entries.is_empty());
}

// Home preservation: recovering metadata must not invent identities or resolve source/envelope conflicts.

/// Checks approved fixture writes recover complete record hashes and original identities when the home becomes a source.
/// Planning leaves the initial target absent; round-trip assertions cover these fixtures, not other OKF consumers.
#[test]
fn home_round_trip_restores_original_identity_scope_and_unknown_metadata() {
    for name in ["markdown", "chatgpt", "claude"] {
        let directory = fixture(name);
        let target = directory.path().join("target");
        let migration = engine(&target);
        let plan = migration
            .plan(&directory.path().join("source"), policy(GateAction::Pass))
            .unwrap();
        assert!(!target.exists());
        let receipt = apply(&migration, &plan);
        assert!(
            receipt
                .entries
                .iter()
                .filter(|entry| entry.verification.is_some())
                .all(|entry| entry.verification == Some(Verification::Verified))
        );
        let inventory: FileInventory = fs::read_dir(target.join("memories"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    format!("memories/{}", entry.file_name().to_str().unwrap()),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect();
        let recovered = read(&MarkdownReader, inventory);
        let original: BTreeMap<_, _> = plan
            .digest_inputs
            .records
            .iter()
            .map(|record| (&record.canonical_id, &record.record_hash))
            .collect();
        for record in recovered.iter().flat_map(|output| &output.records) {
            assert_eq!(
                &record_hash(record).unwrap(),
                original[&record.canonical_id]
            );
            assert!(!record.source_record_id.starts_with("memories/"));
        }
        let second = engine(&directory.path().join("another-home"))
            .plan(&target, policy(GateAction::Pass))
            .unwrap();
        assert_eq!(
            second.entries.len(),
            receipt
                .entries
                .iter()
                .filter(|entry| entry.verification.is_some())
                .count()
        );
        assert!(
            second
                .entries
                .iter()
                .all(|entry| entry.disposition == Disposition::Accepted)
        );
    }
}

/// Checks unequal original/envelope metadata fails rather than overwriting a source value.
/// Nonconflicting new envelope members must survive beside original metadata.
#[test]
fn conflicting_original_and_house_metadata_are_not_silently_merged() {
    let directory = fixture("markdown");
    let target = directory.path().join("target");
    let engine = engine(&target);
    let plan = engine
        .plan(&directory.path().join("source"), policy(GateAction::Pass))
        .unwrap();
    let receipt = apply(&engine, &plan);
    let path = target.join(format!(
        "{}.md",
        receipt.entries[0].target_id.as_ref().unwrap()
    ));
    let text = fs::read_to_string(path).unwrap();
    let edited = text.replacen(
        "---\n",
        "---\nname: Conflicting synthetic envelope name\n",
        1,
    );
    let source = SourceFs {
        root: "/synthetic/home".into(),
        files: files(&[("memories/note.md", &edited)]),
    };
    let claim = MarkdownReader.claim(&source.files).remove(0);
    let error = match MarkdownReader.read(&claim, &source) {
        Ok(_) => panic!("Metadata conflict was silently resolved"),
        Err(error) => error,
    };
    assert!(
        error
            .to_string()
            .contains("Conflicting original and envelope source metadata")
    );
    let extra = text.replacen("---\n", "---\ncustom_envelope:\n  enabled: true\n", 1);
    let recovered = read(&MarkdownReader, files(&[("memories/note.md", &extra)]));
    let fields = &recovered[0].records[0].source_extra.as_ref().unwrap()["frontmatter"];
    assert_eq!(fields["name"], "Synthetic preference");
    assert_eq!(fields["custom_envelope"]["enabled"], true);
}

// Integrity and privacy: source metadata participates in approval binding and diagnostics must not echo values.

/// Checks unchanged body hashes cannot hide metadata changes from approval binding.
/// Secret-bearing metadata is rejected by block policy, leaves no target, and is absent from plan/receipt JSON.
#[test]
fn metadata_only_changes_are_hashed_and_secret_metadata_cannot_bypass_the_gate() {
    let directory = fixture("markdown");
    let target = directory.path().join("target");
    let engine = engine(&target);
    let first = engine
        .plan(&directory.path().join("source"), policy(GateAction::Pass))
        .unwrap();
    let path = directory.path().join("source/note.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("enabled: true", "enabled: false")).unwrap();
    let second = engine
        .plan(&directory.path().join("source"), policy(GateAction::Pass))
        .unwrap();
    assert_eq!(
        first.digest_inputs.records[0].content_hash,
        second.digest_inputs.records[0].content_hash
    );
    assert_ne!(
        first.digest_inputs.records[0].record_hash,
        second.digest_inputs.records[0].record_hash
    );
    let approval = ApprovalReceipt {
        schema_version: "0.1.0".into(),
        receipt_id: "synthetic".into(),
        plan_digest: first.plan_digest.clone(),
        approved_at: timestamp().unwrap(),
        backend: "local".into(),
        approver: "synthetic".into(),
    };
    assert!(
        engine
            .apply(&first, &approval, "plan".into(), "approval".into())
            .is_err()
    );
    let secret = format!("ghp_TEST{}", "A".repeat(32));
    fs::write(&path, format!("---\ntype: preference\ncustom:\n  credential: {secret}\n---\nSynthetic non-sensitive body.\n")).unwrap();
    let blocked = engine
        .plan(&directory.path().join("source"), policy(GateAction::Block))
        .unwrap();
    assert!(matches!(
        blocked.entries[0].disposition,
        Disposition::Rejected { .. }
    ));
    assert!(
        blocked.entries[0]
            .sensitive_findings
            .iter()
            .any(|finding| finding.field_path == "/source_extra/frontmatter/custom/credential")
    );
    let receipt = apply(&engine, &blocked);
    assert!(!target.exists());
    assert!(!serde_json::to_string(&blocked).unwrap().contains(&secret));
    assert!(!serde_json::to_string(&receipt).unwrap().contains(&secret));
}

/// Checks malformed export boundaries fail without echoing source values; conflicting content aliases produce a diagnostic.
/// Equal aliases must not create redundant preserved metadata.
#[test]
fn malformed_export_boundaries_fail_without_echoing_sensitive_values() {
    for reader in [
        &ChatgptReader as &dyn Reader,
        &ClaudeReader,
        &MarkdownReader,
    ] {
        let (name, text) = match reader.id() {
            "chatgpt" => (
                "memory.json",
                r#"{"memory":"password=synthetic-private-value"}"#,
            ),
            "claude" => (
                "projects.json",
                r#"[{"uuid":"synthetic","docs":"password=synthetic-private-value"}]"#,
            ),
            _ => (
                "note.md",
                "---\ninvalid: [password=synthetic-private-value\n---\nSynthetic body.\n",
            ),
        };
        let source = SourceFs {
            root: "/synthetic".into(),
            files: files(&[(name, text)]),
        };
        let claims = reader.claim(&source.files);
        assert_eq!(claims.len(), 1);
        let error = match reader.read(&claims[0], &source) {
            Ok(_) => panic!("Malformed source was accepted"),
            Err(error) => error,
        };
        assert!(!format!("{error:#}").contains("synthetic-private-value"));
    }
    let conflict = read(
        &ChatgptReader,
        files(&[(
            "memory.json",
            r#"[{"id":"synthetic","content":"First body.","text":"Different body."}]"#,
        )]),
    );
    assert!(conflict[0].records.is_empty());
    assert_eq!(conflict[0].anomalies[0].code, "conflicting_memory_content");
    let alias = read(
        &ChatgptReader,
        files(&[(
            "memory.json",
            r#"[{"id":"synthetic","content":"Same body.","text":"Same body."}]"#,
        )]),
    );
    assert!(alias[0].records[0].source_extra.is_none());
}

// CLI and archive integration: report contracts and explicit approval over the same synthetic source fixtures.

/// Checks CLI registration for all three Readers, schema-valid plans, metadata-value exclusion, and no target/model calls.
#[test]
fn all_readers_are_available_from_cli_and_reports_have_no_raw_metadata_values() {
    for name in ["markdown", "chatgpt", "claude"] {
        let directory = fixture(name);
        let target = format!("okf:{}", directory.path().join("target").display());
        let report_path = directory.path().join("plan.json");
        let result = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
            .args([
                "plan",
                directory.path().join("source").to_str().unwrap(),
                "--to",
                &target,
                "--report",
                report_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: Value = serde_json::from_slice(&fs::read(report_path).unwrap()).unwrap();
        let report: PlanReport = serde_json::from_value(value.clone()).unwrap();
        mem_adaptor_core::schema::validate("plan-report", &report).unwrap();
        assert!(
            value["entries"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry.get("source_extra").is_none())
        );
        assert!(report.model_calls.is_empty());
        assert!(!directory.path().join("target").exists());
        assert!(
            report
                .anomalies
                .iter()
                .all(|anomaly| anomaly.code != "reader_unreported_field")
        );
    }
}

/// Compares masked complete plans to reviewed goldens after fixing only run/time/path fields and recomputing the digest.
/// These normalized plans are snapshot evidence, not executable approval inputs.
#[test]
fn stable_masked_plan_reports_match_reviewed_snapshots() {
    for name in ["markdown", "chatgpt", "claude"] {
        let directory = fixture(name);
        let mut report = engine(&directory.path().join("target"))
            .plan(&directory.path().join("source"), policy(GateAction::Pass))
            .unwrap();
        report.run_id = "plan-synthetic".into();
        report.created_at = "2026-01-02T03:04:05Z".into();
        report.source.location = "/synthetic/source".into();
        report.targets[0].location = "/synthetic/target".into();
        report.digest_inputs.targets = report.targets.clone();
        report.plan_digest = mem_adaptor_core::engine::plan_digest(&report.digest_inputs).unwrap();
        let mut value = serde_json::to_value(&report).unwrap();
        mem_adaptor_core::gate::mask_value(&mut value);
        let snapshot_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/m4/snapshots/{name}.plan.json"));
        let expected: Value = serde_json::from_slice(&fs::read(snapshot_path).unwrap()).unwrap();
        assert_eq!(value, expected, "Snapshot differs for {name}");
    }
}

/// Checks synthetic ZIP fixtures plan without a target, then write at least one verified record after explicit approval.
#[test]
fn reader_fixtures_work_through_zip_loading_and_explicit_apply() {
    use std::io::Write;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    for name in ["chatgpt", "claude"] {
        let directory = TempDir::new().unwrap();
        let archive_path = directory.path().join("synthetic.zip");
        let mut archive = ZipWriter::new(fs::File::create(&archive_path).unwrap());
        for (path, bytes) in fixture_files(name) {
            archive
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive.finish().unwrap();
        let target = directory.path().join("target");
        let engine = engine(&target);
        let plan = engine
            .plan(&archive_path, policy(GateAction::Pass))
            .unwrap();
        assert_eq!(
            plan.source.location,
            archive_path.canonicalize().unwrap().to_str().unwrap()
        );
        assert!(!target.exists());
        let receipt = apply(&engine, &plan);
        assert!(
            receipt
                .entries
                .iter()
                .any(|entry| entry.verification == Some(Verification::Verified))
        );
    }
}
