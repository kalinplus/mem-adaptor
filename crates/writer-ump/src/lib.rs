//! Writer for Universal Memory Protocol JSON file targets.
//! Uses shared fixed-path and regular-file checks at the local target boundary.
//! Approval and read-back remain engine responsibilities; replacement is per file, not a whole-run transaction.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::{CanonicalRecord, Scope};
use mem_adaptor_core::engine::{content_hash, record_hash, timestamp};
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

pub struct UmpWriter {
    location: PathBuf,
}

impl UmpWriter {
    /// Fixes the physical target location before approval and refuses an explicitly linked root.
    pub fn new(location: PathBuf) -> Result<Self> {
        Ok(Self {
            location: target::normalize_root(&location)?,
        })
    }

    fn native_records(&self) -> Result<Vec<Value>> {
        let Some(bytes) = target::read_file(&self.location, FILE)? else {
            return Ok(vec![]);
        };
        parse_records(&bytes)
    }
}

fn parse_records(bytes: &[u8]) -> Result<Vec<Value>> {
    let records: Vec<Value> =
        serde_json::from_slice(bytes).map_err(|_| anyhow::anyhow!("Invalid UMP target array"))?;
    let mut identities = std::collections::BTreeSet::new();
    for record in &records {
        mem_adaptor_core::jcs::validate_numbers(record)?;
        ensure!(
            VALIDATOR.is_valid(record),
            "UMP target record fails official schema"
        );
        ensure!(
            identities.insert(record["id"].as_str().unwrap()),
            "Duplicate UMP target identity"
        );
    }
    Ok(records)
}

pub fn kind(record: &CanonicalRecord) -> &'static str {
    match record.source_kind.as_deref() {
        Some("profile" | "identity") => "identity",
        Some("instruction" | "procedural") => "procedural",
        Some("episodic") => "episodic",
        Some("working") => "working",
        _ => "semantic",
    }
}

fn encode(record: &CanonicalRecord, created: &str, created_origin: &str) -> Result<Value> {
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
    ensure!(
        VALIDATOR.is_valid(&native),
        "Generated UMP record fails official schema"
    );
    Ok(native)
}

fn decode(native: &Value) -> Result<CanonicalRecord> {
    ensure!(
        VALIDATOR.is_valid(native),
        "UMP target record fails official schema"
    );
    let mut metadata = native
        .pointer("/body/structured/mem_adaptor")
        .context("UMP target lacks migration metadata")?
        .clone();
    metadata["content"] = native["body"]["text"].clone();
    let record: CanonicalRecord = serde_json::from_value(metadata)
        .map_err(|_| anyhow::anyhow!("Invalid UMP migration metadata"))?;
    mem_adaptor_core::schema::validate("canonical-record", &record)?;
    ensure!(
        native["id"] == format!("urn:ump:{}", record.canonical_id),
        "UMP bridge identity mismatch"
    );
    Ok(record)
}

impl Writer for UmpWriter {
    fn id(&self) -> &'static str {
        "ump"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn location(&self) -> &Path {
        &self.location
    }
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
                target::mapping(
                    "/source_kind",
                    "/kind",
                    &format!("conservative_kind_{}", kind(record)),
                ),
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
            let position = records
                .iter()
                .position(|record| record["id"] == planned.target_id);
            let old_created = if let Some(prior) = &planned.previous_write {
                ensure!(
                    prior.target_id == planned.target_id,
                    "UMP update identity mismatch"
                );
                let position = position.context("UMP target was deleted after approval")?;
                ensure!(
                    content_hash(&mem_adaptor_core::jcs::to_vec(&records[position])?)
                        == prior.target_hash,
                    "UMP target payload changed after approval"
                );
                ensure!(
                    record_hash(&decode(&records[position])?)? == prior.record_hash,
                    "UMP target metadata changed after approval"
                );
                Some(
                    records[position]["time"]["created"]
                        .as_str()
                        .unwrap()
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
                    records[position]
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
            if let Some(position) = position {
                records[position] = native;
            } else {
                records.push(native);
            }
            written.push(Written {
                canonical_id: planned.record.canonical_id.clone(),
                target_id: planned.target_id.clone(),
                target_hash,
            });
        }
        records.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let bytes = serde_json::to_vec_pretty(&records)?;
        target::atomic_file(&self.location, FILE, &bytes, previous_bytes.as_deref())?;
        Ok(WriteResult {
            written,
            artifacts: vec![target::output_artifact(FILE, &bytes)],
        })
    }
    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>> {
        written
            .iter()
            .map(|written| {
                Ok(ReadBack {
                    canonical_id: written.canonical_id.clone(),
                    record: self
                        .inspect(&written.target_id)?
                        .context("UMP record missing on read-back")?,
                })
            })
            .collect()
    }
    fn inspect(&self, target_id: &str) -> Result<Option<CanonicalRecord>> {
        self.native_records()?
            .iter()
            .find(|record| record["id"] == target_id)
            .map(decode)
            .transpose()
    }
    fn target_hash(&self, target_id: &str) -> Result<Option<String>> {
        self.native_records()?
            .iter()
            .find(|record| record["id"] == target_id)
            .map(|record| Ok(content_hash(&mem_adaptor_core::jcs::to_vec(record)?)))
            .transpose()
    }
    fn artifacts(&self, _target_ids: &[String]) -> Result<Vec<TargetArtifact>> {
        Ok(vec![target::artifact(&self.location, FILE)?])
    }
    fn shared_artifact_paths(&self) -> &'static [&'static str] {
        &[FILE]
    }
}
