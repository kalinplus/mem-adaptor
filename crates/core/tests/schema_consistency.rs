//! Checks the hand-authored internal schemas against synthetic vectors and Rust serialization.
//! Exercises required-field rejection and optional-field preservation, not migration or target side effects.
//! These implementation-linked tests are not the independent conformance runner; placeholder hashes prove shape only.
//! Sections separate schema validity, positive serialization, rejection, and enum vocabulary for reading.

mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jsonschema::{Draft, Registry, Validator};
use mem_adaptor_core::canonical::{CanonicalRecord, Verdict};
use mem_adaptor_core::governance::{ApprovalReceipt, Config, HomeConfig, HomeFormat};
use mem_adaptor_core::reports::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const SCHEMAS: [&str; 5] = [
    "canonical-record",
    "plan-report",
    "receipt-report",
    "approval-receipt",
    "config",
];

// Test support: local schema registration, synthetic vectors, and typed serialization checks.

/// Locates committed schema assets relative to this test crate, not the shell's working directory.
fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

/// Loads trusted synthetic JSON assets; unreadable or malformed assets are test setup failures.
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Loads all five internal contracts so cross-schema references use the same committed definitions.
fn schemas() -> BTreeMap<String, Value> {
    SCHEMAS
        .into_iter()
        .map(|name| {
            (
                name.into(),
                read_json(&schema_dir().join(format!("{name}.schema.json"))),
            )
        })
        .collect()
}

/// Resolves local URNs and enables format assertions; invalid trusted schemas fail test setup.
fn validator(schema: &Value) -> Validator {
    let mut registry = Registry::new();
    for document in schemas().into_values() {
        let id = document["$id"].as_str().unwrap().to_owned();
        registry = registry.add(id, document).unwrap();
    }
    let registry = registry.prepare().unwrap();
    jsonschema::options()
        .with_draft(Draft::Draft202012)
        .with_registry(&registry)
        .should_validate_formats(true)
        .build(schema)
        .unwrap()
}

/// Builds one validator per root contract without fetching remote schemas.
fn validators() -> BTreeMap<String, Validator> {
    schemas()
        .into_iter()
        .map(|(name, schema)| (name, validator(&schema)))
        .collect()
}

/// Reports every structural mismatch for these public synthetic values, never real export data.
fn assert_valid(validator: &Validator, document: &Value, context: &str) {
    let errors: Vec<_> = validator
        .iter_errors(document)
        .map(|error| error.to_string())
        .collect();
    assert!(errors.is_empty(), "{context}: {errors:#?}");
}

/// Checks what a typed deserialize/serialize cycle retains; it does not run engine semantic checks.
fn round_trip<T: DeserializeOwned + Serialize>(document: &Value) -> Value {
    let typed: T = serde_json::from_value(document.clone()).unwrap();
    serde_json::to_value(typed).unwrap()
}

/// Routes a known fixture contract to its Rust type; an unknown name is a fixture-author error.
fn typed_round_trip(schema: &str, document: &Value) -> Value {
    match schema {
        "canonical-record" => round_trip::<CanonicalRecord>(document),
        "plan-report" => round_trip::<PlanReport>(document),
        "receipt-report" => round_trip::<ReceiptReport>(document),
        "approval-receipt" => round_trip::<ApprovalReceipt>(document),
        "config" => round_trip::<Config>(document),
        _ => panic!("Unknown vector schema: {schema}"),
    }
}

/// Loads valid synthetic vectors in filename order for deterministic test diagnostics.
fn valid_vectors() -> Vec<(String, Value)> {
    let mut paths: Vec<_> = fs::read_dir(schema_dir().join("vectors/valid"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            (
                path.file_name().unwrap().to_str().unwrap().into(),
                read_json(&path),
            )
        })
        .collect()
}

// Schema validity: the contracts must be valid before their example values are tested.

/// Checks that each hand-authored contract is itself a valid Draft 2020-12 schema.
#[test]
fn schema_documents_are_valid_draft_2020_12() {
    for (name, schema) in schemas() {
        assert_eq!(
            schema["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        jsonschema::meta::validate(&schema).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

// Positive serialization: required-only, populated, and metadata-only values preserve their declared fields.

/// Checks every valid vector against its schema before and after a lossless typed round trip.
#[test]
fn all_valid_vectors_round_trip_without_field_loss() {
    let validators = validators();
    let vectors = valid_vectors();
    assert!(vectors.len() >= SCHEMAS.len());
    let mut covered = std::collections::BTreeSet::new();
    for (name, vector) in vectors {
        assert_eq!(vector["expected_valid"], true, "{name}");
        let schema = vector["schema"].as_str().unwrap();
        let document = &vector["document"];
        covered.insert(schema.to_owned());
        assert_valid(&validators[schema], document, &name);
        let serialized = typed_round_trip(schema, document);
        assert_eq!(&serialized, document, "{name}: serialization lost fields");
        assert_valid(&validators[schema], &serialized, &name);
    }
    assert_eq!(covered.len(), SCHEMAS.len());
}

/// Checks Rust-built minimal and populated records/reports for valid shape and serialization preservation.
/// The populated record combines optional variants for coverage, not a coherent executable migration.
#[test]
fn rust_constructed_minimal_and_full_examples_match_schemas() {
    let validators = validators();
    let approval = ApprovalReceipt {
        schema_version: "0.1.0".into(),
        receipt_id: "synthetic-approval".into(),
        plan_digest: support::HASH.into(),
        approved_at: support::TIME.into(),
        backend: "local".into(),
        approver: "human:synthetic".into(),
    };
    let config = Config {
        schema_version: "0.1.0".into(),
        gate_policy: support::policy(),
        home: None,
    };
    let examples = [
        ("canonical-record", json!(support::canonical())),
        ("canonical-record", json!(support::canonical_full())),
        ("plan-report", json!(support::plan())),
        ("plan-report", json!(support::plan_full())),
        ("receipt-report", json!(support::receipt(false))),
        ("receipt-report", json!(support::receipt(true))),
        ("approval-receipt", json!(approval)),
        ("config", json!(config)),
        (
            "config",
            json!(Config {
                home: Some(HomeConfig {
                    format: HomeFormat::Okf,
                    okf_version: "0.2".into(),
                }),
                ..config
            }),
        ),
    ];
    for (schema, document) in examples {
        assert_valid(&validators[schema], &document, schema);
        assert_eq!(typed_round_trip(schema, &document), document);
    }
}

/// Checks that model/dimension metadata can round-trip without inventing absent vector or normalization data.
#[test]
fn embedding_metadata_only_round_trips_without_synthesizing_vectors() {
    let mut record = support::canonical_full();
    record.embedding.as_mut().unwrap().vector = None;
    record.embedding.as_mut().unwrap().normalized = None;
    let document = json!(record);
    assert_valid(
        &validators()["canonical-record"],
        &document,
        "metadata-only embedding",
    );
    assert!(document["embedding"].get("vector").is_none());
    assert_eq!(round_trip::<CanonicalRecord>(&document), document);
}

// Structural rejection: invalid values and missing required fields must not pass the contracts.

/// Applies each declared invalid mutation to a valid base and checks structural rejection.
#[test]
fn all_invalid_vectors_are_rejected() {
    let validators = validators();
    let invalid = read_json(&schema_dir().join("vectors/invalid/cases.json"));
    assert_eq!(invalid["expected_valid"], false);
    let cases = invalid["cases"].as_array().unwrap();
    assert!(cases.len() >= SCHEMAS.len());
    let mut covered = std::collections::BTreeSet::new();
    for case in cases {
        let base = read_json(
            &schema_dir()
                .join("vectors")
                .join(case["base"].as_str().unwrap()),
        );
        let schema = base["schema"].as_str().unwrap();
        let constraint = case["constraint"].as_str().unwrap();
        assert!(!constraint.is_empty());
        covered.insert(schema.to_owned());
        let mut document = base["document"].clone();
        assert_valid(&validators[schema], &document, constraint);
        for patch in case["patch"].as_array().unwrap() {
            let path = patch["path"].as_str().unwrap();
            let (parent, member) = path.rsplit_once('/').unwrap();
            let member = member.replace("~1", "/").replace("~0", "~");
            let object = document
                .pointer_mut(parent)
                .unwrap()
                .as_object_mut()
                .unwrap();
            match patch["op"].as_str().unwrap() {
                "remove" => {
                    assert!(object.remove(&member).is_some());
                }
                "add" => {
                    assert!(patch.as_object().unwrap().contains_key("value"));
                    object.insert(member, patch["value"].clone());
                }
                op => panic!("Unsupported vector patch operation: {op}"),
            }
        }
        assert!(
            !validators[schema].is_valid(&document),
            "Accepted invalid vector: {constraint}"
        );
    }
    assert_eq!(covered.len(), SCHEMAS.len());
}

/// Removes each schema-required root field in turn; optional omissions belong in the minimal examples instead.
#[test]
fn removing_each_required_top_level_field_is_rejected() {
    let schemas = schemas();
    let validators = validators();
    for (name, vector) in valid_vectors() {
        let schema = vector["schema"].as_str().unwrap();
        for field in schemas[schema]["required"].as_array().unwrap() {
            let field = field.as_str().unwrap();
            let mut document = vector["document"].clone();
            assert!(document.as_object_mut().unwrap().remove(field).is_some());
            assert!(
                !validators[schema].is_valid(&document),
                "{name}: missing {field} was accepted"
            );
        }
    }
}

// Enum vocabulary: these serialized alternatives do not prove their runtime enforcement.

/// Checks the currently enumerated disposition, verification, and verdict shapes, not their runtime enforcement.
#[test]
fn every_disposition_and_verification_variant_matches_schema() {
    let dispositions = [
        Disposition::Accepted,
        Disposition::Transformed {
            changes: vec![Change {
                field_path: "/content".into(),
                kind: ChangeKind::Reformatted,
            }],
        },
        Disposition::Omitted {
            reason: OmissionReason::DuplicateOf {
                canonical_id: support::ID.into(),
            },
        },
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated,
        },
        Disposition::Omitted {
            reason: OmissionReason::DeletedInTarget,
        },
        Disposition::Omitted {
            reason: OmissionReason::TargetUnsupported {
                field: "/ttl".into(),
            },
        },
        Disposition::Omitted {
            reason: OmissionReason::VerdictExcluded {
                cluster_id: support::HASH.into(),
            },
        },
        Disposition::Omitted {
            reason: OmissionReason::SourceDeleted,
        },
        Disposition::Omitted {
            reason: OmissionReason::SecretReferenceUnsupported,
        },
        Disposition::Unresolved {
            reason: UnresolvedReason::Conflict {
                cluster_id: support::HASH.into(),
            },
        },
        Disposition::Unresolved {
            reason: UnresolvedReason::DnaUnsupported {
                field: "/ttl".into(),
            },
        },
        Disposition::Unresolved {
            reason: UnresolvedReason::MemoryDisabled,
        },
        Disposition::Unresolved {
            reason: UnresolvedReason::DeletionNeedsDecision,
        },
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified,
        },
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetUntracked,
        },
        Disposition::Rejected {
            rule: "synthetic-rule".into(),
        },
    ];
    let disposition_validator = validator(&json!({
        "$ref": "urn:mem-adaptor:schema:plan-report:0.1.0#/$defs/disposition"
    }));
    for disposition in dispositions {
        let document = json!(disposition);
        assert_valid(&disposition_validator, &document, "disposition");
        assert_eq!(round_trip::<Disposition>(&document), document);
    }
    let verification_validator = validator(&json!({
        "$ref": "urn:mem-adaptor:schema:receipt-report:0.1.0#/$defs/verification"
    }));
    for verification in [
        Verification::Verified,
        Verification::Mismatch {
            diff: vec![FieldDiff {
                field_path: "/content".into(),
                kind: DiffKind::Changed,
                expected_hash: Some(support::HASH.into()),
                actual_hash: Some(support::HASH.into()),
            }],
        },
        Verification::Unverifiable {
            why: "Synthetic target has no read API.".into(),
        },
    ] {
        let document = json!(verification);
        assert_valid(&verification_validator, &document, "verification");
        assert_eq!(round_trip::<Verification>(&document), document);
    }
    let verdict_validator = validator(&json!({
        "$ref": "urn:mem-adaptor:schema:canonical-record:0.1.0#/$defs/verdict"
    }));
    for verdict in [
        Verdict::Keep {
            cluster_id: support::HASH.into(),
            canonical_ids: vec![support::ID.into()],
        },
        Verdict::NeedsMoreContext {
            cluster_id: support::HASH.into(),
        },
    ] {
        let document = json!(verdict);
        assert_valid(&verdict_validator, &document, "verdict");
        assert_eq!(round_trip::<Verdict>(&document), document);
    }
}
