//! Writer for Open Knowledge Format Markdown homes, called after engine planning and explicit approval.
//! Shares native projection and envelope parsing with the Reader; unowned files are never adopted.
//! Writes individual approved files atomically, without cross-file transactions or cross-process locking.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::{CanonicalRecord, ReembedPlan};
use mem_adaptor_core::engine::{BasisMismatch, WriteToken, record_hash};
use mem_adaptor_core::okf;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reports::*;
use mem_adaptor_core::writer as target;
use serde_json::{Value, json};

pub struct OkfWriter {
    location: PathBuf,
}

/// Classified state of one OKF managed-target file (DEC-21 A/D): the planning path distinguishes a
/// de-managed or damaged-managed file from a healthy managed record so only that file is reported.
enum Observation {
    Missing,
    Unmanaged,
    ManagedInvalid,
    Managed {
        record: Box<CanonicalRecord>,
        /// Home-file-side pointers edited relative to the envelope evidence (DEC-21 A/B).
        home_changed_fields: Vec<String>,
        /// Record hash of the file's own tool-owned block before home-side adoption (DEC-21 A).
        envelope_hash: String,
    },
}

impl OkfWriter {
    /// Normalizes accepted ancestor aliases once and rejects an explicitly symlinked target root.
    pub fn new(location: PathBuf) -> Result<Self> {
        Ok(Self {
            location: target::normalize_root(&location)?,
        })
    }

    /// One classified look at a managed-target candidate (DEC-21 A/D). Parse-level failures (damaged
    /// claimed envelopes, invalid UTF-8, invalid target ids) still error and abort planning; a parseable
    /// but schema/consistency-invalid envelope or a de-managed file classifies so the engine can report
    /// that single file instead of failing the plan. Managed records adopt home-side edits, so the
    /// returned record reflects the home's current facts (body hash, tags).
    fn observe(&self, target_id: &str) -> Result<Observation> {
        let id = target_id
            .strip_prefix("memories/")
            .context("Invalid OKF target id")?;
        ensure!(okf::valid_id(id), "Invalid OKF target id");
        let Some(bytes) = target::read_file(&self.location, &format!("{target_id}.md"))? else {
            return Ok(Observation::Missing);
        };
        let text = std::str::from_utf8(&bytes).context("Invalid target Markdown encoding")?;
        let Some(metadata) = okf::managed_metadata(text)? else {
            return Ok(Observation::Unmanaged);
        };
        let (_, body) = mem_adaptor_core::reader::markdown_document(text)?;
        // A parseable but schema/consistency-invalid envelope (including a path-identity mismatch)
        // classifies per file instead of failing the plan; strict single-file `inspect` turns the
        // same classification into an error for write-time rechecks.
        let Ok(mut record) = okf::restore(&metadata, body) else {
            return Ok(Observation::ManagedInvalid);
        };
        if record.canonical_id != id {
            return Ok(Observation::ManagedInvalid);
        }
        let envelope_hash = mem_adaptor_core::engine::record_hash(&record)?;
        let edits = okf::apply_home_edits(&mut record, &metadata, body);
        Ok(Observation::Managed {
            record: Box::new(record),
            home_changed_fields: okf::home_changed_pointers(&edits),
            envelope_hash,
        })
    }

    /// Renders the exact bytes a write of `planned` would produce, without writing or checking
    /// authorization (DEC-21 B/D): sticky advisory fields come from the current file's frontmatter
    /// and envelope whenever a previous write anchors an update, mirroring `write` byte for byte.
    fn render(&self, planned: &Planned, previous_bytes: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut sticky_base: Option<Value> = None;
        let mut frontmatter = if planned.previous_write.is_some() {
            let text =
                std::str::from_utf8(previous_bytes.context("Missing previous target bytes")?)?;
            let old_metadata =
                okf::managed_metadata(text)?.context("Missing managed OKF metadata")?;
            // Sticky base (DEC-21 A): the old envelope's derivation decides whether an advisory
            // value was user-edited, so restore the old record before overwriting the frontmatter.
            let (_, old_body) = mem_adaptor_core::reader::markdown_document(text)?;
            let old_record = okf::restore(&old_metadata, old_body)?;
            sticky_base = Some(okf::native_projection(&old_record));
            old_metadata
        } else {
            json!({})
        };
        let mut metadata = serde_json::to_value(&planned.record)?;
        metadata.as_object_mut().unwrap().remove("content");
        let native = okf::native_projection(&planned.record);
        frontmatter["type"] = native["type"].clone();
        frontmatter["mem_adaptor_envelope"] = Value::String("okf:0.2".into());
        // Advisory fields are sticky (DEC-21 A): a value the user changed relative to the old
        // envelope's derivation survives the update instead of being overwritten by the new
        // projection. Tags follow the canonical record, whose home value is already the fact.
        for field in ["title", "sources", "generated"] {
            let keep_user_value = sticky_base.as_ref().is_some_and(|old| {
                frontmatter.get(field).is_some() && frontmatter.get(field) != old.get(field)
            });
            if keep_user_value {
                continue;
            }
            if let Some(value) = native.get(field) {
                frontmatter[field] = value.clone();
            } else {
                frontmatter.as_object_mut().unwrap().remove(field);
            }
        }
        if let Some(tags) = native.get("tags") {
            frontmatter["tags"] = tags.clone();
        } else {
            frontmatter.as_object_mut().unwrap().remove("tags");
        }
        frontmatter["mem_adaptor"] = metadata;
        let yaml = serde_saphyr::to_string(&frontmatter)?;
        Ok(format!("---\n{yaml}---\n{}", planned.record.content).into_bytes())
    }

    /// Enumerates managed-candidate paths by envelope claim, not by parse success, so target state
    /// observation includes damaged envelopes (DEC-21 A/D) without aborting, while unmanaged user notes
    /// with id-shaped names stay excluded (never adopted). YAML-level damage on a claimed envelope still
    /// propagates because `managed_metadata` refuses it.
    fn managed_paths(&self) -> Result<std::collections::BTreeSet<String>> {
        let mut paths = std::collections::BTreeSet::new();
        if !target::directory_exists(&self.location.join("memories"))? {
            return Ok(paths);
        }
        for entry in fs::read_dir(self.location.join("memories"))? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some(id) = name.strip_suffix(".md") else {
                continue;
            };
            if !okf::valid_id(id) {
                continue;
            }
            let bytes: Option<Vec<u8>> =
                target::read_file(&self.location, &format!("memories/{id}.md"))?;
            let bytes = bytes.context("Managed candidate vanished during enumeration")?;
            let text = std::str::from_utf8(&bytes).context("Invalid target Markdown encoding")?;
            if okf::managed_metadata(text)?.is_some() {
                paths.insert(format!("memories/{id}.md"));
            }
        }
        Ok(paths)
    }

    /// Preflights every valid-id candidate using the same managed-envelope classifier as direct inspection.
    /// Ordinary Markdown remains unowned; unreadable candidates and corrupt managed records stop planning.
    fn managed_records(&self) -> Result<std::collections::BTreeMap<String, CanonicalRecord>> {
        let mut records = std::collections::BTreeMap::new();
        if !target::directory_exists(&self.location.join("memories"))? {
            return Ok(records);
        }
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
            if !okf::valid_id(id) {
                continue;
            }
            let target_id = format!("memories/{id}");
            if let Some(record) = self.inspect(&target_id)? {
                records.insert(target_id, record);
            }
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

    /// Projects vector loss explicitly and refuses unowned shared artifacts without swallowing filesystem errors.
    /// Native mappings describe only fields present in this record's projected target.
    fn plan(&self, record: &CanonicalRecord, _previous: Option<&ReceiptEntry>) -> Result<Planned> {
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
            if let Some(bytes) = target::read_file(&self.location, name)? {
                let managed = match std::str::from_utf8(&bytes) {
                    Ok(text) if is_index => okf::owned_index(text)?,
                    Ok(text) => okf::owned_log(text)?.is_some(),
                    Err(_) => false,
                };
                if !managed {
                    disposition = Disposition::Unresolved {
                        reason: UnresolvedReason::TargetUntracked,
                    };
                }
            }
        }
        let native = okf::native_projection(&projected);
        let mut target_map = vec![
            target::mapping("/content", "/body", "body_bytes_unchanged"),
            target::mapping(
                "/content",
                "/frontmatter/title",
                "first_nonempty_line_markdown_heading_stripped_80_unicode_characters",
            ),
            target::mapping(
                "/source_record_id",
                "/frontmatter/sources/0/id",
                "source_record_id",
            ),
            target::mapping(
                "/source_locator",
                "/frontmatter/sources/0/resource",
                "source_locator",
            ),
            target::mapping(
                "",
                "/frontmatter/mem_adaptor",
                "canonical_metadata_without_body_or_embedding_vector",
            ),
        ];
        if native["sources"][0].get("author").is_some() {
            target_map.push(target::mapping(
                "/provenance/actor",
                "/frontmatter/sources/0/author",
                "source_author_human_prefix_for_user",
            ));
        }
        if projected.updated_at.is_some() {
            target_map.push(target::mapping(
                "/updated_at",
                "/frontmatter/sources/0/last_modified",
                "source_record_modified_time_not_fact_time",
            ));
        }
        if native.get("generated").is_some() {
            target_map.extend([
                target::mapping(
                    "/provenance/actor",
                    "/frontmatter/generated/by",
                    "source_author_human_prefix_for_user",
                ),
                target::mapping(
                    "/updated_at",
                    "/frontmatter/generated/at",
                    "source_record_modified_time_not_fact_time",
                ),
            ]);
        }
        if projected.tags.is_some() {
            target_map.push(target::mapping(
                "/tags",
                "/frontmatter/tags",
                "tags_unchanged",
            ));
        }
        Ok(Planned {
            record: projected,
            disposition,
            target_id: format!("memories/{}", record.canonical_id),
            previous_write: _previous.and_then(|entry| entry.prior_write.clone()),
            duplicate_write: None,
            target_map,
        })
    }

    /// Rechecks approved bytes and ownership before any mutation, then persists records and native shared files.
    /// Failure after the first persistence may leave partial output; no batch rollback or durability is promised.
    fn write(&self, batch: &[Planned], token: &WriteToken) -> Result<WriteResult> {
        token.authorize(self, batch)?;
        if batch.is_empty() {
            return Ok(WriteResult::default());
        }
        let old_index = target::read_file(&self.location, "index.md")?;
        let old_log = target::read_file(&self.location, "log.md")?;
        token.authorize_artifact(self, "index.md", old_index.as_deref())?;
        token.authorize_artifact(self, "log.md", old_log.as_deref())?;
        if let Some(bytes) = &old_index {
            ensure!(
                okf::owned_index(
                    std::str::from_utf8(bytes).context("Invalid OKF index encoding")?
                )?,
                "Target index is not owned by this Writer"
            );
        }
        let mut log_groups = match &old_log {
            Some(bytes) => {
                okf::owned_log(std::str::from_utf8(bytes).context("Invalid OKF log encoding")?)?
                    .context("Target log is not owned by this Writer")?
            }
            None => std::collections::BTreeMap::new(),
        };
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
            let path = format!("{}.md", planned.target_id);
            let previous_bytes = target::read_file(&self.location, &path)?;
            token.authorize_artifact(self, &path, previous_bytes.as_deref())?;
            if let Some(prior) = &planned.previous_write {
                ensure!(
                    prior.target_id == planned.target_id,
                    "Update identity does not match previous write"
                );
                let actual = self.inspect(&prior.target_id)?;
                // A target that moved after approval is an execution-basis mismatch, whether it was
                // deleted or edited, so the exit code stays the same across those two outcomes.
                let Some(actual) = actual else {
                    return Err(anyhow::Error::new(BasisMismatch::new(
                        "Target deleted after approval",
                    )));
                };
                // A target that moved after approval is an execution-basis mismatch, not an ordinary failure.
                if record_hash(&actual)? != prior.record_hash {
                    return Err(anyhow::Error::new(BasisMismatch::new(
                        "Target changed after approval",
                    )));
                }
                if self.target_hash(&prior.target_id)?.as_ref() != Some(&prior.target_hash) {
                    return Err(anyhow::Error::new(BasisMismatch::new(
                        "Target payload changed after approval",
                    )));
                }
            } else {
                ensure!(
                    previous_bytes.is_none(),
                    "Target record already exists without a previous write"
                );
            }
            let text = self.render(planned, previous_bytes.as_deref())?;
            target::atomic_file(&self.location, &path, &text, previous_bytes.as_deref())?;
            written.push(Written {
                canonical_id: planned.record.canonical_id.clone(),
                target_id: planned.target_id.clone(),
                target_hash: mem_adaptor_core::engine::content_hash(&text),
            });
            artifacts.push(target::output_artifact(&path, &text));
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
        let mut index = format!(
            "---\nokf_version: '0.2'\n---\n{}\n# Memory index\n",
            okf::INDEX_MARKER
        );
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
        let updates = batch
            .iter()
            .filter(|planned| planned.previous_write.is_some())
            .count();
        let timestamp = mem_adaptor_core::engine::timestamp()?;
        let (date, time) = timestamp
            .split_once('T')
            .context("Invalid migration timestamp")?;
        // The clock describes this migration event, never the creation or modification of a source fact.
        let clock = format!("{}Z", time.trim_end_matches('Z').split('.').next().unwrap());
        let group = log_groups.entry(date.into()).or_default();
        let mut event = format!(
            "- [{clock}] mem-adaptor: +{} ~{updates}\n",
            batch.len() - updates
        );
        // Keep the previous body contiguous and separate its first paragraph from the new Markdown list item.
        if group.first().is_some_and(|line| !line.trim().is_empty()) {
            event.push('\n');
        }
        group.insert(0, event);
        let mut log = format!("{}\n# Directory Update Log\n", okf::LOG_MARKER);
        for (date, entries) in log_groups.into_iter().rev() {
            if !log.ends_with('\n') {
                log.push('\n');
            }
            if !log.ends_with("\n\n") {
                log.push('\n');
            }
            log.push_str(&format!("## {date}\n"));
            if entries.first().is_some_and(|line| !line.trim().is_empty()) {
                log.push('\n');
            }
            for entry in entries {
                log.push_str(&entry);
            }
        }
        target::atomic_file(&self.location, "log.md", log.as_bytes(), old_log.as_deref())?;
        artifacts.push(target::output_artifact("log.md", log.as_bytes()));
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

    /// Reads a safe regular-file candidate, distinguishing unowned Markdown from corrupt managed envelopes.
    /// Managed records adopt home-side edits (DEC-21 A) so the returned record reflects the home's current
    /// facts; a parseable but invalid envelope stays an error here because write-time rechecks and single
    /// inspections must keep refusing instead of classifying (the planning path uses `inspect_many`).
    fn inspect(&self, target_id: &str) -> Result<Option<CanonicalRecord>> {
        match self.observe(target_id)? {
            Observation::Missing | Observation::Unmanaged => Ok(None),
            Observation::Managed { record, .. } => Ok(Some(*record)),
            Observation::ManagedInvalid => anyhow::bail!(
                "Managed OKF envelope is invalid: {}",
                mem_adaptor_core::gate::mask(&format!("{target_id}.md"))
            ),
        }
    }

    /// Classifies each managed-target file once and lets planning continue past user damage (DEC-21 A/D):
    /// YAML-level parse failures still abort the whole plan through `observe`'s error, while a parseable
    /// but invalid envelope or a de-managed file becomes a classified entry the engine reports per file.
    fn inspect_many(
        &self,
        target_ids: &[String],
    ) -> Result<std::collections::BTreeMap<String, TargetState>> {
        target_ids
            .iter()
            .map(|id| {
                Ok((
                    id.clone(),
                    match self.observe(id)? {
                        Observation::Missing => TargetState {
                            record: None,
                            target_hash: None,
                            classification: None,
                            home_changed_fields: Vec::new(),
                            envelope_hash: None,
                        },
                        Observation::Unmanaged => TargetState {
                            record: None,
                            target_hash: Some(mem_adaptor_core::engine::content_hash(
                                &target::read_file(&self.location, &format!("{id}.md"))?
                                    .context("Target vanished during inspection")?,
                            )),
                            classification: Some(TargetClassification::Unmanaged),
                            home_changed_fields: Vec::new(),
                            envelope_hash: None,
                        },
                        Observation::ManagedInvalid => TargetState {
                            record: None,
                            target_hash: Some(mem_adaptor_core::engine::content_hash(
                                &target::read_file(&self.location, &format!("{id}.md"))?
                                    .context("Target vanished during inspection")?,
                            )),
                            classification: Some(TargetClassification::ManagedInvalid),
                            home_changed_fields: Vec::new(),
                            envelope_hash: None,
                        },
                        Observation::Managed {
                            record,
                            home_changed_fields,
                            envelope_hash,
                        } => TargetState {
                            target_hash: Some(mem_adaptor_core::engine::content_hash(
                                &target::read_file(&self.location, &format!("{id}.md"))?
                                    .context("Target vanished during inspection")?,
                            )),
                            record: Some(*record),
                            classification: None,
                            home_changed_fields,
                            envelope_hash: Some(envelope_hash),
                        },
                    },
                ))
            })
            .collect()
    }

    fn target_hash(&self, target_id: &str) -> Result<Option<String>> {
        self.inspect(target_id)?;
        Ok(
            target::read_file(&self.location, &format!("{target_id}.md"))?
                .map(|bytes| mem_adaptor_core::engine::content_hash(&bytes)),
        )
    }

    /// Renders the exact bytes a write of `planned` would currently produce (DEC-21 B rule five),
    /// so the engine can prove convergence instead of guessing; `None` never happens for OKF except
    /// when an anchored file disappeared, which planning reports through its own inspection path.
    fn project(&self, planned: &Planned) -> Result<Option<Vec<u8>>> {
        let path = format!("{}.md", planned.target_id);
        let previous_bytes = target::read_file(&self.location, &path)?;
        if planned.previous_write.is_some() && previous_bytes.is_none() {
            return Ok(None);
        }
        Ok(Some(self.render(planned, previous_bytes.as_deref())?))
    }

    fn artifacts(&self, target_ids: &[String]) -> Result<Vec<TargetArtifact>> {
        let mut paths: std::collections::BTreeSet<_> =
            target_ids.iter().map(|id| format!("{id}.md")).collect();
        // Claim-level enumeration keeps damaged (still-declared) managed files observable as target
        // state instead of failing the plan (DEC-21 A/D), while unmanaged user notes are never
        // adopted; write-time handling stays strict in `managed_records`.
        paths.extend(self.managed_paths()?);
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
