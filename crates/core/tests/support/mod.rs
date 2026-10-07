//! Synthetic typed examples shared by the core schema tests.
//! Minimal and populated records expose required/optional shape; IDs and hashes are illustrative placeholders.
//! Report constructors exercise serialization vocabulary, not engine execution or real platform coverage.

use mem_adaptor_core::canonical::*;
use mem_adaptor_core::governance::*;
use mem_adaptor_core::reports::*;

pub const ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const HASH: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const TIME: &str = "2026-10-05T12:00:00Z";

/// Supplies a default pass policy for shape tests without simulating a user's policy choice.
pub fn policy() -> GatePolicy {
    GatePolicy {
        secrets: GateAction::Pass,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}

/// Builds the required record shape with all optional semantics absent, not a semantically verified Reader output.
pub fn canonical() -> CanonicalRecord {
    CanonicalRecord {
        canonical_id: ID.into(),
        source: SourceIdentity {
            system: "markdown".into(),
            adapter_version: "0.1.0".into(),
            export_version: "unknown".into(),
            satellite_id: None,
        },
        source_record_id: "example.md".into(),
        source_locator: "example.md".into(),
        scope: Scope::User,
        content: "Synthetic memory.\n".into(),
        content_hash: HASH.into(),
        dna_class: DnaClass::Standard,
        provenance: Provenance {
            actor: "human:synthetic".into(),
            actor_kind: ActorKind::User,
            method: "filesystem".into(),
            source_ref: None,
            evidence: None,
        },
        evidence_level: EvidenceLevel::Measured,
        scope_qualifier: None,
        owner_declared: None,
        source_kind: None,
        source_extra: None,
        tags: None,
        entities: None,
        relations: None,
        created_at: None,
        updated_at: None,
        observed_at: None,
        valid_from: None,
        valid_to: None,
        expires_at: None,
        ttl: None,
        consent: None,
        approval: None,
        sensitive_findings: None,
        deletion_intent: None,
        tombstone: None,
        embedding: None,
        reembed_plan: None,
        conflict_cluster_id: None,
        conflict_candidates: None,
        verdict: None,
    }
}

/// Populates every optional record field for serialization coverage; these combined declarations are not a migration scenario.
pub fn canonical_full() -> CanonicalRecord {
    let mut record = canonical();
    record.source.satellite_id = Some("abcd2345".into());
    record.scope_qualifier = Some("synthetic-project".into());
    record.owner_declared = Some("untrusted-example".into());
    record.source_kind = Some("preference".into());
    record.source_extra = Some(
        serde_json::json!({"custom": {"enabled": true, "labels": ["synthetic"], "count": 2}})
            .as_object()
            .unwrap()
            .clone(),
    );
    record.tags = Some(vec!["synthetic".into()]);
    record.entities = Some(vec![Entity {
        id: "example".into(),
        kind: "project".into(),
        label: Some("Synthetic project".into()),
    }]);
    record.relations = Some(vec![Relation {
        kind: "about".into(),
        source: "example".into(),
        target: "other".into(),
    }]);
    record.created_at = Some(TIME.into());
    record.updated_at = Some(TIME.into());
    record.observed_at = Some(TIME.into());
    record.valid_from = Some(TIME.into());
    record.valid_to = Some(TIME.into());
    record.expires_at = Some(TIME.into());
    record.ttl = Some("P2D".into());
    record.provenance.source_ref = Some("example.md".into());
    record.provenance.evidence = Some(vec![Evidence {
        source_ref: "example.md".into(),
        weight: Some(0.75),
    }]);
    record.consent = Some(Consent {
        exportable: Some(true),
        retention: Some("P2D".into()),
        redact: Some(vec!["$.private".into()]),
        memory_enabled: Some(true),
    });
    record.approval = Some(ApprovalState {
        state: ApprovalStatus::Approved,
        receipt_ref: Some("approval.json".into()),
    });
    record.sensitive_findings = Some(vec![Finding {
        rule_id: "synthetic-rule".into(),
        tier: FindingTier::PersonalFact,
        field_path: "/content".into(),
        key_hash: None,
        byte_span: ByteSpan { start: 0, end: 9 },
        disposition: FindingDisposition::Reported,
    }]);
    record.deletion_intent = Some(DeletionIntent::Deprecate);
    record.tombstone = Some(Tombstone {
        deleted_at: TIME.into(),
        source_ref: "deletion/example".into(),
        actor: Some("human:synthetic".into()),
    });
    record.embedding = Some(Embedding {
        model: "synthetic-local".into(),
        dim: 2,
        vector: Some(vec![0.25, 0.75]),
        normalized: Some(false),
    });
    record.reembed_plan = Some(ReembedPlan {
        canonical_ids: vec![ID.into()],
        model: "synthetic-local".into(),
        dim: 2,
        quality_impact: "Synthetic fixture only.".into(),
    });
    record.conflict_cluster_id = Some(HASH.into());
    record.conflict_candidates = Some(vec![ConflictCandidate {
        canonical_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        basis: "synthetic".into(),
    }]);
    record.verdict = Some(Verdict::Keep {
        cluster_id: HASH.into(),
        canonical_ids: vec![ID.into()],
    });
    record
}

/// Builds an empty plan shape with target/writer declarations and a placeholder digest.
pub fn plan() -> PlanReport {
    let target = TargetSpec {
        id: "home".into(),
        location: "synthetic-home".into(),
        writer: "okf".into(),
        artifacts: vec![],
    };
    let writer = WriterSpec {
        id: "okf".into(),
        version: "0.1.0".into(),
        capabilities: Capabilities {
            supported_fields: vec!["/content".into()],
            unsupported_fields: vec!["/embedding/vector".into()],
            read_back: true,
            update: true,
        },
    };
    PlanReport {
        schema_version: "0.1.0".into(),
        canonical_model_version: "0.1.0".into(),
        run_id: "synthetic-plan".into(),
        created_at: TIME.into(),
        source: SourceSpec {
            location: "synthetic-source".into(),
            system: "markdown".into(),
            export_version: "unknown".into(),
            adapters: vec![AdapterVersion {
                id: "markdown".into(),
                version: "0.1.0".into(),
            }],
            satellite: None,
        },
        source_inventory: SourceInventory {
            files: vec![],
            state: InventoryState::Empty,
        },
        targets: vec![target.clone()],
        writers: vec![writer.clone()],
        model_calls: vec![],
        gate_policy: policy(),
        entries: vec![],
        source_unavailable: vec![],
        anomalies: vec![],
        warnings: vec![],
        bundle_manifest: BundleManifest {
            source_system: "markdown".into(),
            export_version: "unknown".into(),
            files: vec![],
            exported_at: None,
        },
        digest_inputs: DigestInputs {
            records: vec![],
            targets: vec![target],
            writers: vec![writer],
            gate_policy: policy(),
            previous_receipt_hash: None,
        },
        plan_digest: HASH.into(),
        previous_receipt_ref: None,
    }
}

/// Adds populated inventory, mappings, predictions, and diagnostics to exercise optional plan serialization.
pub fn plan_full() -> PlanReport {
    let mut report = plan();
    report.source.satellite = Some(SatelliteSpec {
        id: "abcd2345".into(),
        label: Some("Synthetic satellite".into()),
    });
    report.source_inventory = SourceInventory {
        files: vec![InventoryFile {
            path: "example.md".into(),
            content_hash: HASH.into(),
            bytes: 18,
            status: InventoryStatus::Claimed,
            reader: Some("markdown".into()),
            layer: Some("auto_memory".into()),
            registered_count: None,
            deleted_count: None,
        }],
        state: InventoryState::DataPresent,
    };
    report.model_calls = vec![ModelCall {
        origin: ModelCallOrigin::OptIn,
        model: "synthetic-local".into(),
        count: 1,
        canonical_ids: vec![ID.into()],
        field_paths: vec!["/content".into()],
        remote: false,
    }];
    let record = canonical_full();
    report.entries = vec![PlanEntry {
        canonical_id: ID.into(),
        source_record_id: "example.md".into(),
        source_locator: "example.md".into(),
        content_hash: HASH.into(),
        target: "home".into(),
        disposition: Disposition::Transformed {
            changes: vec![Change {
                field_path: "/embedding/vector".into(),
                kind: ChangeKind::FieldOmitted,
            }],
        },
        field_map: vec![FieldMapping {
            source_path: "/body".into(),
            canonical_path: "/content".into(),
            rule: None,
        }],
        target_map: vec![TargetMapping {
            canonical_path: "/content".into(),
            target_path: "/body".into(),
            rule: "body_bytes_unchanged".into(),
        }],
        unmapped: vec![UnmappedField {
            source_path: "/custom".into(),
            reason: UnmappedReason::SourceUnknown,
        }],
        sensitive_findings: record.sensitive_findings.unwrap(),
        evidence_level: EvidenceLevel::Measured,
        content_preview: Some("Synthetic memory.".into()),
        reembed_plan: record.reembed_plan,
        prior_write: None,
        duplicate_write: None,
    }];
    report.source_unavailable = vec![SourceUnavailable {
        system: "chatgpt".into(),
        layer: "memory_summary".into(),
        reason: "Synthetic example of unavailable source data.".into(),
        evidence_level: EvidenceLevel::ThirdParty,
    }];
    report.anomalies = vec![Anomaly {
        source_locator: "example.md".into(),
        code: "synthetic_bad_line".into(),
        line: Some(1),
        field_path: Some("/body".into()),
    }];
    report.warnings = vec!["Synthetic warning.".into()];
    report.bundle_manifest.exported_at = Some(TIME.into());
    report.bundle_manifest.files = vec![ManifestFile {
        path: "example.md".into(),
        content_hash: HASH.into(),
        bytes: 18,
    }];
    report.digest_inputs.records = vec![DigestRecord {
        canonical_id: ID.into(),
        content_hash: HASH.into(),
        record_hash: HASH.into(),
        predictions: vec![Prediction {
            target: "home".into(),
            disposition: report.entries[0].disposition.clone(),
            target_map: report.entries[0].target_map.clone(),
            prior_write: None,
            duplicate_write: None,
        }],
    }];
    report
}

/// Builds an empty or populated receipt shape; illustrative verified entries are not evidence of a real write.
pub fn receipt(full: bool) -> ReceiptReport {
    let plan = if full { plan_full() } else { plan() };
    ReceiptReport {
        schema_version: plan.schema_version,
        canonical_model_version: plan.canonical_model_version,
        run_id: "synthetic-receipt".into(),
        created_at: TIME.into(),
        source: plan.source,
        targets: plan.targets,
        writers: plan.writers,
        model_calls: plan.model_calls,
        gate_policy: plan.gate_policy,
        bundle_manifest: plan.bundle_manifest,
        plan_digest: plan.plan_digest,
        plan_ref: "plan.json".into(),
        approval_receipt_ref: "approval.json".into(),
        entries: plan
            .entries
            .into_iter()
            .map(|entry| ReceiptEntry {
                canonical_id: entry.canonical_id,
                source_record_id: entry.source_record_id,
                source_locator: entry.source_locator,
                content_hash: entry.content_hash,
                target: entry.target,
                disposition: entry.disposition,
                target_map: entry.target_map,
                sensitive_findings: entry.sensitive_findings,
                evidence_level: entry.evidence_level,
                target_id: Some(format!("memories/{ID}")),
                verification: Some(Verification::Verified),
                duplicate_write: None,
                prior_write: Some(PriorWrite {
                    target_id: format!("memories/{ID}"),
                    content_hash: HASH.into(),
                    record_hash: HASH.into(),
                    target_hash: HASH.into(),
                    verification: Verification::Verified,
                }),
            })
            .collect(),
        verdicts: if full {
            vec![Verdict::NeedsMoreContext {
                cluster_id: HASH.into(),
            }]
        } else {
            vec![]
        },
        previous_receipt_ref: full.then(|| "previous.json".into()),
    }
}
