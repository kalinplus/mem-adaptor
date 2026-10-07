//! Regression evidence for strict Universal Memory Protocol (UMP) targets and complete incremental history.
//! Synthetic Readers exercise the real engine and approval capability; native JSON assertions do not use Writer decoding.
//! CLI cases capture privacy-safe failures and complete isolated target trees, including genuine partially completed writes.
//! Fault wrappers operate at explicit boundaries; these tests do not claim transactions, rollback, or race-free directory handles.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::{Engine, WriteToken, content_hash, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as source;
use mem_adaptor_core::reports::*;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use mem_adaptor_writer_ump::UmpWriter;
use serde_json::{Value, json};
use tempfile::TempDir;

const FILE: &str = "records.ump.json";

struct SyntheticReader(Vec<CanonicalRecord>);

impl Reader for SyntheticReader {
    /// Matches the stable source system used by all synthetic canonical identities.
    fn id(&self) -> &'static str {
        "ump-repair-test"
    }
    /// Supplies a deterministic adapter version to approval recomputation.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Claims only the fixture inventory; an actually empty directory yields no records.
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
    /// Emits declared body mappings and full typed records for the production validation and policy pipeline.
    fn read(&self, claim: &Claim, _: &SourceFs) -> Result<ReaderOutput> {
        let mut output = source::output();
        for record in &self.0 {
            let mut original = source::source(record, &claim.path, json!({"body": record.content}));
            source::map(&mut original, "/body", "/content");
            output.source_records.push(original);
            output.records.push(record.clone());
        }
        Ok(output)
    }
}

/// Builds a valid isolated source while leaving its target absent.
fn fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(directory.path().join("source/synthetic.json"), "{}").unwrap();
    directory
}

/// Creates stable canonical input without assuming any target-generated time or owner.
fn record(id: &str, body: &str) -> CanonicalRecord {
    source::record(
        "ump-repair-test",
        "test",
        id,
        "synthetic.json",
        body,
        EvidenceLevel::Measured,
    )
}

/// Covers retained optional fields that do not themselves request blocked export or deletion.
fn rich_record(id: &str) -> CanonicalRecord {
    let mut record = record(id, "Exact body.\r\n\n--- retained delimiter ---\n");
    record.scope = Scope::Project;
    record.scope_qualifier = Some("project-address".into());
    record.owner_declared = Some("not-an-authenticated-owner".into());
    record.source_kind = Some("instruction".into());
    record.dna_class = DnaClass::Dna;
    record.source_extra = Some(
        json!({"nested": {"false": false, "null": null, "integer": 7, "list": ["x", 0.25]}})
            .as_object()
            .unwrap()
            .clone(),
    );
    record.tags = Some(vec!["one".into(), "two".into()]);
    record.entities = Some(vec![Entity {
        id: "entity-one".into(),
        kind: "person".into(),
        label: Some("Synthetic entity".into()),
    }]);
    record.relations = Some(vec![Relation {
        kind: "related".into(),
        source: "entity-one".into(),
        target: "entity-two".into(),
    }]);
    record.created_at = Some("2025-01-01T01:02:03Z".into());
    record.updated_at = Some("2025-02-01T01:02:03Z".into());
    record.observed_at = Some("2025-03-01T01:02:03Z".into());
    record.valid_from = Some("2025-04-01T01:02:03Z".into());
    record.valid_to = Some("2025-05-01T01:02:03Z".into());
    record.expires_at = Some("2025-06-01T01:02:03Z".into());
    record.ttl = Some("P1D".into());
    record.provenance = Provenance {
        actor: "synthetic-author".into(),
        actor_kind: ActorKind::User,
        method: "synthetic-source".into(),
        source_ref: Some("source-evidence".into()),
        evidence: Some(vec![Evidence {
            source_ref: "supporting-evidence".into(),
            weight: Some(0.5),
        }]),
    };
    record.consent = Some(Consent {
        exportable: Some(true),
        retention: Some("P2D".into()),
        redact: Some(vec![]),
        memory_enabled: Some(true),
    });
    record.approval = Some(ApprovalState {
        state: ApprovalStatus::Approved,
        receipt_ref: Some("source-approval".into()),
    });
    record.embedding = Some(Embedding {
        model: "synthetic-vector-model".into(),
        dim: 3,
        vector: Some(vec![0.125, -0.5, 0.875]),
        normalized: Some(false),
    });
    record.reembed_plan = Some(ReembedPlan {
        canonical_ids: vec![record.canonical_id.clone()],
        model: "synthetic-next-model".into(),
        dim: 4,
        quality_impact: "An unexecuted source proposal".into(),
    });
    record.conflict_cluster_id = Some(format!("sha256:{}", "c".repeat(64)));
    record.conflict_candidates = Some(vec![ConflictCandidate {
        canonical_id: record.canonical_id.clone(),
        basis: "source-declared".into(),
    }]);
    record.verdict = Some(Verdict::Keep {
        cluster_id: format!("sha256:{}", "c".repeat(64)),
        canonical_ids: vec![record.canonical_id.clone()],
    });
    record
}

/// Registers a selected real Writer or fault seam without replacing core approval logic.
fn engine(records: Vec<CanonicalRecord>, writer: impl Writer + 'static) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(SyntheticReader(records)).unwrap();
    registry.register_writer("home".into(), writer).unwrap();
    Engine { registry }
}

/// Selects either production format for shared-history regressions.
fn format_engine(directory: &TempDir, records: Vec<CanonicalRecord>, format: &str) -> Engine {
    let path = directory.path().join("target");
    match format {
        "ump" => engine(records, UmpWriter::new(path).unwrap()),
        "okf" => engine(records, OkfWriter::new(path).unwrap()),
        _ => unreachable!(),
    }
}

/// Uses explicit local pass policy; privacy tests select block separately.
fn policy() -> GatePolicy {
    GatePolicy {
        secrets: GateAction::Pass,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}

/// Plans against the real source loader and optional genuine prior receipt.
fn plan(engine: &Engine, directory: &TempDir, previous: Option<&Path>) -> PlanReport {
    engine
        .plan_with_previous(&directory.path().join("source"), policy(), previous)
        .unwrap()
}

/// Obtains a real engine-issued WriteToken through approval instead of fabricating a capability.
fn apply(engine: &Engine, report: &PlanReport) -> Result<ReceiptReport> {
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
        "synthetic-plan.json".into(),
        "synthetic-approval.json".into(),
    )
}

/// Saves a real returned receipt under a unique path for the next incremental round.
fn history(directory: &TempDir, receipt: &ReceiptReport, name: &str) -> PathBuf {
    let path = directory.path().join(name);
    write_json_new(&path, receipt).unwrap();
    path
}

/// Captures all isolated ordinary files and directories, never following links or opening special files.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    /// Traverses the fixture tree with exact relative names and bytes.
    fn visit(root: &Path, current: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let relative = path.strip_prefix(root).unwrap().to_owned();
            if metadata.is_dir() {
                output.insert(relative, vec![]);
                visit(root, &path, output);
            } else {
                assert!(
                    metadata.is_file(),
                    "Snapshot encountered a nonregular entry"
                );
                output.insert(relative, fs::read(path).unwrap());
            }
        }
    }
    let mut result = BTreeMap::new();
    if root.exists() {
        visit(root, root, &mut result);
    }
    result
}

/// Reads actual native JSON independently of the production projection and read-back path.
fn native(directory: &TempDir) -> Vec<Value> {
    serde_json::from_slice(&fs::read(directory.path().join("target").join(FILE)).unwrap()).unwrap()
}

/// Finds a native output by its explicitly specified public identity.
fn native_record<'a>(records: &'a [Value], original: &CanonicalRecord) -> &'a Value {
    records
        .iter()
        .find(|native| native["id"] == format!("urn:ump:{}", original.canonical_id))
        .unwrap()
}

/// Requires actual verification and correct execution-evidence references for newly written entries.
fn verified(receipt: &ReceiptReport) {
    assert!(
        receipt
            .entries
            .iter()
            .any(|entry| entry.target_id.is_some())
    );
    for entry in receipt
        .entries
        .iter()
        .filter(|entry| entry.target_id.is_some())
    {
        assert_eq!(entry.verification, Some(Verification::Verified));
        assert_eq!(
            entry.prior_write.as_ref().unwrap().verification,
            Verification::Verified
        );
    }
    assert_eq!(receipt.plan_ref, "synthetic-plan.json");
    assert_eq!(receipt.approval_receipt_ref, "synthetic-approval.json");
}

/// Requires an ordinary attributable refusal without leaking synthetic member names or payloads.
fn refusal(error: &anyhow::Error, cause: &str, private: &str) {
    let message = format!("{error:#}");
    assert!(
        message.contains(cause),
        "Expected refusal category: {cause}"
    );
    assert!(
        !message.contains("panicked"),
        "Ordinary input caused a panic"
    );
    assert!(!message.contains(private), "Refusal exposed private input");
}

/// Supplies a lawful native record with no bridge; preservation must not depend on canonical decoding.
fn foreign() -> Value {
    json!({
        "ump": "1.0", "id": "urn:ump:foreign", "kind": "episodic",
        "body": {"text": "Native user content.", "structured": {"user": {"nested": [false, null, 7]}}},
        "scope": {"owner": "native-owner", "visibility": "shared", "session": "native-session"},
        "time": {"created": "2024-01-02T03:04:05Z"},
        "lifecycle": {"status": "active", "confidence": 0.25},
        "provenance": {"actor": "native-author", "actor_kind": "user", "method": "manual"}
    })
}

/// Writes a bounded native fixture, including malformed variants, without using Writer encoding.
fn write_native(directory: &TempDir, records: &[Value]) {
    fs::create_dir_all(directory.path().join("target")).unwrap();
    fs::write(
        directory.path().join("target").join(FILE),
        serde_json::to_vec(records).unwrap(),
    )
    .unwrap();
}

/// Independently checks all retained optional metadata, vector values, native projections, and package version.
#[test]
fn optional_bridge_native_scope_consent_vector_and_version_are_exact() {
    let directory = fixture();
    let original = rich_record("rich");
    let runner = format_engine(&directory, vec![original.clone()], "ump");
    let approved = plan(&runner, &directory, None);
    // Read the workspace version directly rather than trusting Writer::version.
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")).unwrap();
    let expected_version = manifest
        .split("[workspace.package]")
        .nth(1)
        .unwrap()
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = \"")
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap();
    assert_eq!(approved.writers[0].version, expected_version);
    let receipt = apply(&runner, &approved).unwrap();
    verified(&receipt);
    assert_eq!(receipt.writers[0].version, expected_version);
    assert!(receipt.model_calls.is_empty());
    let records = native(&directory);
    assert_eq!(records.len(), 1);
    let actual = &records[0];
    assert_eq!(actual["kind"], "procedural");
    assert_eq!(
        actual["body"]["text"],
        "Exact body.\r\n\n--- retained delimiter ---\n"
    );
    assert_eq!(actual["scope"]["project"], "project-address");
    assert_eq!(actual["scope"]["visibility"], "private");
    assert_eq!(actual["scope"].as_object().unwrap().len(), 3);
    assert!(
        actual["scope"]["owner"]
            .as_str()
            .unwrap()
            .starts_with("mem-adaptor:sha256:")
    );
    assert_ne!(actual["scope"]["owner"], "not-an-authenticated-owner");
    assert_eq!(
        actual["time"],
        json!({"created": "2025-01-01T01:02:03Z", "observed": "2025-03-01T01:02:03Z",
               "valid_from": "2025-04-01T01:02:03Z", "valid_to": "2025-05-01T01:02:03Z"})
    );
    assert_eq!(
        actual["body"]["structured"]["created_origin"],
        "source_record"
    );
    assert_eq!(
        actual["provenance"],
        json!({"actor": "process:mem-adaptor", "actor_kind": "import", "method": "local_migration",
               "source": {"ref": "synthetic.json"}})
    );
    assert_eq!(
        actual["consent"],
        json!({"exportable": true, "retention": "P2D", "redact": []})
    );
    let bridge = &actual["body"]["structured"]["mem_adaptor"];
    assert_eq!(bridge["source_kind"], "instruction");
    assert_eq!(bridge["dna_class"], "dna");
    assert_eq!(bridge["scope"], "project");
    assert_eq!(bridge["scope_qualifier"], "project-address");
    assert_eq!(bridge["owner_declared"], "not-an-authenticated-owner");
    assert_eq!(
        bridge["source_extra"],
        json!({"nested": {"false": false, "null": null, "integer": 7, "list": ["x", 0.25]}})
    );
    assert_eq!(bridge["tags"], json!(["one", "two"]));
    assert_eq!(
        bridge["entities"],
        json!([{"id": "entity-one", "kind": "person", "label": "Synthetic entity"}])
    );
    assert_eq!(
        bridge["relations"],
        json!([{"kind": "related", "source": "entity-one", "target": "entity-two"}])
    );
    assert_eq!(bridge["created_at"], "2025-01-01T01:02:03Z");
    assert_eq!(bridge["updated_at"], "2025-02-01T01:02:03Z");
    assert_eq!(bridge["observed_at"], "2025-03-01T01:02:03Z");
    assert_eq!(bridge["valid_from"], "2025-04-01T01:02:03Z");
    assert_eq!(bridge["valid_to"], "2025-05-01T01:02:03Z");
    assert_eq!(bridge["expires_at"], "2025-06-01T01:02:03Z");
    assert_eq!(bridge["ttl"], "P1D");
    assert_eq!(
        bridge["provenance"],
        json!({"actor": "synthetic-author", "actor_kind": "user",
        "method": "synthetic-source", "source_ref": "source-evidence",
        "evidence": [{"source_ref": "supporting-evidence", "weight": 0.5}]})
    );
    assert_eq!(
        bridge["consent"],
        json!({"exportable": true, "retention": "P2D", "redact": [], "memory_enabled": true})
    );
    assert_eq!(
        bridge["approval"],
        json!({"state": "approved", "receipt_ref": "source-approval"})
    );
    assert_eq!(
        bridge["embedding"],
        json!({"model": "synthetic-vector-model", "dim": 3,
        "vector": [0.125, -0.5, 0.875], "normalized": false})
    );
    assert_eq!(
        bridge["reembed_plan"],
        json!({"canonical_ids": [original.canonical_id.clone()],
        "model": "synthetic-next-model", "dim": 4, "quality_impact": "An unexecuted source proposal"})
    );
    assert_eq!(
        bridge["conflict_cluster_id"],
        format!("sha256:{}", "c".repeat(64))
    );
    assert_eq!(
        bridge["conflict_candidates"],
        json!([{"canonical_id": original.canonical_id.clone(), "basis": "source-declared"}])
    );
    assert_eq!(
        bridge["verdict"],
        json!({"status": "keep", "cluster_id": format!("sha256:{}", "c".repeat(64)),
        "canonical_ids": [original.canonical_id.clone()]})
    );
    assert!(bridge.get("content").is_none());
    assert!(bridge.get("deletion_intent").is_none());
    assert!(bridge.get("tombstone").is_none());
    assert!(bridge.get("sensitive_findings").is_none());
}

/// Exercises each independent scope qualifier and verifies migration time within the actual write interval.
#[test]
fn four_qualifier_scopes_and_migration_creation_interval_are_native_facts() {
    let directory = fixture();
    let inputs: Vec<_> = [
        (Scope::User, "user", "user-address"),
        (Scope::Project, "project", "project-address"),
        (Scope::Agent, "agent", "agent-address"),
        (Scope::Session, "session", "session-address"),
    ]
    .into_iter()
    .map(|(scope, field, address)| {
        let mut input = record(field, &format!("Body for {field}."));
        input.scope = scope;
        input.scope_qualifier = Some(address.into());
        input
    })
    .collect();
    let runner = format_engine(&directory, inputs.clone(), "ump");
    let approved = plan(&runner, &directory, None);
    let before = time::OffsetDateTime::now_utc();
    let receipt = apply(&runner, &approved).unwrap();
    let after = time::OffsetDateTime::now_utc();
    verified(&receipt);
    let records = native(&directory);
    assert_eq!(records.len(), 4);
    for (input, field) in inputs.iter().zip(["user", "project", "agent", "session"]) {
        let actual = native_record(&records, input);
        assert_eq!(actual["scope"][field], format!("{field}-address"));
        // These SHA-256 expectations were computed independently from the source-system/address contract.
        let expected_owner = match field {
            "user" => {
                "mem-adaptor:sha256:38c80b46949d8f84c0e4c0f02910e99cb39b102f215969b054fbdd8dc2ff0216"
            }
            "project" => {
                "mem-adaptor:sha256:199f2f7f897583683d3e73c1d282c62103baf06fe487ca3cbcea6745078adf35"
            }
            "agent" => {
                "mem-adaptor:sha256:8c2f820a0b747f62c5fa312bff93db5602e52d5d0d1813a549de30eb5110d1d9"
            }
            "session" => {
                "mem-adaptor:sha256:75052e4d3f9b428489dc8b595a2359a623c28206a9568564f3108f57b7d4f0b4"
            }
            _ => unreachable!(),
        };
        assert_eq!(actual["scope"]["owner"], expected_owner);
        for other in ["user", "project", "agent", "session"] {
            assert_eq!(actual["scope"].get(other).is_some(), other == field);
        }
        let created = time::OffsetDateTime::parse(
            actual["time"]["created"].as_str().unwrap(),
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        assert!(before <= created && created <= after);
        assert_eq!(
            actual["body"]["structured"]["created_origin"],
            "target_migration"
        );
        assert!(
            actual["body"]["structured"]["mem_adaptor"]
                .get("created_at")
                .is_none()
        );
        assert_eq!(actual["scope"]["visibility"], "private");
    }
}

/// Separates known conservative mappings from explicit unknown/missing defaults without exposing unknown source values.
#[test]
fn kind_defaults_are_explicit_and_original_strings_stay_only_in_bridge() {
    for (source_kind, expected_kind, expected_rule) in [
        (Some("profile"), "identity", "conservative_kind_identity"),
        (Some("preference"), "semantic", "conservative_kind_semantic"),
        (
            Some("instruction"),
            "procedural",
            "conservative_kind_procedural",
        ),
        (Some("project"), "semantic", "conservative_kind_semantic"),
        (Some("tool"), "semantic", "conservative_kind_semantic"),
        (
            Some("project_doc"),
            "semantic",
            "conservative_kind_semantic",
        ),
        (Some("episodic"), "episodic", "conservative_kind_episodic"),
        (Some("working"), "working", "conservative_kind_working"),
        (
            Some("synthetic-unknown-kind"),
            "semantic",
            "default_kind_semantic_unknown_source_kind",
        ),
        (
            None,
            "semantic",
            "default_kind_semantic_missing_source_kind",
        ),
    ] {
        let directory = fixture();
        let mut input = record("kind", "Do not infer kind from this body.");
        input.source_kind = source_kind.map(str::to_owned);
        let runner = format_engine(&directory, vec![input], "ump");
        let approved = plan(&runner, &directory, None);
        let mapping = approved.entries[0]
            .target_map
            .iter()
            .find(|item| item.target_path == "/kind")
            .unwrap();
        assert_eq!(mapping.canonical_path, "/source_kind");
        assert_eq!(mapping.rule, expected_rule);
        if source_kind == Some("synthetic-unknown-kind") {
            assert!(
                !serde_json::to_string(&approved)
                    .unwrap()
                    .contains("synthetic-unknown-kind")
            );
        }
        verified(&apply(&runner, &approved).unwrap());
        let output = native(&directory);
        assert_eq!(output[0]["kind"], expected_kind);
        assert_eq!(
            output[0]["body"]["structured"]["mem_adaptor"].get("source_kind"),
            source_kind.map(|kind| json!(kind)).as_ref()
        );
    }
}

/// Allows corrected source creation metadata while retaining the first native creation and its true origin.
#[test]
fn source_creation_correction_preserves_first_native_time_and_origin() {
    for source_created in [false, true] {
        let directory = fixture();
        let mut input = record("created", "First source body.");
        if source_created {
            input.created_at = Some("2024-01-02T03:04:05Z".into());
        }
        let first = format_engine(&directory, vec![input.clone()], "ump");
        let receipt = apply(&first, &plan(&first, &directory, None)).unwrap();
        verified(&receipt);
        let old_native = native(&directory)[0].clone();
        let previous = history(&directory, &receipt, "round-one.json");
        input.created_at = Some("2025-06-07T08:09:10Z".into());
        input.content = "Corrected source body.".into();
        input.content_hash = content_hash(input.content.as_bytes());
        let next = format_engine(&directory, vec![input], "ump");
        let approved = plan(&next, &directory, Some(&previous));
        assert_eq!(approved.entries[0].disposition, Disposition::Accepted);
        verified(&apply(&next, &approved).unwrap());
        let updated = native(&directory);
        assert_eq!(updated[0]["time"]["created"], old_native["time"]["created"]);
        assert_eq!(
            updated[0]["body"]["structured"]["created_origin"],
            if source_created {
                "source_record"
            } else {
                "target_migration"
            }
        );
        assert_eq!(
            updated[0]["body"]["structured"]["mem_adaptor"]["created_at"],
            "2025-06-07T08:09:10Z"
        );
        assert_eq!(updated[0]["body"]["text"], "Corrected source body.");
    }
}

/// Native creation-time edits are user changes, not source corrections, and must not be overwritten using stale history.
#[test]
fn native_creation_edit_with_history_is_modified_and_keeps_original_write_evidence() {
    let directory = fixture();
    let mut input = record("native-created", "Original managed body.");
    let initial = format_engine(&directory, vec![input.clone()], "ump");
    let first = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &first, "previous.json");
    let history_bytes = fs::read(&previous).unwrap();
    let mut edited = native(&directory);
    edited[0]["time"]["created"] = json!("2020-01-02T03:04:05Z");
    write_native(&directory, &edited);
    input.content = "A later source body.".into();
    input.content_hash = content_hash(input.content.as_bytes());
    let current = format_engine(&directory, vec![input], "ump");
    let approved = plan(&current, &directory, Some(&previous));
    assert_eq!(
        approved.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified
        }
    );
    let protected = snapshot(&directory.path().join("target"));
    let skipped = apply(&current, &approved).unwrap();
    assert_eq!(snapshot(&directory.path().join("target")), protected);
    assert_eq!(native(&directory), edited);
    assert_eq!(fs::read(&previous).unwrap(), history_bytes);
    assert_eq!(skipped.entries[0].prior_write, first.entries[0].prior_write);
    assert!(skipped.entries[0].target_id.is_none() && skipped.entries[0].verification.is_none());
    assert_eq!(skipped.targets[0].artifacts, first.targets[0].artifacts);
}

/// Refuses historical entries detached from target declarations, duplicate declarations, and changed paths or Writers.
#[test]
fn historical_target_declarations_are_unique_associated_and_destination_bound() {
    for case in ["orphan", "duplicate", "location", "writer"] {
        let directory = fixture();
        let input = record("history", "Historical body.");
        let runner = format_engine(&directory, vec![input.clone()], "ump");
        let receipt = apply(&runner, &plan(&runner, &directory, None)).unwrap();
        let original = history(&directory, &receipt, "original.json");
        let mut damaged = serde_json::to_value(&receipt).unwrap();
        let cause = match case {
            "orphan" => {
                damaged["targets"][0]["id"] = json!("other");
                "lacks a target declaration"
            }
            "duplicate" => {
                let duplicate = damaged["targets"][0].clone();
                damaged["targets"].as_array_mut().unwrap().push(duplicate);
                "Duplicate target"
            }
            "location" => {
                damaged["targets"][0]["location"] =
                    json!(directory.path().join("other").to_str().unwrap());
                "another target"
            }
            _ => {
                damaged["targets"][0]["writer"] = json!("okf");
                "another target"
            }
        };
        let bad = directory.path().join("bad.json");
        fs::write(&bad, serde_json::to_vec(&damaged).unwrap()).unwrap();
        let before = snapshot(directory.path());
        let error = runner
            .plan_with_previous(&directory.path().join("source"), policy(), Some(&bad))
            .unwrap_err();
        refusal(&error, cause, "unused-private-input");
        assert_eq!(snapshot(directory.path()), before);
        assert_eq!(
            fs::read(original).unwrap(),
            before[Path::new("original.json")]
        );
    }
}

/// A genuinely new logical target may use existing history without stealing another target's write evidence.
#[test]
fn new_target_without_related_historical_entries_is_legal() {
    let directory = fixture();
    let input = record("new-target", "A new satellite destination.");
    let first = format_engine(&directory, vec![input.clone()], "ump");
    let receipt = apply(&first, &plan(&first, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let before = snapshot(&directory.path().join("target"));
    let mut registry = Registry::default();
    registry
        .register_reader(SyntheticReader(vec![input]))
        .unwrap();
    registry
        .register_writer(
            "home".into(),
            UmpWriter::new(directory.path().join("target")).unwrap(),
        )
        .unwrap();
    registry
        .register_writer(
            "new".into(),
            UmpWriter::new(directory.path().join("new-target")).unwrap(),
        )
        .unwrap();
    let runner = Engine { registry };
    let approved = plan(&runner, &directory, Some(&previous));
    assert_eq!(
        approved
            .entries
            .iter()
            .find(|entry| entry.target == "home")
            .unwrap()
            .disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
    let new = approved
        .entries
        .iter()
        .find(|entry| entry.target == "new")
        .unwrap();
    assert_eq!(new.disposition, Disposition::Accepted);
    assert!(new.prior_write.is_none() && new.duplicate_write.is_none());
    verified(&apply(&runner, &approved).unwrap());
    assert_eq!(snapshot(&directory.path().join("target")), before);
    let output: Vec<Value> =
        serde_json::from_slice(&fs::read(directory.path().join("new-target").join(FILE)).unwrap())
            .unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["body"]["text"], "A new satellite destination.");
}

/// Binds complete old receipt bytes even when current predictions have no records, for both shared formats.
#[test]
fn approved_history_cannot_drop_entries_change_shared_proof_or_verdict_on_empty_source() {
    for format in ["ump", "okf"] {
        for empty in [false, true] {
            for tamper in ["entries", "shared_hash", "verdict", "whitespace"] {
                let directory = fixture();
                let input = record("binding", "A receipt-bound memory.");
                let initial = format_engine(&directory, vec![input.clone()], format);
                let mut receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
                receipt.verdicts.push(Verdict::NeedsMoreContext {
                    cluster_id: format!("sha256:{}", "a".repeat(64)),
                });
                let previous = history(&directory, &receipt, "previous.json");
                let original_bytes = fs::read(&previous).unwrap();
                if empty {
                    fs::remove_file(directory.path().join("source/synthetic.json")).unwrap();
                }
                let current =
                    format_engine(&directory, if empty { vec![] } else { vec![input] }, format);
                let approved = plan(&current, &directory, Some(&previous));
                let serialized = serde_json::to_value(&approved).unwrap();
                assert_eq!(
                    serialized["digest_inputs"]["previous_receipt_hash"],
                    content_hash(&original_bytes)
                );
                assert_eq!(approved.entries.is_empty(), empty);
                let protected = snapshot(&directory.path().join("target"));
                let mut modified = serde_json::to_value(&receipt).unwrap();
                match tamper {
                    "entries" => modified["entries"] = json!([]),
                    "shared_hash" => {
                        modified["targets"][0]["artifacts"][0]["content_hash"] =
                            json!(format!("sha256:{}", "b".repeat(64)))
                    }
                    "verdict" => {
                        modified["verdicts"][0]["cluster_id"] =
                            json!(format!("sha256:{}", "d".repeat(64)))
                    }
                    "whitespace" => {}
                    _ => unreachable!(),
                }
                let bytes = if tamper == "whitespace" {
                    let mut bytes = original_bytes.clone();
                    bytes.extend_from_slice(b"\n \t");
                    bytes
                } else {
                    serde_json::to_vec(&modified).unwrap()
                };
                fs::write(&previous, &bytes).unwrap();
                let before = snapshot(directory.path());
                let error = apply(&current, &approved).unwrap_err();
                refusal(&error, "Plan digest mismatch", "unused-private-input");
                assert!(format!("{error:#}").contains("[S7]"));
                assert!(format!("{error:#}").contains("before target writes began"));
                assert_eq!(snapshot(directory.path()), before);
                assert_eq!(snapshot(&directory.path().join("target")), protected);
                assert_eq!(fs::read(&previous).unwrap(), bytes);
            }
        }
    }
}

/// Preserves own/duplicate write evidence and shared proofs through five rounds without inventing source deletion or resurrection.
#[test]
fn five_round_missing_duplicate_and_target_deletion_history_is_incremental() {
    for format in ["ump", "okf"] {
        let directory = fixture();
        let mut inputs = vec![
            record("one", "Duplicate memory."),
            record("two", "Duplicate memory."),
        ];
        inputs.sort_by(|left, right| left.canonical_id.cmp(&right.canonical_id));
        let initial = format_engine(&directory, inputs.clone(), format);
        let first = apply(&initial, &plan(&initial, &directory, None)).unwrap();
        verified(&first);
        let representative = first
            .entries
            .iter()
            .find(|entry| entry.prior_write.is_some())
            .unwrap();
        let alias = first
            .entries
            .iter()
            .find(|entry| entry.duplicate_write.is_some())
            .unwrap();
        assert_eq!(
            alias.duplicate_write.as_ref().unwrap().canonical_id,
            representative.canonical_id
        );
        let first_prior = representative.prior_write.clone().unwrap();
        let first_duplicate = alias.duplicate_write.clone().unwrap();
        let baseline = snapshot(&directory.path().join("target"));
        let mut previous = history(&directory, &first, "round-one.json");
        // Round two removes only the representative from the source, retaining its target and alias association.
        let alias_input = inputs
            .iter()
            .find(|input| input.canonical_id == alias.canonical_id)
            .unwrap()
            .clone();
        let second_engine = format_engine(&directory, vec![alias_input], format);
        let second = apply(
            &second_engine,
            &plan(&second_engine, &directory, Some(&previous)),
        )
        .unwrap();
        assert_eq!(snapshot(&directory.path().join("target")), baseline);
        assert_eq!(
            serde_json::to_value(
                &second
                    .entries
                    .iter()
                    .find(|entry| entry.canonical_id == representative.canonical_id)
                    .unwrap()
                    .disposition
            )
            .unwrap(),
            json!({"status": "omitted", "reason": {"code": "source_missing"}})
        );
        assert_eq!(
            second
                .entries
                .iter()
                .find(|entry| entry.canonical_id == alias.canonical_id)
                .unwrap()
                .duplicate_write,
            Some(first_duplicate.clone())
        );
        previous = history(&directory, &second, "round-two.json");
        // Round three is a truly empty source, not a Reader parsing error or a deletion request.
        fs::remove_file(directory.path().join("source/synthetic.json")).unwrap();
        let empty = format_engine(&directory, vec![], format);
        let empty_plan = plan(&empty, &directory, Some(&previous));
        assert_eq!(empty_plan.source_inventory.state, InventoryState::Empty);
        let third = apply(&empty, &empty_plan).unwrap();
        assert_eq!(third.entries.len(), 2);
        assert_eq!(third.targets, first.targets);
        assert_eq!(snapshot(&directory.path().join("target")), baseline);
        for entry in &third.entries {
            assert_eq!(
                serde_json::to_value(&entry.disposition).unwrap(),
                json!({"status": "omitted", "reason": {"code": "source_missing"}})
            );
            assert!(entry.verification.is_none());
        }
        assert_eq!(
            third
                .entries
                .iter()
                .find(|entry| entry.canonical_id == representative.canonical_id)
                .unwrap()
                .prior_write,
            Some(first_prior.clone())
        );
        assert_eq!(
            third
                .entries
                .iter()
                .find(|entry| entry.canonical_id == alias.canonical_id)
                .unwrap()
                .duplicate_write,
            Some(first_duplicate.clone())
        );
        previous = history(&directory, &third, "round-three.json");
        // Round four observes a manual target deletion while still empty, retaining the original evidence.
        let deleted = if format == "ump" {
            directory.path().join("target").join(FILE)
        } else {
            directory
                .path()
                .join("target")
                .join(format!("{}.md", first_prior.target_id))
        };
        fs::remove_file(&deleted).unwrap();
        let deleted_snapshot = snapshot(&directory.path().join("target"));
        let fourth = apply(&empty, &plan(&empty, &directory, Some(&previous))).unwrap();
        let shared_paths: &[&str] = if format == "ump" {
            &[FILE]
        } else {
            &["index.md", "log.md"]
        };
        for path in shared_paths {
            assert_eq!(
                fourth.targets[0]
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.path == *path)
                    .unwrap(),
                first.targets[0]
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.path == *path)
                    .unwrap(),
            );
        }
        if format == "okf" {
            let observed = fourth.targets[0]
                .artifacts
                .iter()
                .find(|artifact| artifact.path == format!("{}.md", first_prior.target_id))
                .unwrap();
            assert!(observed.content_hash.is_none() && observed.bytes.is_none());
        }
        assert_eq!(snapshot(&directory.path().join("target")), deleted_snapshot);
        assert_eq!(
            fourth
                .entries
                .iter()
                .find(|entry| entry.canonical_id == representative.canonical_id)
                .unwrap()
                .prior_write,
            Some(first_prior)
        );
        assert_eq!(
            fourth
                .entries
                .iter()
                .find(|entry| entry.canonical_id == alias.canonical_id)
                .unwrap()
                .duplicate_write,
            Some(first_duplicate)
        );
        previous = history(&directory, &fourth, "round-four.json");
        // Round five returns both source records; neither the representative nor its alias may resurrect.
        fs::write(directory.path().join("source/synthetic.json"), "{}").unwrap();
        let returned = format_engine(&directory, inputs, format);
        let approved = plan(&returned, &directory, Some(&previous));
        for entry in &approved.entries {
            assert_eq!(
                entry.disposition,
                Disposition::Omitted {
                    reason: OmissionReason::DeletedInTarget
                }
            );
        }
        let fifth = apply(&returned, &approved).unwrap();
        assert!(
            fifth
                .entries
                .iter()
                .all(|entry| entry.target_id.is_none() && entry.verification.is_none())
        );
        assert_eq!(snapshot(&directory.path().join("target")), deleted_snapshot);
        assert!(!deleted.exists());
    }
}

/// Explicit deletion requests remain pending human decisions and never become executable target deletion.
#[test]
fn explicit_deletion_and_tombstone_do_not_delete_an_existing_ump_record() {
    for tombstone in [false, true] {
        let directory = fixture();
        let mut input = record("delete", "Preserve this memory.");
        let initial = format_engine(&directory, vec![input.clone()], "ump");
        let first = apply(&initial, &plan(&initial, &directory, None)).unwrap();
        let previous = history(&directory, &first, "previous.json");
        if tombstone {
            input.tombstone = Some(Tombstone {
                deleted_at: "2025-01-02T03:04:05Z".into(),
                source_ref: "source-delete-request".into(),
                actor: Some("synthetic-user".into()),
            });
        } else {
            input.deletion_intent = Some(DeletionIntent::Delete);
        }
        let runner = format_engine(&directory, vec![input], "ump");
        let approved = plan(&runner, &directory, Some(&previous));
        assert_eq!(
            approved.entries[0].disposition,
            Disposition::Unresolved {
                reason: UnresolvedReason::DeletionNeedsDecision
            }
        );
        let before = snapshot(&directory.path().join("target"));
        let receipt = apply(&runner, &approved).unwrap();
        assert_eq!(snapshot(&directory.path().join("target")), before);
        assert_eq!(
            receipt.entries[0].disposition,
            approved.entries[0].disposition
        );
        assert!(receipt.entries[0].target_id.is_none());
        assert_eq!(receipt.entries[0].prior_write, first.entries[0].prior_write);
    }
}

/// Rejects duplicate members before selecting values, including nested and escaped-equivalent sensitive keys.
#[test]
fn duplicate_json_members_are_refused_without_echo_or_any_target_changes() {
    for shape in ["root", "body", "nested", "escaped_sensitive"] {
        let directory = fixture();
        let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
        let ordinary = serde_json::to_string(&foreign()).unwrap();
        let malformed = match shape {
            "root" => ordinary.replacen("\"ump\":\"1.0\"", "\"ump\":\"1.0\",\"ump\":\"1.0\"", 1),
            "body" => ordinary.replacen(
                "\"text\":\"Native user content.\"",
                "\"text\":\"Native user content.\",\"text\":\"Unapproved selected content.\"",
                1,
            ),
            "nested" => ordinary.replacen(
                "\"nested\":[false,null,7]",
                &format!("\"nested\":[false,null,7],\"nested\":\"{secret}\""),
                1,
            ),
            _ => ordinary.replacen(
                "\"user\":",
                &format!("\"{secret}\":1,\"\\u0073{}\":2,\"user\":", &secret[1..]),
                1,
            ),
        };
        assert!(
            malformed != ordinary,
            "Duplicate-member fixture was not injected"
        );
        fs::create_dir(directory.path().join("target")).unwrap();
        fs::write(
            directory.path().join("target/user.txt"),
            b"Protected unrelated bytes.",
        )
        .unwrap();
        fs::write(
            directory.path().join("target").join(FILE),
            format!("[{malformed}]"),
        )
        .unwrap();
        let runner = format_engine(&directory, vec![record("unrelated", "Never write.")], "ump");
        let before = snapshot(directory.path());
        let error = runner
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        refusal(&error, "Duplicate", &secret);
        assert!(
            snapshot(directory.path()) == before,
            "Refusal changed private fixture bytes"
        );
        let output = cli_plan(&directory, "refused-plan.json", &[]);
        let message = cli_failure(&output, "Duplicate", &secret);
        assert!(message.contains("target unchanged"));
        assert!(
            snapshot(directory.path()) == before,
            "CLI refusal changed private fixture bytes"
        );
    }
}

/// Maintains array, identity, official-schema, and safe-integer checks alongside stricter duplicate parsing.
#[test]
fn malformed_native_envelopes_refuse_before_plan_and_preserve_all_bytes() {
    for case in ["not_array", "duplicate_id", "schema", "unsafe_integer"] {
        let directory = fixture();
        let mut lawful = foreign();
        let expected = match case {
            "not_array" => "array",
            "duplicate_id" => "Duplicate UMP target identity",
            "schema" => {
                lawful["kind"] = json!("not-a-native-kind");
                "official schema"
            }
            _ => {
                lawful["body"]["structured"]["unsafe_integer"] = json!(9_007_199_254_740_992_u64);
                "JCS safe range"
            }
        };
        write_native(&directory, &[lawful.clone()]);
        if case == "not_array" {
            fs::write(
                directory.path().join("target").join(FILE),
                serde_json::to_vec(&lawful).unwrap(),
            )
            .unwrap();
        } else if case == "duplicate_id" {
            write_native(&directory, &[lawful.clone(), lawful]);
        }
        let runner = format_engine(&directory, vec![record("new", "Not exported.")], "ump");
        let before = snapshot(directory.path());
        let error = runner
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        refusal(&error, expected, "unused-private-input");
        assert_eq!(snapshot(directory.path()), before);
    }
}

/// Clearly declared malformed bridges are ordinary errors even when no current source references the managed record.
#[test]
fn scalar_array_boolean_number_null_and_incomplete_bridges_refuse_unrelated_writes() {
    for bad in [
        json!("scalar"),
        json!([]),
        json!(true),
        json!(7),
        Value::Null,
        json!({}),
        json!({"canonical_id": 9}),
    ] {
        let directory = fixture();
        let input = record("managed", "Existing managed body.");
        let initial = format_engine(&directory, vec![input], "ump");
        let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
        verified(&receipt);
        let mut records = native(&directory);
        records[0]["body"]["structured"]["mem_adaptor"] = bad;
        records.push(foreign());
        write_native(&directory, &records);
        let runner = format_engine(
            &directory,
            vec![record("unrelated", "Unrelated new body.")],
            "ump",
        );
        let before = snapshot(directory.path());
        let error = runner
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        refusal(&error, "UMP", "unused-private-input");
        assert!(
            format!("{error:#}").contains("metadata") || format!("{error:#}").contains("bridge")
        );
        assert_eq!(snapshot(directory.path()), before);
        assert_eq!(native(&directory).last().unwrap(), &foreign());
    }
}

/// Ordinary foreign records may be preserved but cannot be decoded or taken over as managed canonical records.
#[test]
fn legitimate_foreign_native_records_survive_new_managed_writes_untouched() {
    let directory = fixture();
    let original_foreign = foreign();
    write_native(&directory, std::slice::from_ref(&original_foreign));
    fs::write(
        directory.path().join("target/user.txt"),
        b"Unrelated user file.",
    )
    .unwrap();
    let input = record("managed", "A new managed memory.");
    let runner = format_engine(&directory, vec![input], "ump");
    let approved = plan(&runner, &directory, None);
    assert_eq!(approved.entries[0].disposition, Disposition::Accepted);
    verified(&apply(&runner, &approved).unwrap());
    let actual = native(&directory);
    assert_eq!(actual.len(), 2);
    assert_eq!(
        actual
            .iter()
            .find(|value| value["id"] == "urn:ump:foreign")
            .unwrap(),
        &original_foreign
    );
    assert_eq!(
        fs::read(directory.path().join("target/user.txt")).unwrap(),
        b"Unrelated user file."
    );
    let writer = runner.registry.writer("home").unwrap();
    assert!(writer.inspect("urn:ump:foreign").unwrap().is_none());
    assert!(writer.target_hash("urn:ump:foreign").unwrap().is_some());
}

/// Refuses native projection conflicts and corrupted canonical identity, body hash, or vector before unrelated writes.
#[test]
fn managed_native_and_bridge_conflicts_are_revalidated_on_inspection_and_readback() {
    for case in [
        "kind",
        "scope",
        "visibility",
        "owner",
        "consent",
        "provenance",
        "observed",
        "created_origin",
        "body_hash",
        "source_identity",
        "native_identity",
        "vector",
    ] {
        let directory = fixture();
        let input = rich_record("inspect");
        let initial = format_engine(&directory, vec![input.clone()], "ump");
        let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
        let mut records = native(&directory);
        let managed = &mut records[0];
        match case {
            "kind" => managed["kind"] = json!("identity"),
            "scope" => managed["scope"]["project"] = json!("user-edit"),
            "visibility" => managed["scope"]["visibility"] = json!("public"),
            "owner" => managed["scope"]["owner"] = json!("user-edited-owner"),
            "consent" => managed["consent"]["exportable"] = json!(false),
            "provenance" => managed["provenance"]["actor"] = json!("user-edited-actor"),
            "observed" => managed["time"]["observed"] = json!("2026-01-02T03:04:05Z"),
            "created_origin" => {
                managed["body"]["structured"]["created_origin"] = json!("invented-origin")
            }
            "body_hash" => managed["body"]["text"] = json!("User body edit."),
            "source_identity" => {
                managed["body"]["structured"]["mem_adaptor"]["source_record_id"] =
                    json!("other-source-id")
            }
            "native_identity" => managed["id"] = json!("urn:ump:another"),
            "vector" => managed["body"]["structured"]["mem_adaptor"]["embedding"]["dim"] = json!(4),
            _ => unreachable!(),
        }
        write_native(&directory, &records);
        let before = snapshot(directory.path());
        let writer = UmpWriter::new(directory.path().join("target")).unwrap();
        let native_id = records[0]["id"].as_str().unwrap();
        let expected_cause = match case {
            "body_hash" => "body hash mismatch",
            "source_identity" => "source identity mismatch",
            "native_identity" => "bridge identity mismatch",
            "vector" => "vector length does not match dimension",
            "created_origin" => "creation origin",
            _ => "native projection contradicts migration metadata",
        };
        let inspection = writer.inspect(native_id).unwrap_err();
        refusal(&inspection, expected_cause, "unused-private-input");
        let readback = writer
            .read_back(&[Written {
                canonical_id: input.canonical_id.clone(),
                target_id: native_id.into(),
                target_hash: receipt.entries[0]
                    .prior_write
                    .as_ref()
                    .unwrap()
                    .target_hash
                    .clone(),
            }])
            .err()
            .expect("Corrupt managed record must refuse read-back");
        refusal(&readback, expected_cause, "unused-private-input");
        let unrelated = format_engine(
            &directory,
            vec![record("new", "Unrelated write must not happen.")],
            "ump",
        );
        let error = unrelated
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        refusal(&error, expected_cause, "unused-private-input");
        assert_eq!(snapshot(directory.path()), before);
        assert_eq!(native(&directory), records);
    }
}

/// Fresh batch observations must see subsequent edits and errors rather than reuse a persistent native-file cache.
#[test]
fn batch_inspection_and_hashes_reobserve_each_phase_without_stale_cache() {
    let directory = fixture();
    let input = record("batch", "Original observed body.");
    let runner = format_engine(&directory, vec![input.clone()], "ump");
    let receipt = apply(&runner, &plan(&runner, &directory, None)).unwrap();
    let id = receipt.entries[0].target_id.clone().unwrap();
    let writer = UmpWriter::new(directory.path().join("target")).unwrap();
    let first = writer.inspect_many(std::slice::from_ref(&id)).unwrap();
    assert_eq!(
        first[&id].record.as_ref().unwrap().content,
        "Original observed body."
    );
    let first_hash = first[&id].target_hash.clone();
    let mut actual = native(&directory);
    actual[0]["time"]["created"] = json!("2020-01-02T03:04:05Z");
    write_native(&directory, &actual);
    let second = writer.inspect_many(std::slice::from_ref(&id)).unwrap();
    assert_ne!(second[&id].target_hash, first_hash);
    assert_eq!(
        writer.target_hashes(std::slice::from_ref(&id)).unwrap()[&id],
        second[&id].target_hash
    );
    actual[0]["body"]["structured"]["mem_adaptor"] = Value::Null;
    write_native(&directory, &actual);
    let before = snapshot(directory.path());
    refusal(
        &writer
            .inspect_many(&[])
            .err()
            .expect("Empty inspection must preflight managed records"),
        "UMP",
        "unused-private-input",
    );
    refusal(
        &writer.target_hashes(std::slice::from_ref(&id)).unwrap_err(),
        "UMP",
        "unused-private-input",
    );
    assert_eq!(snapshot(directory.path()), before);
}

enum Injection {
    Body,
    Duplicate,
    Destination(PathBuf),
    BeforePersist,
    ReadBack,
    AfterReadBack,
    ReadBackMismatch,
    #[cfg(unix)]
    SwapParent {
        parent: PathBuf,
        moved: PathBuf,
        outside: PathBuf,
    },
}

struct BoundaryWriter {
    inner: UmpWriter,
    injection: Injection,
}

impl Writer for BoundaryWriter {
    /// Preserves actual plugin identity so real tokens cannot authorize a substituted adapter.
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    /// Keeps version comparison tied to the real implementation.
    fn version(&self) -> &'static str {
        self.inner.version()
    }
    /// Exposes the approved physical path until the specified fault boundary.
    fn location(&self) -> &Path {
        self.inner.location()
    }
    /// Retains production supported-field declarations for mismatch verification.
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    /// Delegates planning unchanged so tampering happens only after approval.
    fn plan(&self, record: &CanonicalRecord, previous: Option<&ReceiptEntry>) -> Result<Planned> {
        self.inner.plan(record, previous)
    }
    /// Injects unapproved batches, destinations, first-persistence I/O failure, or an explicit parent swap.
    fn write(&self, batch: &[Planned], token: &WriteToken) -> Result<WriteResult> {
        match &self.injection {
            Injection::Body | Injection::Duplicate => {
                let mut altered: Vec<_> = batch.iter().map(copy_planned).collect();
                if matches!(self.injection, Injection::Body) {
                    altered[0].record.content.push_str("Unapproved mutation.");
                    altered[0].record.content_hash =
                        content_hash(altered[0].record.content.as_bytes());
                } else {
                    altered.push(copy_planned(&batch[0]));
                }
                self.inner.write(&altered, token)
            }
            Injection::Destination(path) => UmpWriter::new(path.clone())?.write(batch, token),
            Injection::BeforePersist => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Synthetic first persistence refused",
            )
            .into()),
            #[cfg(unix)]
            Injection::SwapParent {
                parent,
                moved,
                outside,
            } => {
                fs::rename(parent, moved)?;
                std::os::unix::fs::symlink(outside, parent)?;
                self.inner.write(batch, token)
            }
            _ => self.inner.write(batch, token),
        }
    }
    /// Separates ordinary read-back I/O, explicit supported-field mismatch, and output edits after reading.
    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>> {
        if matches!(self.injection, Injection::ReadBack) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Synthetic read-back refused",
            )
            .into());
        }
        let mut result = self.inner.read_back(written)?;
        if matches!(self.injection, Injection::ReadBackMismatch) {
            result[0].record.tags = Some(vec!["Synthetic unexpected read-back tag".into()]);
        }
        if matches!(self.injection, Injection::AfterReadBack) {
            let path = self.location().join(FILE);
            let mut actual: Vec<Value> = serde_json::from_slice(&fs::read(&path)?)?;
            actual[0]["time"]["created"] = json!("2020-01-02T03:04:05Z");
            fs::write(path, serde_json::to_vec(&actual)?)?;
        }
        Ok(result)
    }
    /// Performs real managed-record validation before writes.
    fn inspect(&self, id: &str) -> Result<Option<CanonicalRecord>> {
        self.inner.inspect(id)
    }
    /// Reads native hash evidence without suppressing parsing errors.
    fn target_hash(&self, id: &str) -> Result<Option<String>> {
        self.inner.target_hash(id)
    }
    /// Preserves the production one-phase batch seam instead of multiplying single-record reads.
    fn inspect_many(&self, ids: &[String]) -> Result<BTreeMap<String, TargetState>> {
        self.inner.inspect_many(ids)
    }
    /// Rechecks hashes freshly after read-back.
    fn target_hashes(&self, ids: &[String]) -> Result<BTreeMap<String, Option<String>>> {
        self.inner.target_hashes(ids)
    }
    /// Keeps whole native-file artifact proofs actual.
    fn artifacts(&self, ids: &[String]) -> Result<Vec<TargetArtifact>> {
        self.inner.artifacts(ids)
    }
    /// Preserves the real shared-file protection contract.
    fn shared_artifact_paths(&self) -> &'static [&'static str] {
        self.inner.shared_artifact_paths()
    }
}

/// Copies only the public approved proposal fields for deliberate token-boundary attacks.
fn copy_planned(planned: &Planned) -> Planned {
    Planned {
        record: planned.record.clone(),
        disposition: planned.disposition.clone(),
        target_id: planned.target_id.clone(),
        previous_write: planned.previous_write.clone(),
        duplicate_write: planned.duplicate_write.clone(),
        target_map: planned.target_map.clone(),
    }
}

/// The actual UMP implementation rejects body, entry, and destination tampering with genuine engine-issued tokens.
#[test]
fn real_ump_write_tokens_reject_unapproved_body_duplicate_and_destination() {
    for mode in 0..3 {
        let directory = fixture();
        write_native(&directory, &[foreign()]);
        fs::create_dir(directory.path().join("outside")).unwrap();
        fs::write(
            directory.path().join("outside/user.txt"),
            b"Outside protected bytes.",
        )
        .unwrap();
        let injection = match mode {
            0 => Injection::Body,
            1 => Injection::Duplicate,
            _ => Injection::Destination(directory.path().join("outside")),
        };
        let runner = engine(
            vec![record("token", "Approved memory.")],
            BoundaryWriter {
                inner: UmpWriter::new(directory.path().join("target")).unwrap(),
                injection,
            },
        );
        let approved = plan(&runner, &directory, None);
        let before = snapshot(directory.path());
        let error = apply(&runner, &approved).unwrap_err();
        refusal(
            &error,
            "WriteToken does not authorize",
            "unused-private-input",
        );
        assert_eq!(snapshot(directory.path()), before);
    }
}

/// Write and read-back errors describe possible side effects truthfully and never return fabricated verified receipts.
#[test]
fn ump_persistence_readback_and_postread_proof_failures_preserve_actual_evidence() {
    for injection in [
        Injection::BeforePersist,
        Injection::ReadBack,
        Injection::AfterReadBack,
    ] {
        let directory = fixture();
        let prewrite = matches!(injection, Injection::BeforePersist);
        let postread = matches!(injection, Injection::AfterReadBack);
        let runner = engine(
            vec![record("fault", "Actual approved body.")],
            BoundaryWriter {
                inner: UmpWriter::new(directory.path().join("target")).unwrap(),
                injection,
            },
        );
        let approved = plan(&runner, &directory, None);
        let before = snapshot(directory.path());
        let error = apply(&runner, &approved).unwrap_err();
        let message = format!("{error:#}");
        refusal(
            &error,
            if prewrite { "[S8]" } else { "[S9]" },
            "unused-private-input",
        );
        for context in [
            "targets may be partially changed",
            "No reliable final receipt",
            "do not blindly retry",
        ] {
            assert!(
                message.contains(context),
                "Missing failure boundary: {context}"
            );
        }
        if !postread {
            assert!(error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
            }));
        }
        if prewrite {
            assert_eq!(snapshot(directory.path()), before);
        } else {
            assert_ne!(snapshot(directory.path()), before);
            let actual = native(&directory);
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0]["body"]["text"], "Actual approved body.");
            if postread {
                assert!(
                    message.contains("native payload changed")
                        || message.contains("output changed")
                );
                assert_eq!(actual[0]["time"]["created"], "2020-01-02T03:04:05Z");
            }
        }
    }
}

/// Supported-field mismatches remain mismatch evidence, not a verified historical write, despite correct native bytes.
#[test]
fn readback_mismatch_receipt_does_not_claim_verified_native_write() {
    let directory = fixture();
    let runner = engine(
        vec![record("mismatch", "Unchanged native content.")],
        BoundaryWriter {
            inner: UmpWriter::new(directory.path().join("target")).unwrap(),
            injection: Injection::ReadBackMismatch,
        },
    );
    let approved = plan(&runner, &directory, None);
    let receipt = apply(&runner, &approved).unwrap();
    let Some(Verification::Mismatch { diff }) = &receipt.entries[0].verification else {
        panic!("Read-back mismatch was mislabeled");
    };
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].field_path, "/tags");
    assert_eq!(diff[0].kind, DiffKind::Changed);
    assert!(matches!(
        receipt.entries[0]
            .prior_write
            .as_ref()
            .unwrap()
            .verification,
        Verification::Mismatch { .. }
    ));
    let actual = native(&directory);
    assert_eq!(actual[0]["body"]["text"], "Unchanged native content.");
    assert!(
        actual[0]["body"]["structured"]["mem_adaptor"]
            .get("tags")
            .is_none()
    );
    let previous = history(&directory, &receipt, "mismatch-history.json");
    let next = plan(&runner, &directory, Some(&previous));
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified
        }
    );
}

/// A parent swapped at the actual Writer entrance cannot redirect a real approved UMP write outside its tree.
#[cfg(unix)]
#[test]
fn writer_parent_swap_refuses_without_touching_original_or_outside_bytes() {
    let directory = fixture();
    let parent = directory.path().join("parent");
    let moved = directory.path().join("moved");
    let outside = directory.path().join("outside");
    for path in [&parent, &outside] {
        fs::create_dir_all(path.join("target")).unwrap();
        fs::write(path.join("target/user.txt"), b"Protected target bytes.").unwrap();
    }
    let original = snapshot(&parent);
    let external = snapshot(&outside);
    let runner = engine(
        vec![record("parent", "Do not redirect.")],
        BoundaryWriter {
            inner: UmpWriter::new(parent.join("target")).unwrap(),
            injection: Injection::SwapParent {
                parent: parent.clone(),
                moved: moved.clone(),
                outside: outside.clone(),
            },
        },
    );
    let approved = plan(&runner, &directory, None);
    let error = apply(&runner, &approved).unwrap_err();
    refusal(&error, "symlink", "unused-private-input");
    assert!(
        fs::symlink_metadata(&parent)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(snapshot(&moved), original);
    assert_eq!(snapshot(&outside), external);
}

/// Creates actual Markdown input so CLI integration cannot be satisfied by the synthetic Reader seam.
fn cli_fixture(body: &str) -> TempDir {
    let directory = fixture();
    fs::remove_file(directory.path().join("source/synthetic.json")).unwrap();
    fs::write(directory.path().join("source/note.md"), body).unwrap();
    directory
}

/// Captures both real CLI output streams without remote services or an interactive terminal.
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(args)
        .env("LOG_LEVEL", "info")
        .output()
        .unwrap()
}

/// Runs a UMP CLI plan with a named report and additional authorized options.
fn cli_plan(directory: &TempDir, report: &str, options: &[&str]) -> Output {
    let root = directory.path();
    cli(&[
        &[
            "plan",
            root.join("source").to_str().unwrap(),
            "--to",
            &format!("ump:{}", root.join("target").display()),
            "--report",
            root.join(report).to_str().unwrap(),
        ][..],
        options,
    ]
    .concat())
}

/// Checks output privacy using booleans so failed assertions never print the synthetic secret itself.
fn private_output(output: &Output, secret: &str) {
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains(secret),
        "CLI stdout leaked private input"
    );
    assert!(
        !String::from_utf8_lossy(&output.stderr).contains(secret),
        "CLI stderr leaked private input"
    );
}

/// Requires an ordinary CLI refusal with an attributable cause, never merely a nonzero status.
fn cli_failure(output: &Output, cause: &str, secret: &str) -> String {
    assert!(!output.status.success(), "CLI unexpectedly succeeded");
    private_output(output, secret);
    let message = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !message.contains("panicked"),
        "CLI crashed instead of returning an error"
    );
    assert!(
        message.contains(cause),
        "CLI refusal has the wrong cause: {cause}"
    );
    message
}

/// Parses saved plan/receipt JSON as independent evidence.
fn document(directory: &TempDir, name: &str) -> Value {
    serde_json::from_slice(&fs::read(directory.path().join(name)).unwrap()).unwrap()
}

/// Pass exports the exact sensitive synthetic body, while block reports detection without putting it in UMP or credentials.
#[test]
fn cli_pass_and_block_preserve_privacy_and_actual_native_policy_semantics() {
    let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    for mode in ["pass", "block"] {
        let body = format!("Synthetic local memory.\n{secret}\n");
        let directory = cli_fixture(&body);
        let root = directory.path();
        write_native(&directory, &[foreign()]);
        let before = snapshot(&root.join("target"));
        let planned = cli_plan(&directory, "plan.json", &["--secret-policy", mode]);
        private_output(&planned, &secret);
        assert!(planned.status.success(), "CLI plan failed");
        assert!(
            snapshot(&root.join("target")) == before,
            "Planning changed target bytes"
        );
        let report = document(&directory, "plan.json");
        assert_eq!(report["gate_policy"]["secrets"], mode);
        assert_eq!(report["gate_policy"]["origin"], "user_choice");
        assert_eq!(report["gate_policy"]["user_selected"], true);
        assert!(
            !report["entries"][0]["sensitive_findings"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            report["entries"][0]["sensitive_findings"][0]["disposition"],
            if mode == "pass" { "passed" } else { "blocked" }
        );
        let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
        private_output(&output, &secret);
        assert!(output.status.success(), "CLI apply failed");
        for name in ["plan.json", "plan.approval.json", "plan.receipt.json"] {
            assert!(
                !String::from_utf8_lossy(&fs::read(root.join(name)).unwrap()).contains(&secret),
                "Saved credential leaked private input"
            );
        }
        let receipt = document(&directory, "plan.receipt.json");
        if mode == "block" {
            assert_eq!(receipt["entries"][0]["disposition"]["status"], "rejected");
            assert!(receipt["entries"][0].get("target_id").is_none());
            assert!(receipt["entries"][0].get("verification").is_none());
            assert!(
                snapshot(&root.join("target")) == before,
                "Block policy changed target bytes"
            );
        } else {
            assert_eq!(receipt["entries"][0]["verification"]["status"], "verified");
            let actual = native(&directory);
            assert_eq!(actual.len(), 2);
            let managed = actual
                .iter()
                .find(|native| native["id"] != "urn:ump:foreign")
                .unwrap();
            assert!(
                managed["body"]["text"].as_str().unwrap() == body,
                "Pass policy changed the target body"
            );
            assert_eq!(
                actual
                    .iter()
                    .find(|native| native["id"] == "urn:ump:foreign")
                    .unwrap(),
                &foreign()
            );
        }
    }
}

/// Changed approved target bytes cause S7 refusal, preserve the actual user edit, and save approval but no final receipt.
#[test]
fn cli_target_edit_after_plan_is_not_overwritten_or_laundered() {
    let directory = cli_fixture("Approved source body.");
    let root = directory.path();
    write_native(&directory, &[foreign()]);
    assert!(cli_plan(&directory, "plan.json", &[]).status.success());
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    let mut edited = foreign();
    edited["body"]["text"] = json!("User edit after approval.");
    write_native(&directory, &[edited.clone()]);
    let before = snapshot(&root.join("target"));
    let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
    let message = cli_failure(&output, "[S7]", "unused-private-input");
    assert!(message.contains("Plan digest mismatch"));
    assert!(message.contains("before target writes began"));
    assert!(message.contains("approval was saved") && message.contains("do not blindly retry"));
    assert_eq!(snapshot(&root.join("target")), before);
    assert_eq!(native(&directory), vec![edited]);
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    assert_eq!(
        document(&directory, "plan.approval.json")["plan_digest"],
        document(&directory, "plan.json")["plan_digest"]
    );
    assert!(!root.join("plan.receipt.json").exists());
}

/// Actual CLI history tampering on an empty source must fail at approval recomputation and preserve existing receipts.
#[test]
fn cli_empty_source_history_entry_removal_refuses_with_saved_approval_and_no_write() {
    let directory = cli_fixture("An existing CLI memory.");
    let root = directory.path();
    assert!(cli_plan(&directory, "first.json", &[]).status.success());
    assert!(
        cli(&["apply", root.join("first.json").to_str().unwrap(), "--yes"])
            .status
            .success()
    );
    fs::remove_file(root.join("source/note.md")).unwrap();
    let previous = root.join("first.receipt.json");
    assert!(
        cli_plan(
            &directory,
            "empty.json",
            &["--previous-receipt", previous.to_str().unwrap()]
        )
        .status
        .success()
    );
    let first_approval = fs::read(root.join("first.approval.json")).unwrap();
    let first_plan = fs::read(root.join("first.json")).unwrap();
    let mut altered = document(&directory, "first.receipt.json");
    altered["entries"] = json!([]);
    let changed_bytes = serde_json::to_vec(&altered).unwrap();
    fs::write(&previous, &changed_bytes).unwrap();
    let before = snapshot(&root.join("target"));
    let output = cli(&["apply", root.join("empty.json").to_str().unwrap(), "--yes"]);
    let message = cli_failure(&output, "[S7]", "unused-private-input");
    assert!(message.contains("Plan digest mismatch"));
    assert_eq!(snapshot(&root.join("target")), before);
    assert_eq!(fs::read(&previous).unwrap(), changed_bytes);
    assert_eq!(
        fs::read(root.join("first.approval.json")).unwrap(),
        first_approval
    );
    assert_eq!(fs::read(root.join("first.json")).unwrap(), first_plan);
    assert_eq!(
        document(&directory, "empty.approval.json")["plan_digest"],
        document(&directory, "empty.json")["plan_digest"]
    );
    assert!(!root.join("empty.receipt.json").exists());
}

/// A native-file directory is not absence; CLI planning refuses without altering any target entries or credentials.
#[test]
fn cli_nonregular_native_target_refuses_before_plan_and_credentials() {
    let directory = cli_fixture("Never export to a directory artifact.");
    let root = directory.path();
    fs::create_dir_all(root.join("target").join(FILE)).unwrap();
    fs::write(
        root.join("target").join(FILE).join("user.txt"),
        b"Protected nested bytes.",
    )
    .unwrap();
    let before = snapshot(root);
    let output = cli_plan(&directory, "plan.json", &[]);
    let message = cli_failure(&output, "regular file", "unused-private-input");
    assert!(message.contains("target unchanged"));
    assert_eq!(snapshot(root), before);
}

/// Explicit linked roots are rejected before a report can expose a sensitive synthetic path or create credentials.
#[cfg(unix)]
#[test]
fn cli_linked_ump_root_refuses_without_outside_writes_or_credentials() {
    let directory = cli_fixture("Do not follow the target root.");
    let root = directory.path();
    let outside = root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("user.txt"), b"Outside protected bytes.").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("target")).unwrap();
    let before = snapshot(&outside);
    let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    let report = format!("report-{secret}.json");
    let output = cli_plan(&directory, &report, &[]);
    let message = cli_failure(&output, "[S5]", &secret);
    assert!(message.contains("symlink") && message.contains("target unchanged"));
    assert!(
        fs::symlink_metadata(root.join("target"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(snapshot(&outside), before);
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);
    assert!(!root.join(report).exists());
}

/// A CLI-visible linked ancestor after planning is refused before approval and cannot redirect writes to another directory.
#[cfg(unix)]
#[test]
fn cli_approved_ump_ancestor_swap_keeps_both_target_trees_unchanged() {
    let directory = cli_fixture("Keep the approved target fixed.");
    let root = directory.path();
    let parent = root.join("parent");
    let moved = root.join("moved");
    let outside = root.join("outside");
    for path in [&parent, &outside] {
        fs::create_dir_all(path.join("target")).unwrap();
        fs::write(path.join("target/user.txt"), b"Protected target bytes.").unwrap();
    }
    let output = cli(&[
        "plan",
        root.join("source").to_str().unwrap(),
        "--to",
        &format!("ump:{}", parent.join("target").display()),
        "--report",
        root.join("plan.json").to_str().unwrap(),
    ]);
    assert!(output.status.success(), "CLI planning failed");
    let original = snapshot(&parent);
    let external = snapshot(&outside);
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    fs::rename(&parent, &moved).unwrap();
    std::os::unix::fs::symlink(&outside, &parent).unwrap();
    let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
    let message = cli_failure(
        &output,
        "[S7] Approved target path changed",
        "unused-private-input",
    );
    assert!(message.contains("before approval was saved"));
    assert_eq!(snapshot(&moved), original);
    assert_eq!(snapshot(&outside), external);
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    assert!(!root.join("plan.approval.json").exists());
    assert!(!root.join("plan.receipt.json").exists());
}

/// Bounds a genuine CLI FIFO target refusal, killing only this test's child if a blocking read regresses.
#[cfg(unix)]
#[test]
fn cli_fifo_native_target_is_refused_without_opening_or_hanging() {
    use std::os::unix::fs::FileTypeExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let directory = cli_fixture("Do not open a FIFO.");
    let root = directory.path();
    fs::create_dir(root.join("target")).unwrap();
    fs::write(
        root.join("target/user.txt"),
        b"Protected FIFO-neighbor bytes.",
    )
    .unwrap();
    let fifo = root.join("target").join(FILE);
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args([
            "plan",
            root.join("source").to_str().unwrap(),
            "--to",
            &format!("ump:{}", root.join("target").display()),
            "--report",
            root.join("plan.json").to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("CLI opened a FIFO instead of refusing a nonregular target");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    cli_failure(&output, "regular file", "unused-private-input");
    assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo());
    assert_eq!(
        fs::read(root.join("target/user.txt")).unwrap(),
        b"Protected FIFO-neighbor bytes."
    );
    assert_eq!(fs::read_dir(root.join("target")).unwrap().count(), 2);
    assert!(!root.join("plan.json").exists());
    assert!(!root.join("plan.approval.json").exists());
    assert!(!root.join("plan.receipt.json").exists());
}

/// Exercises actual UMP first-persistence permission failure, with explicit evidence if the local user bypasses permission bits.
#[cfg(unix)]
#[test]
fn cli_ump_permission_failure_saves_approval_without_changing_target() {
    use std::os::unix::fs::PermissionsExt;
    let directory = cli_fixture("Cannot persist in a protected directory.");
    let root = directory.path();
    write_native(&directory, &[foreign()]);
    assert!(cli_plan(&directory, "plan.json", &[]).status.success());
    let target_root = root.join("target");
    let before = snapshot(&target_root);
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    let original_permissions = fs::metadata(&target_root).unwrap().permissions();
    fs::set_permissions(&target_root, fs::Permissions::from_mode(0o500)).unwrap();
    let probe = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target_root.join("permission-probe"));
    if probe.is_ok() {
        drop(probe);
        fs::set_permissions(&target_root, original_permissions).unwrap();
        fs::remove_file(target_root.join("permission-probe")).unwrap();
        eprintln!("Skipping permission fault: this user bypasses directory write permissions");
        return;
    }
    assert_eq!(
        probe.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
    fs::set_permissions(&target_root, original_permissions).unwrap();
    let message = cli_failure(&output, "[S8]", "unused-private-input");
    assert!(message.contains("Permission denied"));
    assert!(
        message.contains("targets may be partially changed")
            && message.contains("do not blindly retry")
    );
    assert_eq!(snapshot(&target_root), before);
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    assert_eq!(
        document(&directory, "plan.approval.json")["plan_digest"],
        document(&directory, "plan.json")["plan_digest"]
    );
    assert!(!root.join("plan.receipt.json").exists());
}

/// Actual CLI execution may complete the first approved target before a second target fails; no cross-target rollback is claimed.
#[cfg(unix)]
#[test]
fn cli_two_ump_targets_report_partial_after_first_actual_native_write() {
    use std::os::unix::fs::PermissionsExt;
    let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    let directory = cli_fixture(&format!("Two-target synthetic memory.\n{secret}\n"));
    let root = directory.path();
    let first = root.join("first");
    let second = root.join("second");
    for path in [&first, &second] {
        fs::create_dir(path).unwrap();
        fs::write(path.join("user.txt"), b"Protected target user bytes.").unwrap();
    }
    let mut registry = Registry::default();
    registry.register_reader(MarkdownReader).unwrap();
    registry
        .register_writer("a".into(), UmpWriter::new(first.clone()).unwrap())
        .unwrap();
    registry
        .register_writer("z".into(), UmpWriter::new(second.clone()).unwrap())
        .unwrap();
    let runner = Engine { registry };
    let approved = runner.plan(&root.join("source"), policy()).unwrap();
    write_json_new(&root.join("plan.json"), &approved).unwrap();
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    let before_first = snapshot(&first);
    let before_second = snapshot(&second);
    let permissions = fs::metadata(&second).unwrap().permissions();
    fs::set_permissions(&second, fs::Permissions::from_mode(0o500)).unwrap();
    let probe = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(second.join("permission-probe"));
    if probe.is_ok() {
        drop(probe);
        fs::set_permissions(&second, permissions).unwrap();
        fs::remove_file(second.join("permission-probe")).unwrap();
        eprintln!(
            "Skipping two-target permission fault: this user bypasses directory write permissions"
        );
        return;
    }
    assert_eq!(
        probe.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
    fs::set_permissions(&second, permissions).unwrap();
    let message = cli_failure(&output, "[S8]", &secret);
    assert!(message.contains("Permission denied"));
    for context in [
        "targets may be partially changed",
        "No reliable final receipt",
        "Inspect all targets",
        "do not blindly retry",
    ] {
        assert!(
            message.contains(context),
            "Missing partial-write context: {context}"
        );
    }
    assert_eq!(snapshot(&second), before_second);
    let actual_first = snapshot(&first);
    assert_eq!(actual_first.len(), before_first.len() + 1);
    assert!(actual_first[Path::new("user.txt")] == before_first[Path::new("user.txt")]);
    let actual: Vec<Value> = serde_json::from_slice(&fs::read(first.join(FILE)).unwrap()).unwrap();
    assert_eq!(actual.len(), 1);
    assert!(
        actual[0]["body"]["text"].as_str().unwrap()
            == format!("Two-target synthetic memory.\n{secret}\n"),
        "Partial target content differs from approved input"
    );
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    assert_eq!(
        document(&directory, "plan.approval.json")["plan_digest"],
        document(&directory, "plan.json")["plan_digest"]
    );
    assert!(!root.join("plan.receipt.json").exists());
    for name in ["plan.json", "plan.approval.json"] {
        assert!(
            !String::from_utf8_lossy(&fs::read(root.join(name)).unwrap()).contains(&secret),
            "Credential leaked private input"
        );
    }
}

/// Final receipt save failure follows real verified UMP writes, retains approval, and never labels an unsaved receipt reliable.
#[test]
fn cli_final_receipt_save_failure_keeps_actual_ump_output_and_original_approval() {
    let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    let directory = cli_fixture("An actually written UMP memory.");
    let root = directory.path();
    write_native(&directory, &[foreign()]);
    let obstruction = root.join(format!("obstruction-{secret},"));
    fs::write(&obstruction, b"Protected receipt obstruction.").unwrap();
    assert!(cli_plan(&directory, "plan.json", &[]).status.success());
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    let before = snapshot(&root.join("target"));
    let receipt_path = obstruction.join("receipt.json");
    let output = cli(&[
        "apply",
        root.join("plan.json").to_str().unwrap(),
        "--yes",
        "--receipt",
        receipt_path.to_str().unwrap(),
    ]);
    let message = cli_failure(&output, "[S9] Final receipt save failed", &secret);
    for context in [
        "Not a directory",
        "targets may already have changed",
        "approval was saved",
        "No reliable final receipt",
        "Inspect targets",
        "do not blindly retry",
    ] {
        assert!(
            message.contains(context),
            "Missing final-save context: {context}"
        );
    }
    let after = snapshot(&root.join("target"));
    assert_ne!(after, before);
    assert_eq!(after.len(), before.len());
    let records = native(&directory);
    assert_eq!(records.len(), 2);
    assert_eq!(
        records
            .iter()
            .find(|value| value["id"] == "urn:ump:foreign")
            .unwrap(),
        &foreign()
    );
    assert_eq!(
        records
            .iter()
            .find(|value| value["id"] != "urn:ump:foreign")
            .unwrap()["body"]["text"],
        "An actually written UMP memory."
    );
    assert_eq!(
        fs::read(&obstruction).unwrap(),
        b"Protected receipt obstruction."
    );
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    let approval_bytes = fs::read(root.join("plan.approval.json")).unwrap();
    assert!(
        !String::from_utf8_lossy(&approval_bytes).contains(&secret),
        "Approval leaked private path"
    );
    assert_eq!(
        document(&directory, "plan.approval.json")["plan_digest"],
        document(&directory, "plan.json")["plan_digest"]
    );
    assert!(!receipt_path.exists() && !root.join("plan.receipt.json").exists());
    let second = cli(&[
        "apply",
        root.join("plan.json").to_str().unwrap(),
        "--yes",
        "--receipt",
        receipt_path.to_str().unwrap(),
    ]);
    cli_failure(&second, "Output report already exists", &secret);
    assert_eq!(snapshot(&root.join("target")), after);
    assert_eq!(
        fs::read(root.join("plan.approval.json")).unwrap(),
        approval_bytes
    );
}
