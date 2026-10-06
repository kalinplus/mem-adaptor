//! Coordinates registered adapters through local planning, approved writes, and read-back receipts.
//! The identity and hash helpers below distinguish source identity from content and metadata changes.
//! Adapter-specific parsing and mapping stay in plugins; the engine does not provide multi-file rollback.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::{Context, ensure};
use data_encoding::{BASE32_NOPAD, HEXLOWER};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::info;

use crate::Result;
use crate::canonical::{CanonicalRecord, Verdict};
use crate::governance::{ApprovalReceipt, FindingDisposition, GatePolicy};
use crate::plugins::*;
use crate::reports::*;
use crate::source::load_source;

pub const SCHEMA_VERSION: &str = "0.1.0";
pub const CANONICAL_MODEL_VERSION: &str = "0.1.0";

/// Returns a prefixed SHA-256 hash of the exact bytes, without whitespace or text normalization.
/// Hashing record content detects body changes; it does not establish record identity.
pub fn content_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", HEXLOWER.encode(&Sha256::digest(bytes)))
}

/// Derives a stable, 32-character lowercase base32 ID from the source system and native record ID.
/// A NUL separator distinguishes otherwise ambiguous concatenations; the first 20 hash bytes form the ID.
/// Content and metadata do not affect it, so callers must supply a stable native ID within the source system.
/// This helper neither validates the source ID nor performs content deduplication.
pub fn canonical_id(system: &str, source_record_id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(system.as_bytes());
    hash.update([0]);
    hash.update(source_record_id.as_bytes());
    BASE32_NOPAD
        .encode(&hash.finalize()[..20])
        .to_ascii_lowercase()
}

/// Formats the current UTC time for reports and approvals, independently of record identity.
pub fn timestamp() -> Result<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

/// Hashes the JCS serialization of records, target state, writers, and gate policy for approval binding.
/// Callers supply deterministically ordered arrays; JCS orders object keys, not array elements.
/// Report run IDs and generation times are outside DigestInputs and therefore do not affect this hash.
/// Serialization or unsafe-number errors propagate; this helper does not approve a plan or inspect targets.
pub fn plan_digest(inputs: &DigestInputs) -> Result<String> {
    Ok(content_hash(&crate::jcs::to_vec(inputs)?))
}

/// Hashes the complete serialized canonical record, including its body, source, scope, and consent.
/// Unlike content_hash, it detects metadata-only changes without deriving a new canonical_id.
/// Serialization or unsafe-number errors propagate; the hash itself does not enforce export permission.
pub fn record_hash(record: &CanonicalRecord) -> Result<String> {
    Ok(content_hash(&crate::jcs::to_vec(record)?))
}

/// Creates a new masked JSON artifact without overwriting an existing report.
/// Serialization/create/write errors propagate; a write failure may leave a partial file, not an atomic report.
/// Masking detected secret patterns is not comprehensive PII cleansing or target-payload redaction.
pub fn write_json_new(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    let mut document = serde_json::to_value(value)?;
    crate::gate::mask_value(&mut document);
    let bytes = serde_json::to_vec_pretty(&document)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).with_context(|| {
        format!(
            "Cannot create report: {}",
            crate::gate::mask(&path.to_string_lossy())
        )
    })?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Only the engine can issue this capability after checking the approved plan.
///
/// ```compile_fail
/// use mem_adaptor_core::engine::WriteToken;
/// let token = WriteToken { batches: Default::default() };
/// ```
pub struct WriteToken {
    batches: BTreeMap<String, String>,
    artifacts: BTreeMap<String, Vec<TargetArtifact>>,
}

impl WriteToken {
    pub fn authorize(&self, writer: &dyn Writer, batch: &[Planned]) -> Result<()> {
        let key = format!("{}:{}", writer.id(), writer.location().display());
        ensure!(
            self.batches.get(&key) == Some(&batch_hash(batch)?),
            "WriteToken does not authorize this target or batch"
        );
        Ok(())
    }

    pub fn authorize_artifact(
        &self,
        writer: &dyn Writer,
        path: &str,
        bytes: Option<&[u8]>,
    ) -> Result<()> {
        let key = format!("{}:{}", writer.id(), writer.location().display());
        let expected = self
            .artifacts
            .get(&key)
            .and_then(|artifacts| artifacts.iter().find(|artifact| artifact.path == path));
        let actual = TargetArtifact {
            path: path.into(),
            content_hash: bytes.map(content_hash),
            bytes: bytes.map(|bytes| bytes.len() as u64),
        };
        ensure!(
            expected == Some(&actual),
            "WriteToken target artifact differs from approval"
        );
        Ok(())
    }
}

fn batch_hash(batch: &[Planned]) -> Result<String> {
    let records: Vec<_> = batch
        .iter()
        .map(|planned| {
            (
                &planned.record,
                &planned.target_id,
                &planned.disposition,
                &planned.previous_write,
                &planned.duplicate_write,
                &planned.target_map,
            )
        })
        .collect();
    Ok(content_hash(&crate::jcs::to_vec(&records)?))
}

fn duplicate_key(record: &CanonicalRecord) -> Result<(String, String)> {
    let normalized = record
        .content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut metadata = serde_json::to_value(record)?;
    for key in [
        "canonical_id",
        "source",
        "source_record_id",
        "source_locator",
        "content",
        "content_hash",
        "sensitive_findings",
    ] {
        metadata.as_object_mut().unwrap().remove(key);
    }
    metadata["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("source_ref");
    Ok((
        content_hash(&crate::jcs::to_vec(&metadata)?),
        content_hash(normalized.as_bytes()),
    ))
}

struct PipelinePlan {
    report: PlanReport,
    batches: BTreeMap<String, Vec<Planned>>,
    previous: Option<ReceiptReport>,
}

fn dedup_eligible(disposition: &Disposition) -> bool {
    matches!(
        disposition,
        Disposition::Accepted
            | Disposition::Transformed { .. }
            | Disposition::Omitted {
                reason: OmissionReason::AlreadyMigrated
            }
    )
}

pub struct Engine {
    pub registry: Registry,
}

impl Engine {
    /// Predicts treatment using no historical receipt; planning inspects inputs but does not invoke Writer writes.
    pub fn plan(&self, source: &Path, gate_policy: GatePolicy) -> Result<PlanReport> {
        self.plan_with_previous(source, gate_policy, None)
    }

    /// Returns a validated plan with optional prior evidence; explicitly supplied invalid history is an error.
    /// Source/adapter/validation errors propagate before target writes; saving the report belongs to the caller.
    pub fn plan_with_previous(
        &self,
        source: &Path,
        gate_policy: GatePolicy,
        previous: Option<&Path>,
    ) -> Result<PlanReport> {
        Ok(self.prepare(source, gate_policy, previous)?.report)
    }

    /// Rebuilds source coverage, target projections, and approval inputs for both plan and apply.
    /// Checks policy, records, and supplied history; it may extract a ZIP to temporary storage but never writes targets.
    fn prepare(
        &self,
        source: &Path,
        gate_policy: GatePolicy,
        previous_ref: Option<&Path>,
    ) -> Result<PipelinePlan> {
        let started = Instant::now();
        crate::schema::validate(
            "config",
            &crate::governance::Config {
                schema_version: SCHEMA_VERSION.into(),
                gate_policy: gate_policy.clone(),
                home: None,
            },
        )?;
        crate::gate::validate_policy(&gate_policy)?;
        info!("[S1] source inventory started");
        let source = load_source(source)?;
        ensure!(
            crate::gate::mask(source.root.to_str().context("Source path is not UTF-8")?)
                == source.root.to_string_lossy(),
            "Sensitive source path cannot be exposed in a report"
        );
        for path in source.files.keys() {
            ensure!(
                crate::gate::mask(path) == *path,
                "Sensitive source filename cannot be exposed in a report"
            );
        }
        let previous = previous_ref
            .map(|path| -> Result<ReceiptReport> {
                let receipt: ReceiptReport = serde_json::from_slice(&fs::read(path)?)
                    .map_err(|_| anyhow::anyhow!("Invalid previous receipt JSON or fields"))?;
                crate::schema::validate("receipt-report", &receipt)?;
                ensure!(
                    receipt.source.location == source.root.to_string_lossy(),
                    "Previous receipt belongs to another source"
                );
                ensure!(
                    receipt
                        .entries
                        .iter()
                        .all(|entry| entry.verification.is_none() || entry.prior_write.is_some()),
                    "Previous receipt lacks historical write state"
                );
                let mut keys = BTreeSet::new();
                ensure!(
                    receipt
                        .entries
                        .iter()
                        .all(|entry| keys.insert((&entry.target, &entry.canonical_id))),
                    "Duplicate identity in previous receipt"
                );
                Ok(receipt)
            })
            .transpose()?;
        let mut inventory: BTreeMap<_, _> = source
            .files
            .iter()
            .map(|(path, bytes)| {
                (
                    path.clone(),
                    InventoryFile {
                        path: path.clone(),
                        content_hash: content_hash(bytes),
                        bytes: bytes.len() as u64,
                        status: InventoryStatus::Unclaimed,
                        reader: None,
                        layer: None,
                        registered_count: None,
                        deleted_count: None,
                    },
                )
            })
            .collect();
        let mut claimed = BTreeSet::new();
        let mut outputs = Vec::new();
        let mut adapters = Vec::new();
        let mut anomalies = Vec::new();
        let mut source_unavailable = Vec::new();
        for reader in &self.registry.readers {
            let claims = reader.claim(&source.files);
            if !claims.is_empty() {
                adapters.push(AdapterVersion {
                    id: reader.id().into(),
                    version: reader.version().into(),
                });
            }
            for claim in claims {
                ensure!(
                    claimed.insert(claim.path.clone()),
                    "File claimed by multiple Readers"
                );
                let entry = inventory
                    .get_mut(&claim.path)
                    .context("Reader claimed an unknown file")?;
                entry.reader = Some(reader.id().into());
                entry.layer = Some(claim.layer.clone());
                entry.status = if claim.registered_only {
                    InventoryStatus::RegisteredOnly
                } else {
                    InventoryStatus::Claimed
                };
                info!("[S2] parsing source file");
                let output = reader.read(&claim, &source)?;
                if claim.registered_only {
                    entry.registered_count = Some(output.registered_count);
                }
                if output.deleted_count > 0 {
                    entry.deleted_count = Some(output.deleted_count);
                }
                for missing in &output.source_unavailable {
                    if !source_unavailable.contains(missing) {
                        source_unavailable.push(missing.clone());
                    }
                }
                anomalies.extend(output.anomalies.iter().cloned());
                outputs.push(output);
            }
        }
        info!(
            files = inventory.len(),
            claimed = claimed.len(),
            elapsed_ms = started.elapsed().as_millis(),
            "[S1] source inventory complete"
        );
        info!(
            source_records = outputs
                .iter()
                .map(|output| output.source_records.len())
                .sum::<usize>(),
            "[S2] parsing complete"
        );
        let mut records = BTreeMap::new();
        let mut mappings = BTreeMap::new();
        let mut source_findings = BTreeMap::new();
        let mut reference_records = BTreeSet::new();
        for mut output in outputs {
            audit_fields(&mut output);
            anomalies.extend(
                output
                    .anomalies
                    .iter()
                    .filter(|anomaly| anomaly.code == "reader_unreported_field")
                    .cloned(),
            );
            for source_record in &output.source_records {
                let mut findings =
                    crate::gate::scan_keys(&source_record.fields, "/source_fields", &gate_policy);
                for field in &source_record.unmapped {
                    if let Some(value) = source_record.fields.pointer(&field.source_path) {
                        findings.extend(crate::gate::scan_value(
                            value,
                            &format!("/source_fields{}", crate::gate::mask(&field.source_path)),
                            &gate_policy,
                        ));
                    }
                }
                source_findings.insert(source_record.canonical_id.clone(), findings);
                if source_record.fields["type"] == "secretRef"
                    || source_record.fields["kind"] == "secretRef"
                    || source_record.fields["frontmatter"]["type"] == "secretRef"
                {
                    reference_records.insert(source_record.canonical_id.clone());
                }
            }
            for record in output.records {
                // Schema validation checks structure; re-derive identity and body hashes rather than trusting Reader claims.
                crate::schema::validate("canonical-record", &record)?;
                ensure!(
                    record.canonical_id
                        == canonical_id(&record.source.system, &record.source_record_id),
                    "Reader canonical identity mismatch"
                );
                if let Some(embedding) = &record.embedding
                    && let Some(vector) = &embedding.vector
                {
                    ensure!(
                        vector.len() as u64 == embedding.dim,
                        "Embedding vector length does not match dimension"
                    );
                }
                ensure!(
                    record.content_hash == content_hash(record.content.as_bytes()),
                    "Reader content hash mismatch"
                );
                let source_record = output
                    .source_records
                    .iter()
                    .find(|source| source.canonical_id == record.canonical_id)
                    .context("Canonical record has no source record")?;
                ensure!(
                    source_record.source_record_id == record.source_record_id,
                    "Source identity association mismatch"
                );
                mappings.insert(
                    record.canonical_id.clone(),
                    (
                        source_record.field_map.clone(),
                        source_record.unmapped.clone(),
                    ),
                );
                ensure!(
                    records
                        .insert(record.canonical_id.clone(), record)
                        .is_none(),
                    "Duplicate canonical identity"
                );
            }
        }
        info!(
            records = records.len(),
            "[S3] canonical schema and source-field coverage validated"
        );
        let mut gate_dispositions = BTreeMap::new();
        for record in records.values_mut() {
            for identity in [&record.source_record_id, &record.source_locator] {
                ensure!(
                    crate::gate::mask(identity) == *identity,
                    "Sensitive source identity cannot be exposed in a report"
                );
            }
            let mut document = serde_json::to_value(&record)?;
            document
                .as_object_mut()
                .unwrap()
                .remove("sensitive_findings");
            let mut findings = crate::gate::scan_value(&document, "", &gate_policy);
            findings.extend(
                source_findings
                    .remove(&record.canonical_id)
                    .unwrap_or_default(),
            );
            if reference_records.contains(&record.canonical_id)
                || record.source_kind.as_deref() == Some("secretRef")
            {
                record.content = "[Secret reference: value not exported]".into();
                record.content_hash = content_hash(record.content.as_bytes());
                gate_dispositions.insert(
                    record.canonical_id.clone(),
                    Disposition::Omitted {
                        reason: OmissionReason::SecretReferenceUnsupported,
                    },
                );
            } else if let Some(finding) = findings
                .iter()
                .find(|finding| finding.disposition == FindingDisposition::Blocked)
            {
                gate_dispositions.insert(
                    record.canonical_id.clone(),
                    Disposition::Rejected {
                        rule: finding.rule_id.clone(),
                    },
                );
            } else if record
                .consent
                .as_ref()
                .is_some_and(|consent| consent.exportable == Some(false))
            {
                gate_dispositions.insert(
                    record.canonical_id.clone(),
                    Disposition::Rejected {
                        rule: "consent_export_disabled".into(),
                    },
                );
            } else if record
                .consent
                .as_ref()
                .is_some_and(|consent| consent.memory_enabled == Some(false))
            {
                gate_dispositions.insert(
                    record.canonical_id.clone(),
                    Disposition::Unresolved {
                        reason: UnresolvedReason::MemoryDisabled,
                    },
                );
            } else if record.deletion_intent.is_some() || record.tombstone.is_some() {
                gate_dispositions.insert(
                    record.canonical_id.clone(),
                    Disposition::Unresolved {
                        reason: UnresolvedReason::DeletionNeedsDecision,
                    },
                );
            }
            record.sensitive_findings = (!findings.is_empty()).then_some(findings);
        }
        info!(
            records = records.len(),
            findings = records
                .values()
                .filter_map(|record| record.sensitive_findings.as_ref())
                .map(Vec::len)
                .sum::<usize>(),
            "[S4] local secret detection complete"
        );
        let previous_entries: BTreeMap<_, _> = previous
            .as_ref()
            .into_iter()
            .flat_map(|receipt| receipt.entries.iter())
            .map(|entry| ((entry.target.clone(), entry.canonical_id.clone()), entry))
            .collect();
        info!(
            records = records.len(),
            previous_entries = previous_entries.len(),
            "[S5] deduplication and receipt reconciliation started"
        );
        let mut warnings =
            vec!["PII detection is not implemented; high_risk_pii is reserved for D2–3.".into()];
        if records.is_empty() {
            warnings.push("No memory records were parsed; see the source inventory for registered and unclaimed files.".into());
        }
        if !gate_policy.user_selected {
            warnings.push("Gate policy comes from defaults, not a user choice.".into());
        }
        let mut batches = BTreeMap::new();
        let mut entries = Vec::new();
        let mut targets = Vec::new();
        let mut writers = BTreeMap::new();
        // Bind complete records so unchanged body text cannot hide scope, consent, or source metadata changes.
        let mut digest_records: BTreeMap<String, DigestRecord> = records
            .values()
            .map(|record| {
                (
                    record.canonical_id.clone(),
                    DigestRecord {
                        canonical_id: record.canonical_id.clone(),
                        content_hash: record.content_hash.clone(),
                        record_hash: record_hash(record).unwrap(),
                        predictions: vec![],
                    },
                )
            })
            .collect();
        for (target, writer) in &self.registry.writers {
            ensure!(
                crate::gate::mask(&writer.location().to_string_lossy())
                    == writer.location().to_string_lossy(),
                "Sensitive target path cannot be exposed in a report"
            );
            if let Some(receipt) = &previous {
                if let Some(old) = receipt.targets.iter().find(|old| old.id == *target) {
                    ensure!(
                        old.location == writer.location().to_string_lossy()
                            && old.writer == writer.id(),
                        "Previous receipt belongs to another target"
                    );
                } else {
                    warnings.push(format!(
                        "Target {target}: no previous receipt; deletion protection unavailable."
                    ));
                }
            } else if writer.location().exists()
                && fs::read_dir(writer.location())?
                    .next()
                    .transpose()?
                    .is_some()
            {
                warnings.push(format!("Target {target}: nonempty target without previous receipt; deletion protection unavailable."));
            }
            targets.push(TargetSpec {
                id: target.clone(),
                location: writer.location().to_string_lossy().into_owned(),
                writer: writer.id().into(),
                artifacts: writer.artifacts(
                    &records
                        .values()
                        .map(|record| writer.plan(record, None).target_id)
                        .collect::<Vec<_>>(),
                )?,
            });
            let shared_modified = previous
                .as_ref()
                .and_then(|receipt| receipt.targets.iter().find(|item| item.id == *target))
                .is_some_and(|old| {
                    writer.shared_artifact_paths().iter().any(|path| {
                        old.artifacts.iter().find(|artifact| artifact.path == *path)
                            != targets
                                .last()
                                .unwrap()
                                .artifacts
                                .iter()
                                .find(|artifact| artifact.path == *path)
                    })
                });
            writers.insert(
                writer.id(),
                WriterSpec {
                    id: writer.id().into(),
                    version: writer.version().into(),
                    capabilities: writer.capabilities(),
                },
            );
            let mut batch = Vec::new();
            for record in records.values() {
                let previous_entry = previous_entries
                    .get(&(target.clone(), record.canonical_id.clone()))
                    .copied();
                let mut planned = writer.plan(record, previous_entry);
                planned.previous_write = previous_entry.and_then(|entry| entry.prior_write.clone());
                planned.duplicate_write =
                    previous_entry.and_then(|entry| entry.duplicate_write.clone());
                if let Some(disposition) = gate_dispositions.get(&record.canonical_id) {
                    planned.disposition = disposition.clone();
                } else if matches!(
                    planned.disposition,
                    Disposition::Accepted | Disposition::Transformed { .. }
                ) && let Some(prior) = planned.previous_write.as_ref().or_else(|| {
                    planned
                        .duplicate_write
                        .as_ref()
                        .map(|reference| &reference.prior_write)
                }) {
                    if prior.verification == Verification::Verified
                        && writer.capabilities().read_back
                    {
                        match writer.inspect(&prior.target_id)? {
                            None => {
                                planned.disposition = Disposition::Omitted {
                                    reason: OmissionReason::DeletedInTarget,
                                }
                            }
                            Some(actual)
                                if record_hash(&actual)? != prior.record_hash
                                    || writer.target_hash(&prior.target_id)?.as_ref()
                                        != Some(&prior.target_hash) =>
                            {
                                planned.disposition = Disposition::Unresolved {
                                    reason: UnresolvedReason::TargetModified,
                                }
                            }
                            Some(actual)
                                if planned.previous_write.is_some()
                                    && actual.canonical_id != record.canonical_id
                                    || planned.previous_write.is_none()
                                        && planned.duplicate_write.as_ref().is_some_and(
                                            |reference| {
                                                reference.canonical_id != actual.canonical_id
                                            },
                                        ) =>
                            {
                                planned.disposition = Disposition::Unresolved {
                                    reason: UnresolvedReason::TargetModified,
                                };
                            }
                            Some(_)
                                if planned.previous_write.is_some()
                                    && record_hash(&planned.record)? == prior.record_hash =>
                            {
                                planned.disposition = Disposition::Omitted {
                                    reason: OmissionReason::AlreadyMigrated,
                                }
                            }
                            Some(_) if !writer.capabilities().update => {
                                planned.disposition = Disposition::Omitted {
                                    reason: OmissionReason::TargetUnsupported {
                                        field: "/content".into(),
                                    },
                                }
                            }
                            Some(_) => {}
                        }
                    } else {
                        warnings.push(format!("Target {target}, record {}: previous write was not verified; confirm target state manually.", record.canonical_id));
                        planned.disposition = Disposition::Unresolved {
                            reason: UnresolvedReason::TargetModified,
                        };
                    }
                } else if matches!(
                    planned.disposition,
                    Disposition::Accepted | Disposition::Transformed { .. }
                ) && writer.inspect(&planned.target_id)?.is_some()
                {
                    planned.disposition = Disposition::Unresolved {
                        reason: UnresolvedReason::TargetUntracked,
                    };
                }
                if !gate_dispositions.contains_key(&record.canonical_id)
                    && let Some(Verdict::Keep { cluster_id, canonical_ids }) = record.conflict_cluster_id.as_ref().and_then(|cluster| previous.as_ref()?.verdicts.iter().find(|verdict| matches!(verdict, Verdict::Keep { cluster_id, .. } if cluster_id == cluster)))
                    && !canonical_ids.contains(&record.canonical_id) {
                        planned.disposition = Disposition::Omitted { reason: OmissionReason::VerdictExcluded { cluster_id: cluster_id.clone() } };
                }
                if shared_modified
                    && matches!(
                        planned.disposition,
                        Disposition::Accepted
                            | Disposition::Transformed { .. }
                            | Disposition::Omitted {
                                reason: OmissionReason::AlreadyMigrated
                            }
                    )
                {
                    planned.disposition = Disposition::Unresolved {
                        reason: UnresolvedReason::TargetModified,
                    };
                }
                batch.push(planned);
            }
            let projected: BTreeMap<_, _> = batch
                .iter()
                .filter(|planned| dedup_eligible(&planned.disposition))
                .map(|planned| (planned.record.canonical_id.clone(), planned.record.clone()))
                .collect();
            for planned in &mut batch {
                if planned.previous_write.is_none()
                    && dedup_eligible(&planned.disposition)
                    && let Some(reference) = &planned.duplicate_write
                {
                    let in_batch = projected.contains_key(&reference.canonical_id);
                    let representative =
                        if let Some(record) = projected.get(&reference.canonical_id) {
                            record.clone()
                        } else {
                            writer
                                .inspect(&reference.prior_write.target_id)?
                                .context("Duplicate target disappeared")?
                        };
                    if duplicate_key(&representative)? == duplicate_key(&planned.record)? {
                        if !in_batch {
                            planned.disposition = Disposition::Omitted {
                                reason: OmissionReason::DuplicateOf {
                                    canonical_id: reference.canonical_id.clone(),
                                },
                            };
                        }
                    } else {
                        planned.duplicate_write = None;
                        if writer.inspect(&planned.target_id)?.is_some() {
                            planned.disposition = Disposition::Unresolved {
                                reason: UnresolvedReason::TargetUntracked,
                            };
                        }
                    }
                }
            }
            let mut groups: BTreeMap<_, Vec<usize>> = BTreeMap::new();
            for (index, planned) in batch.iter().enumerate() {
                if dedup_eligible(&planned.disposition) {
                    groups
                        .entry(duplicate_key(&planned.record)?)
                        .or_default()
                        .push(index);
                }
            }
            for indices in groups.values() {
                let representative = *indices
                    .iter()
                    .min_by_key(|&&index| {
                        (
                            batch[index]
                                .previous_write
                                .as_ref()
                                .is_none_or(|prior| prior.verification != Verification::Verified),
                            !matches!(
                                batch[index].disposition,
                                Disposition::Omitted {
                                    reason: OmissionReason::AlreadyMigrated
                                }
                            ),
                            &batch[index].record.canonical_id,
                        )
                    })
                    .unwrap();
                let id = batch[representative].record.canonical_id.clone();
                for &index in indices {
                    if index != representative {
                        batch[index].disposition = Disposition::Omitted {
                            reason: OmissionReason::DuplicateOf {
                                canonical_id: id.clone(),
                            },
                        };
                        if batch[index]
                            .duplicate_write
                            .as_ref()
                            .is_some_and(|reference| reference.canonical_id != id)
                        {
                            batch[index].duplicate_write = None;
                        }
                    }
                }
            }
            for planned in &batch {
                let record = &records[&planned.record.canonical_id];
                let (field_map, unmapped) = &mappings[&record.canonical_id];
                entries.push(PlanEntry {
                    canonical_id: record.canonical_id.clone(),
                    source_record_id: record.source_record_id.clone(),
                    source_locator: record.source_locator.clone(),
                    content_hash: record.content_hash.clone(),
                    target: target.clone(),
                    disposition: planned.disposition.clone(),
                    field_map: field_map.clone(),
                    target_map: planned.target_map.clone(),
                    unmapped: unmapped.clone(),
                    sensitive_findings: record.sensitive_findings.clone().unwrap_or_default(),
                    evidence_level: record.evidence_level,
                    content_preview: None,
                    reembed_plan: planned.record.reembed_plan.clone(),
                    prior_write: planned.previous_write.clone(),
                    duplicate_write: planned.duplicate_write.clone(),
                });
                digest_records
                    .get_mut(&record.canonical_id)
                    .unwrap()
                    .predictions
                    .push(Prediction {
                        target: target.clone(),
                        disposition: planned.disposition.clone(),
                        target_map: planned.target_map.clone(),
                        prior_write: planned.previous_write.clone(),
                        duplicate_write: planned.duplicate_write.clone(),
                    });
            }
            batches.insert(target.clone(), batch);
        }
        ensure!(!targets.is_empty(), "At least one target is required");
        let writers: Vec<_> = writers.into_values().collect();
        // BTreeMap iteration orders record IDs and target/writer keys; report run IDs and times stay outside the digest.
        let digest_inputs = DigestInputs {
            records: digest_records.into_values().collect(),
            targets: targets.clone(),
            writers: writers.clone(),
            gate_policy: gate_policy.clone(),
        };
        let created_at = timestamp()?;
        let systems: BTreeSet<_> = records
            .values()
            .map(|record| record.source.system.as_str())
            .collect();
        let source_system = match systems.len() {
            0 => {
                if adapters.len() == 1 {
                    adapters[0].id.as_str()
                } else if adapters.is_empty() {
                    "unknown"
                } else {
                    "mixed"
                }
            }
            1 => systems.first().unwrap(),
            _ => "mixed",
        }
        .to_owned();
        let files: Vec<_> = inventory.into_values().collect();
        let mut report = PlanReport {
            schema_version: SCHEMA_VERSION.into(),
            canonical_model_version: CANONICAL_MODEL_VERSION.into(),
            run_id: format!("plan-{}", OffsetDateTime::now_utc().unix_timestamp_nanos()),
            created_at,
            source: SourceSpec {
                location: source.root.to_string_lossy().into_owned(),
                system: source_system.clone(),
                export_version: "unknown".into(),
                adapters,
            },
            source_inventory: SourceInventory {
                state: if records.is_empty() {
                    InventoryState::Empty
                } else {
                    InventoryState::DataPresent
                },
                files: files.clone(),
            },
            targets,
            writers,
            model_calls: vec![],
            gate_policy,
            entries,
            source_unavailable,
            anomalies,
            warnings,
            bundle_manifest: BundleManifest {
                source_system,
                export_version: "unknown".into(),
                files: files
                    .into_iter()
                    .map(|file| ManifestFile {
                        path: file.path,
                        content_hash: file.content_hash,
                        bytes: file.bytes,
                    })
                    .collect(),
                exported_at: None,
            },
            plan_digest: plan_digest(&digest_inputs)?,
            digest_inputs,
            previous_receipt_ref: previous_ref.map(|path| path.to_string_lossy().into_owned()),
        };
        let mut public = serde_json::to_value(&report)?;
        crate::gate::mask_value(&mut public);
        report = serde_json::from_value(public)?;
        report.plan_digest = plan_digest(&report.digest_inputs)?;
        info!(
            records = records.len(),
            targets = report.targets.len(),
            "[S6] dry-run plan complete; target unchanged"
        );
        crate::schema::validate("plan-report", &report)?;
        Ok(PipelinePlan {
            report,
            batches,
            previous,
        })
    }

    /// Recomputes and checks the approved execution basis before writing eligible records.
    /// Checks Writer-produced byte proofs and supported-field read-back, then returns a schema-valid receipt.
    /// Later write/read-back errors may follow target changes; mismatches remain explicit rather than verified.
    /// The caller saves the receipt; this function provides neither whole-run rollback nor an unverifiable fallback.
    pub fn apply(
        &self,
        approved: &PlanReport,
        approval: &ApprovalReceipt,
        plan_ref: String,
        approval_ref: String,
    ) -> Result<ReceiptReport> {
        info!("[S7] checking approval and recomputing plan");
        crate::schema::validate("plan-report", approved)?;
        crate::schema::validate("approval-receipt", approval)?;
        ensure!(
            approval.schema_version == SCHEMA_VERSION,
            "Unsupported approval schema version"
        );
        ensure!(
            approval.plan_digest == approved.plan_digest,
            "Approval digest mismatch"
        );
        ensure!(
            plan_digest(&approved.digest_inputs)? == approved.plan_digest,
            "Plan digest mismatch"
        );
        // Re-read execution inputs and reject a stale approval before invoking any Writer write.
        let current = self.prepare(
            Path::new(&approved.source.location),
            approved.gate_policy.clone(),
            approved.previous_receipt_ref.as_deref().map(Path::new),
        )?;
        ensure!(
            current.report.plan_digest == approved.plan_digest,
            "Plan digest mismatch: source or plan changed"
        );
        ensure!(
            current.report.bundle_manifest == approved.bundle_manifest,
            "Source manifest mismatch: source files changed"
        );
        ensure!(
            current.report.entries == approved.entries,
            "Plan entries mismatch"
        );
        ensure!(
            current.report.targets == approved.targets,
            "Target artifacts changed after approval"
        );
        let mut token = WriteToken {
            batches: BTreeMap::new(),
            artifacts: BTreeMap::new(),
        };
        for target in &approved.targets {
            let writer = self.registry.writer(&target.id)?;
            token.artifacts.insert(
                format!("{}:{}", writer.id(), writer.location().display()),
                target.artifacts.clone(),
            );
        }
        for (target, batch) in &current.batches {
            let writer = self.registry.writer(target)?;
            token.batches.insert(
                format!("{}:{}", writer.id(), writer.location().display()),
                batch_hash(batch)?,
            );
        }
        info!("[S7] approval checked; write capability issued");
        let mut entries = Vec::new();
        let mut output_artifacts: BTreeMap<String, Vec<TargetArtifact>> = BTreeMap::new();
        for (target, batch) in current.batches {
            let writer = self.registry.writer(&target)?;
            let approved_target = approved
                .targets
                .iter()
                .find(|item| item.id == target)
                .unwrap();
            ensure!(
                writer.artifacts(
                    &batch
                        .iter()
                        .map(|planned| planned.target_id.clone())
                        .collect::<Vec<_>>()
                )? == approved_target.artifacts,
                "Target artifacts changed before write"
            );
            let writable: Vec<_> = batch
                .into_iter()
                .filter(|planned| {
                    matches!(
                        planned.disposition,
                        Disposition::Accepted | Disposition::Transformed { .. }
                    )
                })
                .collect();
            // Bind the capability to the exact subset that is permitted to leave the engine.
            token.batches.insert(
                format!("{}:{}", writer.id(), writer.location().display()),
                batch_hash(&writable)?,
            );
            info!(records = writable.len(), "[S8] writing approved batch");
            let result = writer.write(&writable, &token)?;
            let written = result.written;
            ensure!(
                written.len() == writable.len(),
                "Writer receipt count mismatch"
            );
            let mut paths = BTreeSet::new();
            for artifact in &result.artifacts {
                ensure!(
                    paths.insert(&artifact.path)
                        && artifact.content_hash.is_some()
                        && artifact.bytes.is_some(),
                    "Invalid Writer output proof"
                );
                ensure!(
                    crate::writer::artifact(writer.location(), &artifact.path)? == *artifact,
                    "Writer output changed before verification"
                );
            }
            if !written.is_empty() {
                ensure!(
                    writer.shared_artifact_paths().iter().all(|path| result
                        .artifacts
                        .iter()
                        .any(|artifact| artifact.path == *path)),
                    "Writer shared output proof is missing"
                );
            }
            output_artifacts.insert(target.clone(), result.artifacts);
            info!(records = written.len(), "[S9] reading target back");
            let read_back: BTreeMap<_, _> = writer
                .read_back(&written)?
                .into_iter()
                .map(|record| (record.canonical_id, record.record))
                .collect();
            for (planned, written) in writable.into_iter().zip(written) {
                ensure!(
                    written.canonical_id == planned.record.canonical_id,
                    "Writer identity mismatch"
                );
                ensure!(
                    writer.target_hash(&written.target_id)?.as_ref() == Some(&written.target_hash),
                    "Written native payload changed before verification"
                );
                let actual = read_back
                    .get(&written.canonical_id)
                    .context("Missing read-back record")?;
                let expected = serde_json::to_value(&planned.record)?;
                let actual_hash = record_hash(actual)?;
                let actual_content_hash = content_hash(actual.content.as_bytes());
                let actual = serde_json::to_value(actual)?;
                let supported = writer.capabilities().supported_fields;
                let diff: Vec<_> = supported
                    .iter()
                    .filter(|path| expected.pointer(path) != actual.pointer(path))
                    .map(|path| FieldDiff {
                        field_path: path.clone(),
                        kind: DiffKind::Changed,
                        expected_hash: None,
                        actual_hash: None,
                    })
                    .collect();
                let verification = if diff.is_empty() {
                    Verification::Verified
                } else {
                    Verification::Mismatch { diff }
                };
                let prior_write = PriorWrite {
                    target_id: written.target_id.clone(),
                    content_hash: actual_content_hash,
                    record_hash: actual_hash,
                    target_hash: written.target_hash,
                    verification: verification.clone(),
                };
                entries.push(ReceiptEntry {
                    canonical_id: planned.record.canonical_id,
                    source_record_id: planned.record.source_record_id,
                    source_locator: planned.record.source_locator,
                    content_hash: planned.record.content_hash,
                    target: target.clone(),
                    disposition: planned.disposition,
                    target_map: planned.target_map,
                    sensitive_findings: planned.record.sensitive_findings.unwrap_or_default(),
                    evidence_level: planned.record.evidence_level,
                    target_id: Some(written.target_id),
                    verification: Some(verification),
                    prior_write: Some(prior_write),
                    duplicate_write: None,
                });
            }
        }
        for entry in &current.report.entries {
            if !matches!(
                entry.disposition,
                Disposition::Accepted | Disposition::Transformed { .. }
            ) {
                let duplicate_write = if let Disposition::Omitted {
                    reason: OmissionReason::DuplicateOf { canonical_id },
                } = &entry.disposition
                {
                    entries
                        .iter()
                        .find(|written| {
                            written.target == entry.target && written.canonical_id == *canonical_id
                        })
                        .and_then(|written| written.prior_write.clone())
                        .or_else(|| {
                            current
                                .report
                                .entries
                                .iter()
                                .find(|representative| {
                                    representative.target == entry.target
                                        && representative.canonical_id == *canonical_id
                                })
                                .and_then(|representative| representative.prior_write.clone())
                        })
                        .map(|prior_write| DuplicateWrite {
                            canonical_id: canonical_id.clone(),
                            prior_write,
                        })
                        .or_else(|| entry.duplicate_write.clone())
                } else {
                    entry.duplicate_write.clone()
                };
                entries.push(ReceiptEntry {
                    canonical_id: entry.canonical_id.clone(),
                    source_record_id: entry.source_record_id.clone(),
                    source_locator: entry.source_locator.clone(),
                    content_hash: entry.content_hash.clone(),
                    target: entry.target.clone(),
                    disposition: entry.disposition.clone(),
                    target_map: entry.target_map.clone(),
                    sensitive_findings: entry.sensitive_findings.clone(),
                    evidence_level: entry.evidence_level,
                    target_id: None,
                    verification: None,
                    prior_write: entry.prior_write.clone(),
                    duplicate_write,
                });
            }
        }
        info!(records = entries.len(), "[S9] receipt complete");
        if let Some(previous) = &current.previous {
            for old in &previous.entries {
                if self.registry.writers.contains_key(&old.target)
                    && !entries.iter().any(|entry| {
                        entry.target == old.target && entry.canonical_id == old.canonical_id
                    })
                {
                    let mut carried = old.clone();
                    carried.disposition = Disposition::Omitted {
                        reason: OmissionReason::SourceDeleted,
                    };
                    carried.verification = None;
                    entries.push(carried);
                }
            }
        }
        entries.sort_by(|a, b| (&a.target, &a.canonical_id).cmp(&(&b.target, &b.canonical_id)));
        let mut actual_targets = current.report.targets;
        for target in &mut actual_targets {
            let writer = self.registry.writer(&target.id)?;
            target.artifacts = writer.artifacts(
                &entries
                    .iter()
                    .filter(|entry| entry.target == target.id)
                    .filter_map(|entry| {
                        entry
                            .prior_write
                            .as_ref()
                            .or_else(|| {
                                entry
                                    .duplicate_write
                                    .as_ref()
                                    .map(|reference| &reference.prior_write)
                            })
                            .map(|prior| prior.target_id.clone())
                    })
                    .collect::<Vec<_>>(),
            )?;
            for artifact in &output_artifacts[&target.id] {
                ensure!(
                    crate::writer::artifact(writer.location(), &artifact.path)? == *artifact,
                    "Writer output changed before receipt"
                );
                if let Some(observed) = target
                    .artifacts
                    .iter_mut()
                    .find(|observed| observed.path == artifact.path)
                {
                    *observed = artifact.clone();
                } else {
                    target.artifacts.push(artifact.clone());
                }
            }
            target.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
            if !entries.iter().any(|entry| {
                entry.target == target.id && entry.verification == Some(Verification::Verified)
            }) && let Some(old) = current
                .previous
                .as_ref()
                .and_then(|receipt| receipt.targets.iter().find(|old| old.id == target.id))
            {
                for path in writer.shared_artifact_paths() {
                    target.artifacts.retain(|artifact| artifact.path != *path);
                    if let Some(artifact) =
                        old.artifacts.iter().find(|artifact| artifact.path == *path)
                    {
                        target.artifacts.push(artifact.clone());
                    }
                }
                target.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
            }
        }
        let mut report = ReceiptReport {
            schema_version: SCHEMA_VERSION.into(),
            canonical_model_version: CANONICAL_MODEL_VERSION.into(),
            run_id: format!(
                "receipt-{}",
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
            created_at: timestamp()?,
            source: current.report.source,
            targets: actual_targets,
            writers: current.report.writers,
            model_calls: current.report.model_calls,
            gate_policy: current.report.gate_policy,
            bundle_manifest: current.report.bundle_manifest,
            plan_digest: approved.plan_digest.clone(),
            plan_ref,
            approval_receipt_ref: approval_ref,
            entries,
            verdicts: current
                .previous
                .as_ref()
                .map(|previous| previous.verdicts.clone())
                .unwrap_or_default(),
            previous_receipt_ref: approved.previous_receipt_ref.clone(),
        };
        let mut public = serde_json::to_value(&report)?;
        crate::gate::mask_value(&mut public);
        report = serde_json::from_value(public)?;
        crate::schema::validate("receipt-report", &report)?;
        Ok(report)
    }
}

fn audit_fields(output: &mut ReaderOutput) {
    fn paths(value: &serde_json::Value, prefix: &str, output: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(object) if !object.is_empty() => {
                for (key, value) in object {
                    paths(
                        value,
                        &format!("{prefix}/{}", key.replace('~', "~0").replace('/', "~1")),
                        output,
                    );
                }
            }
            serde_json::Value::Array(array) if !array.is_empty() => {
                for (index, value) in array.iter().enumerate() {
                    paths(value, &format!("{prefix}/{index}"), output);
                }
            }
            _ => output.push(prefix.into()),
        }
    }
    for source in &mut output.source_records {
        let mut fields = Vec::new();
        paths(&source.fields, "", &mut fields);
        for field in fields {
            let covered = source
                .field_map
                .iter()
                .map(|mapping| &mapping.source_path)
                .chain(source.unmapped.iter().map(|unmapped| &unmapped.source_path))
                .any(|path| field == *path || field.starts_with(&format!("{path}/")));
            if !covered {
                source.unmapped.push(UnmappedField {
                    source_path: field.clone(),
                    reason: UnmappedReason::EngineUnreported,
                });
                output.anomalies.push(Anomaly {
                    source_locator: source.source_locator.clone(),
                    code: "reader_unreported_field".into(),
                    line: None,
                    field_path: Some(field),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks repeatability, concatenation separation, and the fixed lowercase base32 ID shape.
    #[test]
    fn identities_are_stable_and_delimited() {
        assert_eq!(
            canonical_id("markdown", "a.md"),
            canonical_id("markdown", "a.md")
        );
        assert_ne!(canonical_id("ab", "c"), canonical_id("a", "bc"));
        assert_eq!(canonical_id("markdown", "a.md").len(), 32);
        assert!(
            canonical_id("markdown", "a.md")
                .chars()
                .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
        );
    }

    /// Checks the underlying JCS library's UTF-16 key ordering and number formatting, not our safe-number gate.
    #[test]
    fn jcs_orders_keys_by_utf16_and_normalizes_numbers() {
        let value = serde_json::json!({"\u{e000}": -0.0, "\u{1f600}": 1.0, "a": 1e30});
        assert_eq!(
            String::from_utf8(serde_jcs::to_vec(&value).unwrap()).unwrap(),
            "{\"a\":1e+30,\"😀\":1,\"\":0}"
        );
    }

    #[test]
    fn omitted_source_fields_are_reported_by_engine() {
        let mut output = ReaderOutput {
            source_records: vec![SourceRecord {
                canonical_id: canonical_id("synthetic", "synthetic"),
                source_record_id: "synthetic".into(),
                source_locator: "example.json".into(),
                fields: serde_json::json!({"content": "synthetic", "unknown": {"a/b": 1}}),
                field_map: vec![FieldMapping {
                    source_path: "/content".into(),
                    canonical_path: "/content".into(),
                    rule: None,
                }],
                unmapped: vec![],
            }],
            records: vec![],
            anomalies: vec![],
            registered_count: 0,
            deleted_count: 0,
            source_unavailable: vec![],
        };
        audit_fields(&mut output);
        assert_eq!(
            output.source_records[0].unmapped[0].source_path,
            "/unknown/a~1b"
        );
        assert_eq!(
            output.source_records[0].unmapped[0].reason,
            UnmappedReason::EngineUnreported
        );
        assert_eq!(output.anomalies[0].code, "reader_unreported_field");
    }

    #[test]
    fn one_records_mappings_cannot_cover_another_records_omissions() {
        let mut output = crate::reader::output();
        let first = crate::reader::record(
            "synthetic",
            "test",
            "first",
            "first.json",
            "",
            crate::canonical::EvidenceLevel::Measured,
        );
        let second = crate::reader::record(
            "synthetic",
            "test",
            "second",
            "second.json",
            "",
            crate::canonical::EvidenceLevel::Measured,
        );
        let mut mapped = crate::reader::source(
            &first,
            "first.json",
            serde_json::json!({"metadata": "first"}),
        );
        crate::reader::map(&mut mapped, "/metadata", "/source_extra");
        output.source_records = vec![
            mapped,
            crate::reader::source(
                &second,
                "second.json",
                serde_json::json!({"metadata": "second"}),
            ),
        ];
        audit_fields(&mut output);
        assert!(output.source_records[0].unmapped.is_empty());
        assert_eq!(
            output.source_records[1].unmapped[0].source_path,
            "/metadata"
        );
        assert_eq!(output.anomalies[0].source_locator, "second.json");
    }
}
