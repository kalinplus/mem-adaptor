//! Exercises core planning and receipt behavior with synthetic Readers and an isolated OKF target.
//! Reader-controlled records expose identity, metadata, governance, and history boundaries without real exports.
//! A closing section covers DEC-20 satellite identity: three-segment canonical IDs, satellite-prefixed
//! scope qualifiers, and receipt attribution by satellite ID (home mode) versus source path (direct mode).
//! Assertions prove only their stated scenarios; these implementation-based tests are not independent conformance.
//! Sections group validation, field coverage, deduplication, and governance/history scenarios for reading, not execution order.

use std::fs;
use std::path::Path;

use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::{Engine, canonical_id, content_hash, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::*;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use serde_json::json;
use tempfile::TempDir;

struct SyntheticReader(Vec<CanonicalRecord>);

struct CrossSystemReader;

// Test support: controlled Reader outputs and a temporary approved-write environment.

impl Reader for CrossSystemReader {
    /// Registers one fixture adapter whose output deliberately spans two different source-system identities.
    fn id(&self) -> &'static str {
        "synthetic-multi"
    }
    /// Labels this multi-system fixture adapter rather than presenting a real export version.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Fixture sources are directories held in place for the run, so their path may bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
    }
    /// Claims each isolated fixture file so the engine receives both systems through one Reader call.
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
    /// Emits colliding native IDs under distinct canonical identities, with findings/reference fields only on the left.
    /// Intentionally leaves source-field coverage incomplete so the engine must associate evidence by canonical ID.
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
            canonical.canonical_id = canonical_id(system, "", &canonical.source_record_id);
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
    /// Fixture sources are directories held in place for the run, so their path may bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
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
    record.canonical_id = canonical_id("synthetic", "", id);
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
            OkfWriter::new(directory.path().join("target")).unwrap(),
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

// Record validation and coverage: Reader claims are checked before any target write.

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
        let error = engine(&directory, vec![record])
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        assert!(error.to_string().contains(if corrupt_identity {
            "Reader canonical identity mismatch"
        } else {
            "Embedding vector length does not match dimension"
        }));
        assert!(!directory.path().join("target").exists());
    }
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
    let error = changed_engine
        .apply(
            &first,
            &approval,
            "plan.json".into(),
            "approval.json".into(),
        )
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("[S7]") && message.contains("Plan digest mismatch"));
    assert!(!directory.path().join("target").exists());
}

/// Checks equal native IDs in different systems do not mix secret-reference dispositions, findings, or field coverage.
#[test]
fn native_id_collisions_do_not_cross_contaminate_findings_references_or_mappings() {
    let directory = fixture();
    let mut registry = Registry::default();
    registry.register_reader(CrossSystemReader).unwrap();
    registry
        .register_writer(
            "home".into(),
            OkfWriter::new(directory.path().join("target")).unwrap(),
        )
        .unwrap();
    let report = Engine { registry }
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert_eq!(report.source.system, "mixed");
    let left = report
        .entries
        .iter()
        .find(|entry| entry.canonical_id == canonical_id("left", "", "shared-native-id"))
        .unwrap();
    let right = report
        .entries
        .iter()
        .find(|entry| entry.canonical_id == canonical_id("right", "", "shared-native-id"))
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

// Metadata-aware deduplication: equal body text does not make distinct semantics interchangeable.

/// Checks identical body text remains separate when scope or consent changes its meaning and permitted handling.
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

/// Checks a duplicate alias whose scope changes gains its own verified write instead of borrowing representative history.
/// The native target must then contain both records.
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

// Governance and prior-write evidence: preserved declarations and history are not unconditional permission.

/// Checks a saved human verdict is reused and carried forward, but cannot take precedence over secret blocking.
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
        canonical_ids: vec![canonical_id("synthetic", "", "other")],
        bases: None,
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
            OkfWriter::new(directory.path().join("target")).unwrap(),
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

/// Checks unverifiable historical writes produce a manual-confirmation warning and unresolved disposition, not overwrite approval.
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

/// Checks approved scope changes reach the native target with verified read-back.
/// Later export-disabled or memory-disabled declarations are rejected/unresolved during planning, not silently exported.
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

// Satellite identity (DEC-20): home-mode identity, qualifiers, and receipt chains anchor to the satellite ID.

/// Relative note path shared by every synthetic vault; two vaults must not derive the same record identity.
const VAULT_NOTE: &str = "projects/demo/memory/note.md";

/// Claude Code-style memory document; its frontmatter metadata selects the claude_code source system.
const VAULT_TEXT: &str =
    "---\nmetadata:\n  node_type: memory\n  type: feedback\n---\nSynthetic vault body.\n";

/// Creates an isolated satellite vault at a fresh temporary path; the home target stays caller-controlled.
fn vault() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    let memory = directory.path().join("source/projects/demo/memory");
    fs::create_dir_all(&memory).unwrap();
    fs::write(memory.join("note.md"), VAULT_TEXT).unwrap();
    directory
}

/// Registers the Markdown Reader with an OKF Writer confined to the caller-supplied home target path.
/// Separate vaults sharing one home mirror real home mode: satellites move, the home does not.
fn vault_engine(home: &Path) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(MarkdownReader).unwrap();
    registry
        .register_writer("home".into(), OkfWriter::new(home.to_path_buf()).unwrap())
        .unwrap();
    Engine { registry }
}

/// Checks DEC-20 R4/R8: the same relative note path under two satellites yields different canonical IDs,
/// satellite-prefixed scope qualifiers, and satellite-bound plan reports; direct mode differs from both.
#[test]
fn satellite_identity_anchors_ids_qualifiers_and_plan_reports() {
    let alpha = SatelliteSpec {
        id: "aaaaaaa2".into(),
        label: Some("Alpha vault".into()),
    };
    let beta = SatelliteSpec {
        id: "aaaaaaab".into(),
        label: None,
    };
    let first = vault();
    let second = vault();
    let first_plan = vault_engine(&first.path().join("target"))
        .plan_with_satellite(&first.path().join("source"), policy(), &alpha, None, None)
        .unwrap();
    let second_plan = vault_engine(&second.path().join("target"))
        .plan_with_satellite(&second.path().join("source"), policy(), &beta, None, None)
        .unwrap();
    let direct = vault_engine(&first.path().join("target"))
        .plan(&first.path().join("source"), policy())
        .unwrap();
    let expected = |satellite: &str| canonical_id("claude_code", satellite, VAULT_NOTE);
    assert_eq!(first_plan.entries[0].canonical_id, expected("aaaaaaa2"));
    assert_eq!(second_plan.entries[0].canonical_id, expected("aaaaaaab"));
    assert_eq!(direct.entries[0].canonical_id, expected(""));
    assert_ne!(
        first_plan.entries[0].canonical_id,
        second_plan.entries[0].canonical_id
    );
    assert_eq!(first_plan.source.satellite, Some(alpha.clone()));
    assert_eq!(second_plan.source.satellite, Some(beta));
    assert!(direct.source.satellite.is_none());
    assert_ne!(first_plan.plan_digest, second_plan.plan_digest);
    assert_ne!(first_plan.plan_digest, direct.plan_digest);
    // R8: the home-mode qualifier carries the satellite prefix over the relative project slug;
    // direct mode keeps the plain slug. Both are reader-level facts, checked without an engine run.
    let qualifier = |satellite_id: Option<&str>| {
        let source = SourceFs {
            root: "/synthetic/vault".into(),
            files: [(VAULT_NOTE.to_owned(), VAULT_TEXT.as_bytes().to_vec())]
                .into_iter()
                .collect(),
            satellite_id: satellite_id.map(str::to_owned),
        };
        let claim = Claim {
            path: VAULT_NOTE.into(),
            layer: "auto_memory".into(),
            registered_only: false,
        };
        MarkdownReader
            .read(&claim, &source)
            .unwrap()
            .records
            .remove(0)
            .scope_qualifier
    };
    assert_eq!(
        qualifier(Some("aaaaaaa2")).as_deref(),
        Some("aaaaaaa2/demo")
    );
    assert_eq!(qualifier(None).as_deref(), Some("demo"));
}

/// Checks DEC-20 R7: home-mode receipt ownership binds to the satellite ID, so moving the vault keeps the
/// history chain, while another satellite or a direct-mode receipt is rejected. Direct-mode receipts keep
/// the old path binding: rejected at a moved path, accepted at the same path, and never valid for home mode.
#[test]
fn receipts_bind_to_satellite_id_in_home_mode_and_to_location_in_direct_mode() {
    let alpha = SatelliteSpec {
        id: "aaaaaaa2".into(),
        label: Some("Alpha vault".into()),
    };
    let beta = SatelliteSpec {
        id: "aaaaaaab".into(),
        label: None,
    };
    let home = tempfile::tempdir().unwrap();
    let original = vault();
    let engine = vault_engine(home.path());
    let plan = engine
        .plan_with_satellite(
            &original.path().join("source"),
            policy(),
            &alpha,
            None,
            None,
        )
        .unwrap();
    let receipt = apply(&engine, &plan);
    assert_eq!(receipt.source.satellite, Some(alpha.clone()));
    assert_eq!(
        receipt.entries[0].verification,
        Some(Verification::Verified)
    );
    let previous = original.path().join("previous.json");
    write_json_new(&previous, &receipt).unwrap();

    // The same satellite at a new source path continues the chain against the unchanged home.
    let moved = vault();
    let moved_engine = vault_engine(home.path());
    let moved_plan = moved_engine
        .plan_with_satellite(
            &moved.path().join("source"),
            policy(),
            &alpha,
            Some(&previous),
            None,
        )
        .unwrap();
    assert_eq!(
        moved_plan.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
    assert!(!moved.path().join("target").exists());

    // Another satellite must not consume the receipt, even with identical source contents.
    let error = moved_engine
        .plan_with_satellite(
            &moved.path().join("source"),
            policy(),
            &beta,
            Some(&previous),
            None,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Previous receipt belongs to another satellite")
    );

    // A direct-mode receipt never satisfies a satellite run, and binds by location in direct runs.
    let direct_directory = vault();
    let direct_engine = vault_engine(&direct_directory.path().join("target"));
    let direct_plan = direct_engine
        .plan(&direct_directory.path().join("source"), policy())
        .unwrap();
    let direct_receipt = apply(&direct_engine, &direct_plan);
    assert!(direct_receipt.source.satellite.is_none());
    let direct_previous = direct_directory.path().join("direct.json");
    write_json_new(&direct_previous, &direct_receipt).unwrap();
    let error = moved_engine
        .plan_with_satellite(
            &moved.path().join("source"),
            policy(),
            &alpha,
            Some(&direct_previous),
            None,
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Previous receipt belongs to another satellite")
    );
    let error = vault_engine(&moved.path().join("target"))
        .plan_with_previous(
            &moved.path().join("source"),
            policy(),
            Some(&direct_previous),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Previous receipt belongs to another source")
    );
    let reused = vault_engine(&direct_directory.path().join("target"))
        .plan_with_previous(
            &direct_directory.path().join("source"),
            policy(),
            Some(&direct_previous),
        )
        .unwrap();
    assert_eq!(
        reused.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
}

/// Checks malformed satellite identities are rejected before inventory or planning; no target is created.
#[test]
fn malformed_satellite_specs_reject_before_any_writes() {
    let directory = vault();
    for satellite in [
        SatelliteSpec {
            id: "AAAAAAA2".into(),
            label: None,
        },
        SatelliteSpec {
            id: "aaaaaa12".into(),
            label: None,
        },
        SatelliteSpec {
            id: "aaaaaaa".into(),
            label: None,
        },
        SatelliteSpec {
            id: "aaaaaaa23".into(),
            label: None,
        },
        SatelliteSpec {
            id: "aaaaaaa2".into(),
            label: Some("".into()),
        },
    ] {
        let error = vault_engine(&directory.path().join("target"))
            .plan_with_satellite(
                &directory.path().join("source"),
                policy(),
                &satellite,
                None,
                None,
            )
            .unwrap_err();
        assert!(error.to_string().contains("Satellite"));
        assert!(!directory.path().join("target").exists());
    }
}

/// Recursively captures every regular file's relative path and bytes under a root for fault assertions.
fn target_snapshot(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                directories.push(path);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    files
}

/// Checks a satellite receipt never serves as direct-mode history, even at the same source location:
/// the identity regimes differ, so planning stops before any write and the home keeps its exact bytes.
#[test]
fn direct_runs_reject_satellite_receipts_as_previous_history() {
    let alpha = SatelliteSpec {
        id: "aaaaaaa2".into(),
        label: Some("Alpha vault".into()),
    };
    let directory = vault();
    let home = directory.path().join("target");
    let engine = vault_engine(&home);
    let plan = engine
        .plan_with_satellite(
            &directory.path().join("source"),
            policy(),
            &alpha,
            None,
            None,
        )
        .unwrap();
    let receipt = apply(&engine, &plan);
    assert_eq!(receipt.source.satellite, Some(alpha));
    let previous = directory.path().join("previous.json");
    write_json_new(&previous, &receipt).unwrap();
    let before = target_snapshot(&home);
    let error = vault_engine(&home)
        .plan_with_previous(&directory.path().join("source"), policy(), Some(&previous))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Previous receipt belongs to a satellite run")
    );
    // The rejection happened before planning completed: no target byte changed and the rejected
    // run produced no report artifacts of its own; the satellite receipt stays untouched.
    assert_eq!(target_snapshot(&home), before);
    let stored: ReceiptReport = serde_json::from_slice(&fs::read(&previous).unwrap()).unwrap();
    assert_eq!(stored, receipt);
}

/// Checks the #19 R4 regression end to end: after satellite alpha populates the shared home, satellite
/// beta plans and applies against the same home without history and is accepted under its own identity
/// — no target_untracked, no "another source" rejection. Alpha's memory file stays byte-identical, beta
/// adds its own file, each receipt binds its own satellite, and beta's rerun with its own receipt
/// continues its own chain. Shared-artifact (index/log) reconciliation is Issue #33, not asserted here.
#[test]
fn second_satellite_merges_into_shared_home_without_false_rejection() {
    let alpha = SatelliteSpec {
        id: "aaaaaaa2".into(),
        label: Some("Alpha vault".into()),
    };
    let beta = SatelliteSpec {
        id: "aaaaaaab".into(),
        label: Some("Beta vault".into()),
    };
    let home = tempfile::tempdir().unwrap();
    let alpha_vault = vault();
    let alpha_engine = vault_engine(home.path());
    let alpha_plan = alpha_engine
        .plan_with_satellite(
            &alpha_vault.path().join("source"),
            policy(),
            &alpha,
            None,
            None,
        )
        .unwrap();
    let alpha_receipt = apply(&alpha_engine, &alpha_plan);
    assert_eq!(alpha_receipt.source.satellite, Some(alpha));
    assert_eq!(
        alpha_receipt.entries[0].verification,
        Some(Verification::Verified)
    );

    // The second satellite has the same relative note path but derives its own identity, so the
    // alpha-populated home neither shadows it (target_untracked) nor rejects the run.
    let beta_vault = vault();
    let alpha_file = home.path().join(format!(
        "memories/{}.md",
        canonical_id("claude_code", "aaaaaaa2", VAULT_NOTE)
    ));
    let alpha_bytes = fs::read(&alpha_file).unwrap();
    let beta_engine = vault_engine(home.path());
    let beta_plan = beta_engine
        .plan_with_satellite(
            &beta_vault.path().join("source"),
            policy(),
            &beta,
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        beta_plan.entries[0].canonical_id,
        canonical_id("claude_code", "aaaaaaab", VAULT_NOTE)
    );
    assert_eq!(beta_plan.entries[0].disposition, Disposition::Accepted);
    let beta_receipt = apply(&beta_engine, &beta_plan);
    assert_eq!(beta_receipt.source.satellite, Some(beta.clone()));
    assert_eq!(
        beta_receipt.entries[0].verification,
        Some(Verification::Verified)
    );
    assert_eq!(fs::read(&alpha_file).unwrap(), alpha_bytes);
    let beta_file = home.path().join(format!(
        "memories/{}.md",
        canonical_id("claude_code", "aaaaaaab", VAULT_NOTE)
    ));
    assert!(beta_file.exists());

    // Beta's next run consumes beta's own receipt and rewrites nothing in the shared home.
    let beta_previous = beta_vault.path().join("beta-receipt.json");
    write_json_new(&beta_previous, &beta_receipt).unwrap();
    let before = target_snapshot(home.path());
    let rerun = vault_engine(home.path())
        .plan_with_satellite(
            &beta_vault.path().join("source"),
            policy(),
            &beta,
            Some(&beta_previous),
            None,
        )
        .unwrap();
    assert_eq!(
        rerun.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
    assert_eq!(target_snapshot(home.path()), before);
}

/// Checks #33 (DEC-19 shared products): after beta's verified write advances the shared index/log, alpha
/// re-plans against the same home. With beta's latest verified receipt as the shared basis alpha
/// reconciles against the real disk state and continues its chain; without one the single-chain fallback
/// mistakes beta's write for tampering. A user edit of a shared artifact refuses with the same basis, and
/// a rollback to an older snapshot never passes while the basis is the latest verified receipt — the
/// fallback alone cannot see that rollback, which is why the CLI supplies a basis in home mode.
#[test]
fn shared_basis_reconciles_cross_satellite_writes_and_refuses_edits_and_rollbacks() {
    let alpha = SatelliteSpec {
        id: "aaaaaaa2".into(),
        label: Some("Alpha vault".into()),
    };
    let beta = SatelliteSpec {
        id: "aaaaaaab".into(),
        label: Some("Beta vault".into()),
    };
    let home = tempfile::tempdir().unwrap();
    let alpha_engine = vault_engine(home.path());
    let alpha_vault = vault();
    let alpha_plan = alpha_engine
        .plan_with_satellite(
            &alpha_vault.path().join("source"),
            policy(),
            &alpha,
            None,
            None,
        )
        .unwrap();
    let alpha_receipt = apply(&alpha_engine, &alpha_plan);
    let index_after_alpha = fs::read(home.path().join("index.md")).unwrap();
    let log_after_alpha = fs::read(home.path().join("log.md")).unwrap();

    let beta_vault = vault();
    let beta_engine = vault_engine(home.path());
    let beta_plan = beta_engine
        .plan_with_satellite(
            &beta_vault.path().join("source"),
            policy(),
            &beta,
            None,
            None,
        )
        .unwrap();
    let beta_receipt = apply(&beta_engine, &beta_plan);

    let history = tempfile::tempdir().unwrap();
    let alpha_previous = history.path().join("alpha.json");
    write_json_new(&alpha_previous, &alpha_receipt).unwrap();
    let beta_basis = history.path().join("beta.json");
    write_json_new(&beta_basis, &beta_receipt).unwrap();

    // Without a basis the single-chain check refuses what beta legitimately wrote.
    let engine = vault_engine(home.path());
    let source = alpha_vault.path().join("source");
    let fallback = engine
        .plan_with_satellite(&source, policy(), &alpha, Some(&alpha_previous), None)
        .unwrap();
    assert!(fallback.entries.iter().all(|entry| entry.disposition
        == Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified,
        }));

    // With the latest verified receipt as the basis, alpha's chain continues over beta's advance.
    let reconciled = engine
        .plan_with_satellite(
            &source,
            policy(),
            &alpha,
            Some(&alpha_previous),
            Some(&beta_basis),
        )
        .unwrap();
    assert_eq!(
        reconciled.shared_basis_ref.as_deref(),
        Some(beta_basis.to_str().unwrap())
    );
    assert!(reconciled.digest_inputs.shared_basis_hash.is_some());
    assert!(reconciled.entries.iter().all(|entry| entry.disposition
        == Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated,
        }));
    // The all-omitted apply writes nothing, so the shared state still equals the basis afterwards.
    let reconciled_receipt = apply(&engine, &reconciled);
    assert_eq!(reconciled_receipt.source.satellite, Some(alpha.clone()));
    // A run with no verified entry carries the previous shared artifacts forward instead of claiming a
    // fresh projection, so it can never be mistaken for a shared-state advance.
    let carried = |receipt: &ReceiptReport| {
        receipt
            .targets
            .iter()
            .flat_map(|target| &target.artifacts)
            .find(|artifact| artifact.path == "index.md")
            .unwrap()
            .content_hash
            .clone()
    };
    assert_eq!(carried(&reconciled_receipt), carried(&alpha_receipt));

    // A basis that fails qualification is refused: wrong location, or no verified entry for the target.
    let misplaced = history.path().join("misplaced.json");
    let mut edited_receipt = serde_json::to_value(&beta_receipt).unwrap();
    edited_receipt["targets"][0]["location"] = json!("/somewhere/else");
    write_json_new(&misplaced, &edited_receipt).unwrap();
    let error = engine
        .plan_with_satellite(
            &source,
            policy(),
            &alpha,
            Some(&alpha_previous),
            Some(&misplaced),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("points at another location"),
        "{error}"
    );
    let unverified = history.path().join("unverified.json");
    let mut edited_receipt = serde_json::to_value(&beta_receipt).unwrap();
    for entry in edited_receipt["entries"].as_array_mut().unwrap() {
        for field in [
            "verification",
            "target_id",
            "prior_write",
            "duplicate_write",
        ] {
            entry.as_object_mut().unwrap().remove(field);
        }
        entry["disposition"] = json!({
            "status": "omitted",
            "reason": {"code": "already_migrated"}
        });
    }
    write_json_new(&unverified, &edited_receipt).unwrap();
    let error = engine
        .plan_with_satellite(
            &source,
            policy(),
            &alpha,
            Some(&alpha_previous),
            Some(&unverified),
        )
        .unwrap_err();
    assert!(error.to_string().contains("no verified entry"), "{error}");

    // The basis file is bound into the digest: changing it after approval refuses the apply.
    let approved_basis = engine
        .plan_with_satellite(
            &source,
            policy(),
            &alpha,
            Some(&alpha_previous),
            Some(&beta_basis),
        )
        .unwrap();
    fs::write(
        &beta_basis,
        format!("{} ", fs::read_to_string(&beta_basis).unwrap()),
    )
    .unwrap();
    let error = engine
        .apply(
            &approved_basis,
            &ApprovalReceipt {
                schema_version: "0.1.0".into(),
                receipt_id: "synthetic-approval".into(),
                plan_digest: approved_basis.plan_digest.clone(),
                approved_at: timestamp().unwrap(),
                backend: "local".into(),
                approver: "synthetic-user".into(),
            },
            "plan.json".into(),
            "approval.json".into(),
        )
        .unwrap_err();
    let chained = format!("{error:#}");
    assert!(chained.contains("Plan digest mismatch"), "{chained}");

    // A user edit of a shared artifact refuses under the same basis instead of being overwritten.
    let index = home.path().join("index.md");
    let owned = fs::read_to_string(&index).unwrap();
    fs::write(&index, format!("{owned}\nuser edit\n")).unwrap();
    let edited = engine
        .plan_with_satellite(
            &source,
            policy(),
            &alpha,
            Some(&alpha_previous),
            Some(&beta_basis),
        )
        .unwrap();
    assert!(edited.entries.iter().all(|entry| entry.disposition
        == Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified,
        }));
    // The plan must say why: a shared-artifact divergence is otherwise indistinguishable from an edited
    // memory record, and the user needs the basis to know what to restore.
    assert!(
        edited.warnings.iter().any(|warning| {
            warning.contains("shared artifacts")
                && warning.contains("index.md")
                && warning.contains(beta_basis.to_str().unwrap())
        }),
        "{:?}",
        edited.warnings
    );

    // Rolling the index back to the alpha-era snapshot matches the older alpha receipt, not the latest
    // verified basis: refused. The basis-less fallback alone cannot distinguish that rollback, which is
    // exactly why home mode always supplies the latest verified basis when one exists.
    fs::write(&index, &index_after_alpha).unwrap();
    fs::write(home.path().join("log.md"), &log_after_alpha).unwrap();
    let rolled_back = engine
        .plan_with_satellite(
            &source,
            policy(),
            &alpha,
            Some(&alpha_previous),
            Some(&beta_basis),
        )
        .unwrap();
    assert!(rolled_back.entries.iter().all(|entry| entry.disposition
        == Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified,
        }));
    let blind_fallback = engine
        .plan_with_satellite(&source, policy(), &alpha, Some(&alpha_previous), None)
        .unwrap();
    assert!(blind_fallback.entries.iter().all(|entry| entry.disposition
        == Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated,
        }));
}
