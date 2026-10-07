//! Writer for Universal Memory Protocol JSON file targets.
//! Every observation rejects ambiguous raw JSON and validates all explicitly managed bridges.
//! Bulk observations use one phase-local parse and ID index; foreign native records remain unowned.
//! Approval and read-back remain engine responsibilities; replacement is per file, not a whole-run transaction.

mod json;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::{CanonicalRecord, Scope};
use mem_adaptor_core::engine::{canonical_id, content_hash, record_hash, timestamp};
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::*;
use mem_adaptor_core::writer as target;
use serde_json::{Value, json};

pub const SCHEMA: &str = include_str!("../schema/ump-record.schema.json");
static VALIDATOR: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    jsonschema::options()
        .should_validate_formats(true)
        .build(&serde_json::from_str::<Value>(SCHEMA).unwrap())
        .unwrap()
});
const FILE: &str = "records.ump.json";

#[cfg(test)]
thread_local! {
    static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub struct UmpWriter {
    location: PathBuf,
}

struct NativeRecord {
    value: Value,
    record: Option<CanonicalRecord>,
}

#[derive(Default)]
struct NativeRecords {
    records: Vec<NativeRecord>,
    by_id: BTreeMap<String, usize>,
}

impl NativeRecords {
    /// Looks up an already validated native record without rescanning the target array.
    fn get(&self, target_id: &str) -> Option<&NativeRecord> {
        self.by_id.get(target_id).map(|index| &self.records[*index])
    }
}

impl NativeRecord {
    /// Hashes the complete native payload, including fields outside the canonical bridge.
    fn target_hash(&self) -> Result<String> {
        Ok(content_hash(&mem_adaptor_core::jcs::to_vec(&self.value)?))
    }
}

impl UmpWriter {
    /// Fixes the physical target location before approval and refuses an explicitly linked root.
    pub fn new(location: PathBuf) -> Result<Self> {
        Ok(Self {
            location: target::normalize_root(&location)?,
        })
    }

    /// Reads and validates the complete array once for this observation, without retaining a cross-phase cache.
    fn native_records(&self) -> Result<NativeRecords> {
        let Some(bytes) = target::read_file(&self.location, FILE)? else {
            return Ok(NativeRecords::default());
        };
        parse_records(&bytes)
    }
}

/// Rejects duplicate members and IDs, unsafe numbers, and every broken managed record before returning an index.
fn parse_records(bytes: &[u8]) -> Result<NativeRecords> {
    #[cfg(test)]
    PARSES.with(|count| count.set(count.get() + 1));
    let Value::Array(values) = json::parse(bytes)? else {
        anyhow::bail!("Invalid UMP target array");
    };
    let mut records = NativeRecords::default();
    for value in values {
        mem_adaptor_core::jcs::validate_numbers(&value)?;
        ensure!(
            VALIDATOR.is_valid(&value),
            "UMP target record fails official schema"
        );
        let id = value["id"]
            .as_str()
            .context("UMP target record lacks identity")?;
        ensure!(
            records
                .by_id
                .insert(id.to_owned(), records.records.len())
                .is_none(),
            "Duplicate UMP target identity"
        );
        let record = if value.pointer("/body/structured/mem_adaptor").is_some() {
            Some(decode(&value)?)
        } else {
            None
        };
        records.records.push(NativeRecord { value, record });
    }
    Ok(records)
}

/// Conservatively maps declared kinds without inferring a category from body text or dates.
pub fn kind(record: &CanonicalRecord) -> &'static str {
    match record.source_kind.as_deref() {
        Some("profile" | "identity") => "identity",
        Some("instruction" | "procedural") => "procedural",
        Some("episodic") => "episodic",
        Some("working") => "working",
        _ => "semantic",
    }
}

/// Explains known mappings and distinguishes unknown from missing categories without echoing either value.
fn kind_rule(record: &CanonicalRecord) -> String {
    match record.source_kind.as_deref() {
        None => "default_kind_semantic_missing_source_kind".into(),
        Some(
            "profile" | "identity" | "instruction" | "procedural" | "episodic" | "working"
            | "preference" | "project" | "tool" | "project_doc" | "semantic",
        ) => format!("conservative_kind_{}", kind(record)),
        Some(_) => "default_kind_semantic_unknown_source_kind".into(),
    }
}

/// Checks canonical shape and independently re-derives source identity, body integrity and vector dimensions.
fn validate_bridge(record: &CanonicalRecord) -> Result<()> {
    mem_adaptor_core::schema::validate("canonical-record", record)
        .map_err(|_| anyhow::anyhow!("Invalid UMP migration metadata schema"))?;
    ensure!(
        record.canonical_id == canonical_id(&record.source.system, &record.source_record_id),
        "UMP bridge source identity mismatch"
    );
    ensure!(
        record.content_hash == content_hash(record.content.as_bytes()),
        "UMP bridge body hash mismatch"
    );
    if let Some(embedding) = &record.embedding
        && let Some(vector) = &embedding.vector
    {
        ensure!(
            vector.len() as u64 == embedding.dim,
            "UMP bridge vector length does not match dimension"
        );
    }
    Ok(())
}

/// Projects validated canonical metadata and native fields; creation time describes the first target creation.
fn project(record: &CanonicalRecord, created: &str, created_origin: &str) -> Result<Value> {
    let mut metadata = serde_json::to_value(record)?;
    metadata.as_object_mut().unwrap().remove("content");
    let owner = format!(
        "mem-adaptor:{}",
        content_hash(
            format!(
                "{}\0{}",
                record.source.system,
                record.scope_qualifier.as_deref().unwrap_or("user")
            )
            .as_bytes()
        )
    );
    let mut native = json!({
        "ump": "1.0", "id": format!("urn:ump:{}", record.canonical_id), "kind": kind(record),
        "body": {"text": record.content, "structured": {"mem_adaptor": metadata, "created_origin": created_origin}},
        "scope": {"owner": owner, "visibility": "private"}, "time": {"created": created},
        "provenance": {"actor": "process:mem-adaptor", "actor_kind": "import", "method": "local_migration", "source": {"ref": record.source_locator}}
    });
    if let Some(address) = &record.scope_qualifier {
        let field = match record.scope {
            Scope::User => Some("user"),
            Scope::Project => Some("project"),
            Scope::Agent => Some("agent"),
            Scope::Session => Some("session"),
            _ => None,
        };
        if let Some(field) = field {
            native["scope"][field] = Value::String(address.clone());
        }
    }
    for (field, value) in [
        ("observed", &record.observed_at),
        ("valid_from", &record.valid_from),
        ("valid_to", &record.valid_to),
    ] {
        if let Some(value) = value {
            native["time"][field] = Value::String(value.clone());
        }
    }
    if let Some(consent) = &record.consent {
        let mut value = json!({});
        if let Some(exportable) = consent.exportable {
            value["exportable"] = Value::Bool(exportable);
        }
        if let Some(retention) = &consent.retention {
            value["retention"] = Value::String(retention.clone());
        }
        if let Some(redact) = &consent.redact {
            value["redact"] = json!(redact);
        }
        native["consent"] = value;
    }
    Ok(native)
}

/// Generates a schema-valid native record without silently inventing bridge integrity or creation provenance.
fn encode(record: &CanonicalRecord, created: &str, created_origin: &str) -> Result<Value> {
    validate_bridge(record)?;
    ensure!(
        matches!(created_origin, "source_record" | "target_migration"),
        "Invalid UMP target creation origin"
    );
    let native = project(record, created, created_origin)?;
    ensure!(
        VALIDATOR.is_valid(&native),
        "Generated UMP record fails official schema"
    );
    Ok(native)
}

/// Decodes an officially validated managed record and rejects native projections contradicting its bridge.
/// The first created time and its origin are preserved independently of later source-created-time corrections.
fn decode(native: &Value) -> Result<CanonicalRecord> {
    let mut metadata = native
        .pointer("/body/structured/mem_adaptor")
        .context("UMP target lacks migration metadata")?
        .as_object()
        .context("UMP migration metadata must be an object")?
        .clone();
    ensure!(
        !metadata.contains_key("content"),
        "UMP migration metadata must not duplicate the native body"
    );
    metadata.insert("content".into(), native["body"]["text"].clone());
    // Validate raw metadata first so explicit null or malformed optional fields cannot disappear during decoding.
    mem_adaptor_core::schema::validate("canonical-record", &Value::Object(metadata.clone()))
        .map_err(|_| anyhow::anyhow!("Invalid UMP migration metadata schema"))?;
    let record: CanonicalRecord = serde_json::from_value(Value::Object(metadata))
        .map_err(|_| anyhow::anyhow!("Invalid UMP migration metadata"))?;
    validate_bridge(&record)?;
    ensure!(
        native["id"] == format!("urn:ump:{}", record.canonical_id),
        "UMP bridge identity mismatch"
    );
    let created = native["time"]["created"]
        .as_str()
        .context("Invalid UMP target creation time")?;
    let origin = native
        .pointer("/body/structured/created_origin")
        .and_then(Value::as_str)
        .context("Invalid UMP target creation origin")?;
    ensure!(
        matches!(origin, "source_record" | "target_migration"),
        "Invalid UMP target creation origin"
    );
    let expected = project(&record, created, origin)?;
    for field in ["kind", "scope", "consent", "time", "provenance"] {
        ensure!(
            native.get(field) == expected.get(field),
            "UMP native projection contradicts migration metadata"
        );
    }
    Ok(record)
}

impl Writer for UmpWriter {
    /// Identifies this local UMP file adapter in plans and approvals.
    fn id(&self) -> &'static str {
        "ump"
    }
    /// Records the actual Writer package version rather than the native UMP format version.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    /// Exposes the target root fixed before approval.
    fn location(&self) -> &Path {
        &self.location
    }
    /// Declares canonical preservation through the UMP extension slot and verified update support.
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supported_fields: target::fields(),
            unsupported_fields: vec![],
            read_back: true,
            update: true,
        }
    }
    /// Projects UMP fields without writes, using the shared fallible planning contract.
    fn plan(&self, record: &CanonicalRecord, previous: Option<&ReceiptEntry>) -> Result<Planned> {
        Ok(Planned {
            record: record.clone(),
            disposition: if target::redact_requires_processing(record) {
                Disposition::Rejected {
                    rule: "consent_redact_requires_processing".into(),
                }
            } else {
                Disposition::Accepted
            },
            target_id: format!("urn:ump:{}", record.canonical_id),
            previous_write: previous.and_then(|entry| entry.prior_write.clone()),
            duplicate_write: None,
            target_map: vec![
                target::mapping("/canonical_id", "/id", "urn_ump_reversible_base32"),
                target::mapping("/content", "/body/text", "body_bytes_unchanged"),
                target::mapping("/source_kind", "/kind", &kind_rule(record)),
                target::mapping(
                    "/created_at",
                    "/time/created",
                    if previous
                        .and_then(|entry| entry.prior_write.as_ref())
                        .is_some()
                    {
                        "preserve_original_target_created_time_on_update"
                    } else if record.created_at.is_some() {
                        "source_record_created_time"
                    } else {
                        "explicit_target_migration_created_time"
                    },
                ),
                target::mapping(
                    "",
                    "/body/structured/mem_adaptor",
                    "canonical_metadata_without_body",
                ),
                target::mapping(
                    "/owner_declared",
                    "/body/structured/mem_adaptor/owner_declared",
                    "untrusted_declaration_not_authorization",
                ),
            ],
        })
    }
    /// Rechecks approved bytes and all managed records, then replaces one incrementally updated array.
    fn write(
        &self,
        batch: &[Planned],
        token: &mem_adaptor_core::engine::WriteToken,
    ) -> Result<WriteResult> {
        token.authorize(self, batch)?;
        if batch.is_empty() {
            return Ok(WriteResult::default());
        }
        let previous_bytes = target::read_file(&self.location, FILE)?;
        token.authorize_artifact(self, FILE, previous_bytes.as_deref())?;
        let mut records = previous_bytes
            .as_deref()
            .map(parse_records)
            .transpose()?
            .unwrap_or_default();
        let created = timestamp()?;
        let mut written = Vec::new();
        for planned in batch {
            ensure!(
                planned.target_id == format!("urn:ump:{}", planned.record.canonical_id),
                "UMP planned identity mismatch"
            );
            let position = records.by_id.get(&planned.target_id).copied();
            let old_created = if let Some(prior) = &planned.previous_write {
                ensure!(
                    prior.target_id == planned.target_id,
                    "UMP update identity mismatch"
                );
                let position = position.context("UMP target was deleted after approval")?;
                ensure!(
                    records.records[position].target_hash()? == prior.target_hash,
                    "UMP target payload changed after approval"
                );
                ensure!(
                    record_hash(
                        records.records[position]
                            .record
                            .as_ref()
                            .context("UMP update target lacks migration metadata")?
                    )? == prior.record_hash,
                    "UMP target metadata changed after approval"
                );
                Some(
                    records.records[position].value["time"]["created"]
                        .as_str()
                        .context("Invalid UMP target creation time")?
                        .to_owned(),
                )
            } else {
                ensure!(position.is_none(), "UMP target exists without history");
                None
            };
            let time = old_created
                .as_deref()
                .or(planned.record.created_at.as_deref())
                .unwrap_or(&created);
            let origin = position
                .and_then(|position| {
                    records.records[position]
                        .value
                        .pointer("/body/structured/created_origin")
                        .and_then(Value::as_str)
                })
                .unwrap_or(if planned.record.created_at.is_some() {
                    "source_record"
                } else {
                    "target_migration"
                });
            let native = encode(&planned.record, time, origin)?;
            let target_hash = content_hash(&mem_adaptor_core::jcs::to_vec(&native)?);
            let item = NativeRecord {
                value: native,
                record: Some(planned.record.clone()),
            };
            if let Some(position) = position {
                records.records[position] = item;
            } else {
                records
                    .by_id
                    .insert(planned.target_id.clone(), records.records.len());
                records.records.push(item);
            }
            written.push(Written {
                canonical_id: planned.record.canonical_id.clone(),
                target_id: planned.target_id.clone(),
                target_hash,
            });
        }
        let mut native: Vec<_> = records.records.into_iter().map(|item| item.value).collect();
        native.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let bytes = serde_json::to_vec_pretty(&native)?;
        target::atomic_file(&self.location, FILE, &bytes, previous_bytes.as_deref())?;
        Ok(WriteResult {
            written,
            artifacts: vec![target::output_artifact(FILE, &bytes)],
        })
    }
    /// Revalidates the whole target once and resolves the written batch from one phase-local ID index.
    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>> {
        let records = self.native_records()?;
        written
            .iter()
            .map(|written| {
                Ok(ReadBack {
                    canonical_id: written.canonical_id.clone(),
                    record: records
                        .get(&written.target_id)
                        .and_then(|item| item.record.clone())
                        .context("UMP record missing on read-back")?,
                })
            })
            .collect()
    }
    /// Returns only managed canonical records; a foreign record is not adopted even when its ID matches.
    fn inspect(&self, target_id: &str) -> Result<Option<CanonicalRecord>> {
        Ok(self
            .native_records()?
            .get(target_id)
            .and_then(|item| item.record.clone()))
    }
    /// Observes native payload identity independently of whether a record carries our bridge.
    fn target_hash(&self, target_id: &str) -> Result<Option<String>> {
        self.native_records()?
            .get(target_id)
            .map(NativeRecord::target_hash)
            .transpose()
    }
    /// Parses once for a planning batch, retaining foreign hashes but never promoting them to managed records.
    fn inspect_many(&self, target_ids: &[String]) -> Result<BTreeMap<String, TargetState>> {
        let records = self.native_records()?;
        target_ids
            .iter()
            .map(|id| {
                let item = records.get(id);
                Ok((
                    id.clone(),
                    TargetState {
                        record: item.and_then(|item| item.record.clone()),
                        target_hash: item.map(NativeRecord::target_hash).transpose()?,
                    },
                ))
            })
            .collect()
    }
    /// Re-reads and validates once for a native-hash batch without reusing the read-back observation.
    fn target_hashes(&self, target_ids: &[String]) -> Result<BTreeMap<String, Option<String>>> {
        let records = self.native_records()?;
        target_ids
            .iter()
            .map(|id| {
                Ok((
                    id.clone(),
                    records.get(id).map(NativeRecord::target_hash).transpose()?,
                ))
            })
            .collect()
    }
    /// Binds raw target bytes only after all native and managed records have passed preflight validation.
    fn artifacts(&self, _target_ids: &[String]) -> Result<Vec<TargetArtifact>> {
        let bytes = target::read_file(&self.location, FILE)?;
        if let Some(bytes) = &bytes {
            parse_records(bytes)?;
        }
        Ok(vec![TargetArtifact {
            path: FILE.into(),
            content_hash: bytes.as_deref().map(content_hash),
            bytes: bytes.as_ref().map(|bytes| bytes.len() as u64),
        }])
    }
    /// Declares the one shared array artifact required by approval and output-proof verification.
    fn shared_artifact_paths(&self) -> &'static [&'static str] {
        &[FILE]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves each bulk observation parses once for many records and re-reads on the next phase.
    #[test]
    fn bulk_observations_parse_once_and_do_not_cache_across_calls() {
        let directory = tempfile::tempdir().unwrap();
        let writer = UmpWriter::new(directory.path().to_path_buf()).unwrap();
        let natives: Vec<_> = (0..64)
            .map(|i| {
                let record = mem_adaptor_core::reader::record(
                    "synthetic",
                    "test",
                    &i.to_string(),
                    "fixture",
                    &format!("Body {i}"),
                    mem_adaptor_core::canonical::EvidenceLevel::Measured,
                );
                encode(&record, "2026-01-02T03:04:05Z", "target_migration").unwrap()
            })
            .collect();
        std::fs::write(
            writer.location().join(FILE),
            serde_json::to_vec(&natives).unwrap(),
        )
        .unwrap();
        let ids: Vec<_> = natives
            .iter()
            .map(|n| n["id"].as_str().unwrap().to_owned())
            .collect();
        PARSES.with(|count| count.set(0));
        let observed = writer.inspect_many(&ids).unwrap();
        assert_eq!(observed.len(), 64);
        PARSES.with(|count| assert_eq!(count.get(), 1));
        let written: Vec<_> = ids
            .iter()
            .map(|id| Written {
                canonical_id: id.strip_prefix("urn:ump:").unwrap().into(),
                target_id: id.clone(),
                target_hash: "unused".into(),
            })
            .collect();
        assert_eq!(writer.read_back(&written).unwrap().len(), 64);
        PARSES.with(|count| assert_eq!(count.get(), 2));
        assert_eq!(writer.target_hashes(&ids).unwrap().len(), 64);
        PARSES.with(|count| assert_eq!(count.get(), 3));
        std::fs::write(writer.location().join(FILE), b"[]").unwrap();
        assert!(
            writer
                .inspect_many(&ids)
                .unwrap()
                .values()
                .all(|state| state.record.is_none() && state.target_hash.is_none())
        );
        PARSES.with(|count| assert_eq!(count.get(), 4));
    }
}
