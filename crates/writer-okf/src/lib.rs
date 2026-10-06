//! Writer for Open Knowledge Format Markdown homes.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::{ActorKind, CanonicalRecord, ReembedPlan};
use mem_adaptor_core::engine::{WriteToken, record_hash};
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::*;
use mem_adaptor_core::writer as target;
use serde_json::{Value, json};

pub struct OkfWriter {
    location: PathBuf,
}

impl OkfWriter {
    pub fn new(location: PathBuf) -> Self {
        Self { location }
    }

    fn managed_records(&self) -> Result<std::collections::BTreeMap<String, CanonicalRecord>> {
        let mut records = std::collections::BTreeMap::new();
        let entries = match fs::read_dir(self.location.join("memories")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(records),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".md") else {
                continue;
            };
            if id.len() != 32
                || !id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
            {
                continue;
            }
            let relative = format!("memories/{name}");
            let bytes =
                target::read_file(&self.location, &relative)?.context("Target file disappeared")?;
            let text = std::str::from_utf8(&bytes).context("Invalid target Markdown encoding")?;
            let Some(rest) = text.strip_prefix("---\n") else {
                continue;
            };
            let yaml = rest.split_once("---\n").map_or(rest, |(yaml, _)| yaml);
            if !yaml
                .lines()
                .any(|line| line.starts_with("mem_adaptor_envelope:"))
            {
                continue;
            }
            let metadata: Value = serde_saphyr::from_str(yaml)
                .map_err(|_| anyhow::anyhow!("Invalid managed OKF YAML"))?;
            ensure!(
                metadata["mem_adaptor_envelope"] == "okf:0.2",
                "Unsupported managed OKF envelope"
            );
            let target_id = format!("memories/{id}");
            let record = self
                .inspect(&target_id)?
                .context("Managed OKF record disappeared")?;
            ensure!(
                record.canonical_id == id,
                "Managed OKF path identity mismatch"
            );
            records.insert(target_id, record);
        }
        Ok(records)
    }
}

impl Writer for OkfWriter {
    fn id(&self) -> &'static str {
        "okf"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn location(&self) -> &Path {
        &self.location
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            supported_fields: target::fields()
                .into_iter()
                .filter(|path| path != "/embedding")
                .chain([
                    "/embedding/model".into(),
                    "/embedding/dim".into(),
                    "/embedding/normalized".into(),
                ])
                .collect(),
            unsupported_fields: vec!["/embedding/vector".into()],
            read_back: true,
            update: true,
        }
    }

    fn plan(&self, record: &CanonicalRecord, _previous: Option<&ReceiptEntry>) -> Planned {
        let mut projected = record.clone();
        let mut disposition = Disposition::Accepted;
        if let Some(embedding) = &mut projected.embedding
            && embedding.vector.take().is_some()
        {
            if projected.reembed_plan.is_none() {
                projected.reembed_plan = Some(ReembedPlan {
                canonical_ids: vec![record.canonical_id.clone()], model: embedding.model.clone(), dim: embedding.dim,
                quality_impact: "The OKF home retains model and dimension but not vectors. Re-embedding is a separate, explicit consumer action; no model is called by this migration.".into(),
            });
            }
            disposition = Disposition::Transformed {
                changes: vec![Change {
                    field_path: "/embedding/vector".into(),
                    kind: ChangeKind::FieldOmitted,
                }],
            };
        }
        if target::redact_requires_processing(record) {
            disposition = Disposition::Rejected {
                rule: "consent_redact_requires_processing".into(),
            };
        }
        for (name, is_index) in [("index.md", true), ("log.md", false)] {
            if let Ok(Some(bytes)) = target::read_file(&self.location, name) {
                let managed = if is_index {
                    std::str::from_utf8(&bytes).is_ok_and(|text| {
                        text.starts_with(
                            "---\ntype: Index\nokf_version: '0.2'\nmem_adaptor_index: true\n",
                        )
                    })
                } else {
                    bytes.starts_with(b"<!-- mem-adaptor managed migration log -->\n")
                };
                if !managed {
                    disposition = Disposition::Unresolved {
                        reason: UnresolvedReason::TargetUntracked,
                    };
                }
            }
        }
        Planned {
            record: projected,
            disposition,
            target_id: format!("memories/{}", record.canonical_id),
            previous_write: _previous.and_then(|entry| entry.prior_write.clone()),
            duplicate_write: None,
            target_map: vec![
                target::mapping("/content", "/body", "body_bytes_unchanged"),
                target::mapping(
                    "/content",
                    "/frontmatter/title",
                    "first_nonempty_line_80_unicode_characters",
                ),
                target::mapping(
                    "/source_locator",
                    "/frontmatter/sources/0/resource",
                    "source_locator",
                ),
                target::mapping(
                    "/updated_at",
                    "/frontmatter/generated/at",
                    "source_record_modified_time_not_fact_time",
                ),
                target::mapping(
                    "",
                    "/frontmatter/mem_adaptor",
                    "canonical_metadata_without_body_or_embedding_vector",
                ),
            ],
        }
    }

    fn write(&self, batch: &[Planned], token: &WriteToken) -> Result<WriteResult> {
        token.authorize(self, batch)?;
        if batch.is_empty() {
            return Ok(WriteResult::default());
        }
        let old_index = target::read_file(&self.location, "index.md")?;
        let old_log = target::read_file(&self.location, "log.md")?;
        token.authorize_artifact(self, "index.md", old_index.as_deref())?;
        token.authorize_artifact(self, "log.md", old_log.as_deref())?;
        let mut managed = self.managed_records()?;
        for target_id in managed.keys() {
            let path = format!("{target_id}.md");
            token.authorize_artifact(
                self,
                &path,
                target::read_file(&self.location, &path)?.as_deref(),
            )?;
        }
        fs::create_dir_all(self.location.join("memories"))?;
        ensure!(
            !fs::symlink_metadata(self.location.join("memories"))?
                .file_type()
                .is_symlink(),
            "Target memories directory must not be a symlink"
        );
        let mut written = Vec::new();
        let mut artifacts = Vec::new();
        for planned in batch {
            let mut metadata = serde_json::to_value(&planned.record)?;
            metadata.as_object_mut().unwrap().remove("content");
            let path = format!("{}.md", planned.target_id);
            let previous_bytes = target::read_file(&self.location, &path)?;
            token.authorize_artifact(self, &path, previous_bytes.as_deref())?;
            let mut frontmatter = if let Some(prior) = &planned.previous_write {
                ensure!(
                    prior.target_id == planned.target_id,
                    "Update identity does not match previous write"
                );
                let actual = self
                    .inspect(&prior.target_id)?
                    .context("Target deleted after approval")?;
                ensure!(
                    record_hash(&actual)? == prior.record_hash,
                    "Target changed after approval"
                );
                ensure!(
                    self.target_hash(&prior.target_id)?.as_ref() == Some(&prior.target_hash),
                    "Target payload changed after approval"
                );
                let text = std::str::from_utf8(previous_bytes.as_ref().unwrap())?;
                let yaml = text
                    .strip_prefix("---\n")
                    .context("Missing OKF frontmatter")?
                    .split_once("---\n")
                    .context("Unclosed OKF frontmatter")?
                    .0;
                serde_saphyr::from_str::<Value>(yaml)
                    .map_err(|_| anyhow::anyhow!("Invalid OKF YAML"))?
            } else {
                ensure!(
                    previous_bytes.is_none(),
                    "Target record already exists without a previous write"
                );
                json!({})
            };
            frontmatter["type"] = Value::String("Memory".into());
            frontmatter["mem_adaptor_envelope"] = Value::String("okf:0.2".into());
            frontmatter["title"] = Value::String(target::title(&planned.record));
            let mut origin = json!({"id": planned.record.source_record_id, "resource": planned.record.source_locator});
            if matches!(
                planned.record.provenance.actor_kind,
                ActorKind::User | ActorKind::Agent | ActorKind::Model
            ) {
                origin["author"] = Value::String(planned.record.provenance.actor.clone());
            }
            if let Some(modified) = &planned.record.updated_at {
                origin["last_modified"] = Value::String(modified.clone());
            }
            frontmatter["sources"] = json!([origin]);
            if let Some(tags) = &planned.record.tags {
                frontmatter["tags"] = json!(tags);
            } else {
                frontmatter.as_object_mut().unwrap().remove("tags");
            }
            if let Some(modified) = &planned.record.updated_at {
                let mut generated = json!({"at": modified});
                if matches!(
                    planned.record.provenance.actor_kind,
                    ActorKind::User | ActorKind::Agent | ActorKind::Model
                ) {
                    generated["by"] = Value::String(planned.record.provenance.actor.clone());
                }
                frontmatter["generated"] = generated;
            } else {
                frontmatter.as_object_mut().unwrap().remove("generated");
            }
            frontmatter["mem_adaptor"] = metadata;
            let yaml = serde_saphyr::to_string(&frontmatter)?;
            let text = format!("---\n{yaml}---\n{}", planned.record.content);
            target::atomic_file(
                &self.location,
                &path,
                text.as_bytes(),
                previous_bytes.as_deref(),
            )?;
            written.push(Written {
                canonical_id: planned.record.canonical_id.clone(),
                target_id: planned.target_id.clone(),
                target_hash: mem_adaptor_core::engine::content_hash(text.as_bytes()),
            });
            artifacts.push(target::output_artifact(&path, text.as_bytes()));
            managed.insert(planned.target_id.clone(), planned.record.clone());
        }
        let mut groups = std::collections::BTreeMap::<String, Vec<String>>::new();
        for (target_id, record) in managed {
            let scope = serde_json::to_value(record.scope)?
                .as_str()
                .unwrap()
                .to_owned();
            let label = record
                .scope_qualifier
                .as_ref()
                .map_or(scope.clone(), |qualifier| format!("{scope}: {qualifier}"));
            let title = target::title(&record)
                .replace('\\', "\\\\")
                .replace('[', "\\[")
                .replace(']', "\\]");
            groups
                .entry(label)
                .or_default()
                .push(format!("- [{title}]({target_id}.md)\n"));
        }
        let mut index =
            "---\ntype: Index\nokf_version: '0.2'\nmem_adaptor_index: true\n---\n# Memory index\n"
                .to_owned();
        for (scope, mut entries) in groups {
            entries.sort();
            index.push_str(&format!("\n## {}\n\n", scope.replace(['\r', '\n'], " ")));
            for entry in entries {
                index.push_str(&entry);
            }
        }
        target::atomic_file(
            &self.location,
            "index.md",
            index.as_bytes(),
            old_index.as_deref(),
        )?;
        artifacts.push(target::output_artifact("index.md", index.as_bytes()));
        let mut log = old_log.clone().unwrap_or_else(|| {
            b"<!-- mem-adaptor managed migration log -->\n# Migration log\n".to_vec()
        });
        let updates = batch
            .iter()
            .filter(|planned| planned.previous_write.is_some())
            .count();
        let systems: std::collections::BTreeSet<_> = batch
            .iter()
            .map(|planned| planned.record.source.system.as_str())
            .collect();
        log.extend_from_slice(
            format!(
                "\n- {}: source={}, added={}, updated={}, removed=0.\n",
                mem_adaptor_core::engine::timestamp()?,
                systems.into_iter().collect::<Vec<_>>().join(","),
                batch.len() - updates,
                updates
            )
            .as_bytes(),
        );
        target::atomic_file(&self.location, "log.md", &log, old_log.as_deref())?;
        artifacts.push(target::output_artifact("log.md", &log));
        Ok(WriteResult { written, artifacts })
    }

    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>> {
        written
            .iter()
            .map(|written| {
                let record = self
                    .inspect(&written.target_id)?
                    .context("Missing target on read-back")?;
                Ok(ReadBack {
                    canonical_id: written.canonical_id.clone(),
                    record,
                })
            })
            .collect()
    }

    fn inspect(&self, target_id: &str) -> Result<Option<CanonicalRecord>> {
        let id = target_id
            .strip_prefix("memories/")
            .context("Invalid OKF target id")?;
        ensure!(
            id.len() == 32
                && id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)),
            "Invalid OKF target id"
        );
        let path = self.location.join(format!("{target_id}.md"));
        if self.location.join("memories").exists() {
            ensure!(
                !fs::symlink_metadata(self.location.join("memories"))?
                    .file_type()
                    .is_symlink(),
                "Target memories directory must not be a symlink"
            );
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Target record must be a regular file"
        );
        let text = fs::read_to_string(&path)?;
        let rest = text
            .strip_prefix("---\n")
            .context("Missing OKF frontmatter")?;
        let (yaml, body) = rest
            .split_once("---\n")
            .context("Unclosed OKF frontmatter")?;
        let mut metadata: Value = serde_saphyr::from_str(yaml)
            .map_err(|_| anyhow::anyhow!("Invalid OKF YAML on read-back"))?;
        ensure!(metadata["type"] == "Memory", "Unexpected OKF type");
        let extension = metadata["mem_adaptor"]
            .as_object_mut()
            .context("Missing OKF metadata extension")?;
        extension.insert("content".into(), Value::String(body.into()));
        let record: CanonicalRecord = serde_json::from_value(metadata["mem_adaptor"].clone())
            .map_err(|_| anyhow::anyhow!("Invalid OKF record fields"))?;
        mem_adaptor_core::schema::validate("canonical-record", &record)?;
        Ok(Some(record))
    }

    fn target_hash(&self, target_id: &str) -> Result<Option<String>> {
        self.inspect(target_id)?;
        Ok(
            target::read_file(&self.location, &format!("{target_id}.md"))?
                .map(|bytes| mem_adaptor_core::engine::content_hash(&bytes)),
        )
    }

    fn artifacts(&self, target_ids: &[String]) -> Result<Vec<TargetArtifact>> {
        let mut paths: std::collections::BTreeSet<_> =
            target_ids.iter().map(|id| format!("{id}.md")).collect();
        paths.extend(
            self.managed_records()?
                .into_keys()
                .map(|id| format!("{id}.md")),
        );
        paths.extend(["index.md".into(), "log.md".into()]);
        paths
            .into_iter()
            .map(|path| target::artifact(&self.location, &path))
            .collect()
    }
    fn shared_artifact_paths(&self) -> &'static [&'static str] {
        &["index.md", "log.md"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_preserves_unknown_fields_order_and_typed_values() {
        let input =
            "type: Memory\nx_unknown:\n  z_last: [false, 3, text]\n  a_first: 2\ntitle: Example\n";
        let value: Value = serde_saphyr::from_str(input).unwrap();
        assert_eq!(
            value
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            ["type", "x_unknown", "title"]
        );
        let output = serde_saphyr::to_string(&value).unwrap();
        let restored: Value = serde_saphyr::from_str(&output).unwrap();
        assert_eq!(restored, value);
        assert!(output.find("z_last").unwrap() < output.find("a_first").unwrap());
        assert!(output.find("x_unknown").unwrap() < output.find("title").unwrap());
    }
}
