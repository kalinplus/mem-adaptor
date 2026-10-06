//! Exercises core planning and receipt behavior with synthetic Readers and an isolated OKF target.
//! Reader-controlled records expose identity, metadata, governance, and history boundaries without real exports.
//! Assertions prove only their stated scenarios; these implementation-based tests are not independent conformance.

use std::fs;

use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::{Engine, canonical_id, content_hash, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::*;
use mem_adaptor_writer_okf::OkfWriter;
use serde_json::json;
use tempfile::TempDir;

struct SyntheticReader(Vec<CanonicalRecord>);

struct CrossSystemReader;

impl Reader for CrossSystemReader {
    fn id(&self) -> &'static str {
        "synthetic-multi"
    }
    fn version(&self) -> &'static str {
        "test"
    }
    fn claim(&self, files: &FileInventory) -> Vec<Claim> {
        files
            .keys()
            .map(|path| Claim {
                path: path.clone(),
                layer: "memory".into(),
                registered_only: false,
            })
            .collect()
    }
    fn read(&self, claim: &Claim, _: &SourceFs) -> mem_adaptor_core::Result<ReaderOutput> {
        let mut output = mem_adaptor_core::reader::output();
        for (system, fields) in [
            (
                "left",
                json!({"body": "Synthetic", "type": "secretRef", "credential": format!("ghp_TEST{}", "A".repeat(32))}),
            ),
            ("right", json!({"body": "Synthetic"})),
        ] {
            let mut canonical = record("shared-native-id");
            canonical.source.system = system.into();
            canonical.canonical_id = canonical_id(system, &canonical.source_record_id);
            let mut source = mem_adaptor_core::reader::source(&canonical, &claim.path, fields);
            mem_adaptor_core::reader::map(&mut source, "/body", "/content");
            output.source_records.push(source);
            output.records.push(canonical);
        }
        Ok(output)
    }
}

impl Reader for SyntheticReader {
    /// Identifies the test Reader independently of the source systems carried by its records.
    fn id(&self) -> &'static str {
        "synthetic"
    }
    /// Labels fixture output with a synthetic adapter version.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Claims every isolated fixture file as convertible memory without parsing its contents.
    fn claim(&self, files: &FileInventory) -> Vec<Claim> {
        files
            .keys()
            .map(|path| Claim {
                path: path.clone(),
                layer: "memory".into(),
                registered_only: false,
            })
            .collect()
    }
    /// Returns supplied records unchanged so tests can inject metadata changes or invalid Reader claims.
    /// Pairs each record with source fields and deliberately leaves one field unreported for engine auditing.
    fn read(&self, claim: &Claim, _: &SourceFs) -> mem_adaptor_core::Result<ReaderOutput> {
        Ok(ReaderOutput {
            source_records: self.0.iter().map(|record| SourceRecord {
                canonical_id: record.canonical_id.clone(),
                source_record_id: record.source_record_id.clone(), source_locator: claim.path.clone(),
                fields: json!({"body": record.content, "unreported": "Synthetic unknown field"}),
                field_map: vec![FieldMapping { source_path: "/body".into(), canonical_path: "/content".into(), rule: None }],
                unmapped: vec![],
            }).collect(),
            records: self.0.clone(),
            anomalies: vec![], registered_count: 0, deleted_count: 0, source_unavailable: vec![],
        })
    }
}

/// Builds a minimal synthetic record with freshly derived identity and body hash, not vector placeholder hashes.
fn record(id: &str) -> CanonicalRecord {
    let vector: serde_json::Value = serde_json::from_str(include_str!(
        "../../../schema/vectors/valid/canonical-minimal.json"
    ))
    .unwrap();
    let mut record: CanonicalRecord = serde_json::from_value(vector["document"].clone()).unwrap();
    record.source.system = "synthetic".into();
    record.source_record_id = id.into();
    record.source_locator = "fixture.json".into();
    record.canonical_id = canonical_id("synthetic", id);
    record.content_hash = content_hash(record.content.as_bytes());
    record
}

/// Creates an isolated source file; the target stays absent until a test explicitly applies a plan.
fn fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(directory.path().join("source/fixture.json"), "{}").unwrap();
    directory
}

/// Registers supplied synthetic records and an OKF Writer confined to the fixture's target path.
fn engine(directory: &TempDir, records: Vec<CanonicalRecord>) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(SyntheticReader(records)).unwrap();
    registry
        .register_writer(
            "home".into(),
            OkfWriter::new(directory.path().join("target")),
        )
        .unwrap();
    Engine { registry }
}

/// Builds default pass-policy settings without claiming user selection or disabling secret detection.
fn policy() -> GatePolicy {
    GatePolicy {
        secrets: GateAction::Pass,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}

/// Applies an already planned fixture using a matching synthetic approval and returns its receipt.
/// This helper writes the temporary target; its report references are labels, not saved report files.
fn apply(engine: &Engine, report: &PlanReport) -> ReceiptReport {
    engine
        .apply(
            report,
            &ApprovalReceipt {
                schema_version: "0.1.0".into(),
                receipt_id: "synthetic-approval".into(),
                plan_digest: report.plan_digest.clone(),
                approved_at: timestamp().unwrap(),
                backend: "local".into(),
                approver: "synthetic-user".into(),
            },
            "plan.json".into(),
            "approval.json".into(),
        )
        .unwrap()
}

/// Checks omitted-field reporting and that metadata-only changes invalidate approval despite unchanged body hashes.
/// The failed apply leaves the fixture target absent; the test does not establish general rollback guarantees.
#[test]
fn engine_exposes_reader_omissions_and_binds_metadata_to_approval() {
    let directory = fixture();
    let original = record("example");
    let first = engine(&directory, vec![original.clone()])
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert!(
        first.entries[0]
            .unmapped
            .iter()
            .any(|field| field.reason == UnmappedReason::EngineUnreported
                && field.source_path == "/unreported")
    );
    assert!(
        first
            .anomalies
            .iter()
            .any(|anomaly| anomaly.code == "reader_unreported_field")
    );
    let mut changed = original;
    changed.scope = Scope::Project;
    changed.scope_qualifier = Some("synthetic-project".into());
    let changed_engine = engine(&directory, vec![changed]);
    let second = changed_engine
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert_eq!(first.bundle_manifest, second.bundle_manifest);
    assert_eq!(
        first.digest_inputs.records[0].content_hash,
        second.digest_inputs.records[0].content_hash
    );
    assert_ne!(first.plan_digest, second.plan_digest);
    let approval = ApprovalReceipt {
        schema_version: "0.1.0".into(),
        receipt_id: "synthetic".into(),
        plan_digest: first.plan_digest.clone(),
        approved_at: timestamp().unwrap(),
        backend: "local".into(),
        approver: "synthetic".into(),
    };
    assert!(
        changed_engine
            .apply(
                &first,
                &approval,
                "plan.json".into(),
                "approval.json".into()
            )
            .is_err()
    );
    assert!(!directory.path().join("target").exists());
}

#[test]
fn native_id_collisions_do_not_cross_contaminate_findings_references_or_mappings() {
    let directory = fixture();
    let mut registry = Registry::default();
    registry.register_reader(CrossSystemReader).unwrap();
    registry
        .register_writer(
            "home".into(),
            OkfWriter::new(directory.path().join("target")),
        )
        .unwrap();
    let report = Engine { registry }
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert_eq!(report.source.system, "mixed");
    let left = report
        .entries
        .iter()
        .find(|entry| entry.canonical_id == canonical_id("left", "shared-native-id"))
        .unwrap();
    let right = report
        .entries
        .iter()
        .find(|entry| entry.canonical_id == canonical_id("right", "shared-native-id"))
        .unwrap();
    assert_eq!(
        left.disposition,
        Disposition::Omitted {
            reason: OmissionReason::SecretReferenceUnsupported
        }
    );
    assert!(!left.sensitive_findings.is_empty());
    assert_eq!(right.disposition, Disposition::Accepted);
    assert!(right.sensitive_findings.is_empty());
    assert!(right.unmapped.is_empty());
}

#[test]
fn identical_text_in_distinct_scopes_or_with_distinct_consent_is_not_deduplicated() {
    let directory = fixture();
    let first = record("first");
    let mut second = record("second");
    second.scope = Scope::Project;
    second.scope_qualifier = Some("project".into());
    let mut third = record("third");
    third.consent = Some(Consent {
        exportable: Some(true),
        memory_enabled: Some(true),
        retention: Some("P30D".into()),
        redact: None,
    });
    let report = engine(&directory, vec![first, second, third])
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert!(
        report
            .entries
            .iter()
            .all(|entry| entry.disposition == Disposition::Accepted)
    );
}

#[test]
fn prior_verdict_is_reused_but_does_not_override_secret_blocking() {
    let directory = fixture();
    let mut original = record("example");
    let cluster = content_hash(b"synthetic cluster");
    original.conflict_cluster_id = Some(cluster.clone());
    let engine = engine(&directory, vec![original.clone()]);
    let plan = engine
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    let mut receipt = apply(&engine, &plan);
    receipt.verdicts = vec![Verdict::Keep {
        cluster_id: cluster.clone(),
        canonical_ids: vec![canonical_id("synthetic", "other")],
    }];
    let previous = directory.path().join("previous.json");
    write_json_new(&previous, &receipt).unwrap();
    let report = engine
        .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
        .unwrap();
    assert_eq!(
        report.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::VerdictExcluded {
                cluster_id: cluster
            }
        }
    );
    assert_eq!(apply(&engine, &report).verdicts, receipt.verdicts);
    original.content = format!("ghp_TEST{}", "A".repeat(32));
    original.content_hash = content_hash(original.content.as_bytes());
    let mut blocked = policy();
    blocked.secrets = GateAction::Block;
    let mut registry = Registry::default();
    registry
        .register_reader(SyntheticReader(vec![original]))
        .unwrap();
    registry
        .register_writer(
            "home".into(),
            OkfWriter::new(directory.path().join("target")),
        )
        .unwrap();
    let report = Engine { registry }
        .plan_with_previous(&directory.path().join("source"), blocked, Some(&previous))
        .unwrap();
    assert!(matches!(
        report.entries[0].disposition,
        Disposition::Rejected { .. }
    ));
}

#[test]
fn unverifiable_previous_writes_require_manual_target_confirmation() {
    let directory = fixture();
    let engine = engine(&directory, vec![record("example")]);
    let plan = engine
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    let mut receipt = apply(&engine, &plan);
    let verification = Verification::Unverifiable {
        why: "Synthetic no-read-back target".into(),
    };
    receipt.entries[0].verification = Some(verification.clone());
    receipt.entries[0]
        .prior_write
        .as_mut()
        .unwrap()
        .verification = verification;
    let previous = directory.path().join("previous.json");
    write_json_new(&previous, &receipt).unwrap();
    let report = engine
        .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
        .unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("confirm target state manually"))
    );
    assert!(matches!(
        report.entries[0].disposition,
        Disposition::Unresolved { .. }
    ));
}

#[test]
fn metadata_updates_are_written_and_consent_disables_export() {
    let directory = fixture();
    let original = record("example");
    let first_engine = engine(&directory, vec![original.clone()]);
    let plan = first_engine
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    let receipt = apply(&first_engine, &plan);
    let previous = directory.path().join("previous.json");
    write_json_new(&previous, &receipt).unwrap();
    let mut changed = original.clone();
    changed.scope = Scope::Project;
    changed.scope_qualifier = Some("updated-project".into());
    let changed_engine = engine(&directory, vec![changed]);
    let plan = changed_engine
        .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
        .unwrap();
    assert_eq!(plan.entries[0].disposition, Disposition::Accepted);
    assert_eq!(
        apply(&changed_engine, &plan).entries[0].verification,
        Some(Verification::Verified)
    );
    let actual = changed_engine
        .registry
        .writer("home")
        .unwrap()
        .inspect(&receipt.entries[0].target_id.clone().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(actual.scope, Scope::Project);
    for (exportable, memory_enabled, expected) in [
        (
            Some(false),
            None,
            Disposition::Rejected {
                rule: "consent_export_disabled".into(),
            },
        ),
        (
            None,
            Some(false),
            Disposition::Unresolved {
                reason: UnresolvedReason::MemoryDisabled,
            },
        ),
    ] {
        let mut record = original.clone();
        record.consent = Some(Consent {
            exportable,
            memory_enabled,
            retention: None,
            redact: None,
        });
        let report = engine(&directory, vec![record])
            .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
            .unwrap();
        assert_eq!(report.entries[0].disposition, expected);
    }
}

/// Checks that a forged canonical ID or inconsistent vector dimension stops planning before target creation.
#[test]
fn forged_identity_or_wrong_embedding_dimension_fails_before_writes() {
    for corrupt_identity in [true, false] {
        let directory = fixture();
        let mut record = record("example");
        if corrupt_identity {
            record.canonical_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        } else {
            record.embedding = Some(Embedding {
                model: "synthetic-local".into(),
                dim: 3,
                vector: Some(vec![0.0, 1.0]),
                normalized: None,
            });
        }
        assert!(
            engine(&directory, vec![record])
                .plan(&directory.path().join("source"), policy())
                .is_err()
        );
        assert!(!directory.path().join("target").exists());
    }
}

#[test]
fn duplicate_alias_with_changed_metadata_gets_its_own_record() {
    let directory = fixture();
    let first = record("first");
    let second = record("second");
    let initial = engine(&directory, vec![first.clone(), second.clone()]);
    let report = initial
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    let receipt = apply(&initial, &report);
    let alias = receipt
        .entries
        .iter()
        .find(|entry| {
            matches!(
                entry.disposition,
                Disposition::Omitted {
                    reason: OmissionReason::DuplicateOf { .. }
                }
            )
        })
        .unwrap()
        .canonical_id
        .clone();
    let previous = directory.path().join("previous.json");
    write_json_new(&previous, &receipt).unwrap();
    let mut records = vec![first, second];
    records
        .iter_mut()
        .find(|record| record.canonical_id == alias)
        .unwrap()
        .scope = Scope::Project;
    let changed = engine(&directory, records);
    let report = changed
        .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
        .unwrap();
    assert_eq!(
        report
            .entries
            .iter()
            .find(|entry| entry.canonical_id == alias)
            .unwrap()
            .disposition,
        Disposition::Accepted
    );
    let receipt = apply(&changed, &report);
    assert_eq!(
        receipt
            .entries
            .iter()
            .filter(|entry| entry.verification == Some(Verification::Verified))
            .count(),
        1
    );
    assert_eq!(
        fs::read_dir(directory.path().join("target/memories"))
            .unwrap()
            .count(),
        2
    );
}
