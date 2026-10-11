//! Writer for basic-memory project directories: files are truth, the SQLite index is the tenant's.
//! Emits native frontmatter only (title/type/permalink, optional tags) with an explicit permalink so
//! basic-memory's own sync finds nothing to reconcile and never rewrites our bytes (verified
//! experimentally, docs/memory-products.md). Provenance and identity bookkeeping live in mem-adaptor
//! receipts, never in the target files: the frontmatter has a single owner (the target format).

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::Context;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::{BasisMismatch, WriteToken, content_hash, record_hash};
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use mem_adaptor_core::reports::*;
use mem_adaptor_core::writer as target;
use serde_json::{Value, json};

pub struct BasicMemoryWriter {
    location: PathBuf,
}

/// Lowers a display title to basic-memory permalink shape: lowercase ASCII runs joined by hyphens.
/// Non-ASCII scripts yield an empty slug and fall back to an id-based name.
fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for character in title.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

impl BasicMemoryWriter {
    /// Normalizes accepted ancestor aliases and rejects an explicitly symlinked target root.
    pub fn new(location: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            location: target::normalize_root(&location)?,
        })
    }

    /// Renders the exact bytes a write of `planned` would produce. The frontmatter is fully derived
    /// (no sticky fields): title from the body heading, fixed type, the planned permalink identity,
    /// and tags when the record carries them; the body is the record content unchanged.
    fn render(&self, planned: &Planned) -> mem_adaptor_core::Result<Vec<u8>> {
        let mut frontmatter = json!({
            "title": target::title(&planned.record),
            "type": "note",
            "permalink": planned.target_id,
        });
        if let Some(tags) = &planned.record.tags {
            frontmatter["tags"] = json!(tags);
        }
        let yaml = serde_saphyr::to_string(&frontmatter)?;
        Ok(format!("---\n{yaml}---\n{}", planned.record.content).into_bytes())
    }

    /// Derives a fresh permalink identity purely from the record: the title slug when it yields
    /// ASCII content, otherwise an id-based name. Derivation never probes the filesystem, so the
    /// engine's plan-time artifact enumeration and later runs agree. A parseable foreign file at
    /// the slug path surfaces as an unresolved target-untracked entry instead of being clobbered
    /// (an unparseable occupant fails the plan outright); two same-title records refuse their
    /// batch rather than overwrite each other.
    fn fresh_permalink(record: &CanonicalRecord) -> String {
        let slug = slugify(&target::title(record));
        if slug.is_empty() {
            format!("note-{}", &record.canonical_id[..12])
        } else {
            slug
        }
    }
}

impl Writer for BasicMemoryWriter {
    fn id(&self) -> &'static str {
        "basic-memory"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn location(&self) -> &std::path::Path {
        &self.location
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // The native shape round-trips body and tags; every other canonical field (scope,
            // provenance, evidence, time, source identity) is receipt-side bookkeeping, not a
            // target fact — the permalink is a derived target identity, not the source record id.
            supported_fields: vec!["/content".into(), "/tags".into()],
            unsupported_fields: vec!["/embedding/vector".into()],
            read_back: true,
            update: true,
        }
    }

    /// Projects vector loss explicitly and derives the stable permalink identity. Native mappings
    /// describe only fields present in this record's projected target.
    fn plan(
        &self,
        record: &CanonicalRecord,
        previous: Option<&ReceiptEntry>,
    ) -> mem_adaptor_core::Result<Planned> {
        let mut projected = record.clone();
        let mut disposition = Disposition::Accepted;
        if let Some(embedding) = &mut projected.embedding
            && embedding.vector.take().is_some()
        {
            if projected.reembed_plan.is_none() {
                projected.reembed_plan = Some(ReembedPlan {
                    canonical_ids: vec![record.canonical_id.clone()],
                    model: embedding.model.clone(),
                    dim: embedding.dim,
                    quality_impact: "The basic-memory target keeps files only; its own indexer re-embeds on its side. No model is called by this migration.".into(),
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
        // Updates keep the prior permalink: renaming the note in basic-memory does not move the
        // file, and our receipts bind the identity, so the target id stays stable.
        let target_id = match previous.and_then(|entry| entry.prior_write.as_ref()) {
            Some(prior) => prior.target_id.clone(),
            None => Self::fresh_permalink(record),
        };
        let mut target_map = vec![
            target::mapping("/content", "/body", "body_bytes_unchanged"),
            target::mapping(
                "/content",
                "/frontmatter/title",
                "first_nonempty_line_markdown_heading_stripped_80_unicode_characters",
            ),
            target::mapping("", "/frontmatter/permalink", "title_slug_with_id_fallback"),
            target::mapping("", "/frontmatter/type", "fixed_native_type_note"),
        ];
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
            target_id,
            previous_write: previous.and_then(|entry| entry.prior_write.clone()),
            duplicate_write: None,
            target_map,
        })
    }

    /// Rechecks approved bytes before any mutation, then persists one native file per record.
    /// Failure after the first persistence may leave partial output; no batch rollback is promised.
    fn write(
        &self,
        batch: &[Planned],
        token: &WriteToken,
    ) -> mem_adaptor_core::Result<WriteResult> {
        token.authorize(self, batch)?;
        if batch.is_empty() {
            return Ok(WriteResult::default());
        }
        // Two records cannot share one permalink file: refuse the batch instead of clobbering.
        let mut ids = BTreeSet::new();
        for planned in batch {
            anyhow::ensure!(
                ids.insert(planned.target_id.clone()),
                "Duplicate permalink target {} in one batch; the colliding notes need distinct titles",
                planned.target_id
            );
        }
        let mut written = Vec::new();
        let mut artifacts = Vec::new();
        for planned in batch {
            let path = format!("{}.md", planned.target_id);
            let previous_bytes = target::read_file(&self.location, &path)?;
            token.authorize_artifact(self, &path, previous_bytes.as_deref())?;
            if let Some(prior) = &planned.previous_write {
                anyhow::ensure!(
                    prior.target_id == planned.target_id,
                    "Update identity does not match previous write"
                );
                let actual = self.inspect(&prior.target_id)?;
                // A target that moved after approval fails the execution-basis check when it was
                // deleted or edited (an unparseable edit instead surfaces as an ordinary error
                // from `inspect`, which is equally refusing but a different exit class).
                let Some(actual) = actual else {
                    return Err(anyhow::Error::new(BasisMismatch::new(
                        "Target deleted after approval",
                    )));
                };
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
                anyhow::ensure!(
                    previous_bytes.is_none(),
                    "Target record already exists without a previous write"
                );
            }
            let bytes = self.render(planned)?;
            target::atomic_file(&self.location, &path, &bytes, previous_bytes.as_deref())?;
            written.push(Written {
                canonical_id: planned.record.canonical_id.clone(),
                target_id: planned.target_id.clone(),
                target_hash: content_hash(&bytes),
            });
            artifacts.push(target::output_artifact(&path, &bytes));
        }
        Ok(WriteResult { written, artifacts })
    }

    fn read_back(&self, written: &[Written]) -> mem_adaptor_core::Result<Vec<ReadBack>> {
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

    /// Rebuilds the observable record from one native file: body, permalink identity, and tags.
    /// The reconstruction is deterministic from the file alone (receipts, not the file, carry the
    /// migration identity), so its record hash stays stable across write-time and later inspections.
    fn inspect(&self, target_id: &str) -> mem_adaptor_core::Result<Option<CanonicalRecord>> {
        anyhow::ensure!(
            !target_id.is_empty() && !target_id.contains('/'),
            "Invalid basic-memory target id"
        );
        let Some(bytes) = target::read_file(&self.location, &format!("{target_id}.md"))? else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes).context("Invalid target Markdown encoding")?;
        let (frontmatter, body) = normalize::markdown_document(text)?;
        let value = frontmatter
            .and_then(|yaml| normalize::frontmatter(yaml).transpose())
            .transpose()?
            .with_context(|| {
                format!(
                    "basic-memory target file has no frontmatter: {}",
                    mem_adaptor_core::gate::mask(&format!("{target_id}.md"))
                )
            })?;
        let permalink = value
            .get("permalink")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .with_context(|| {
                format!(
                    "basic-memory target file has no permalink: {}",
                    mem_adaptor_core::gate::mask(&format!("{target_id}.md"))
                )
            })?;
        let mut record = normalize::record(
            self.id(),
            self.version(),
            None,
            &permalink,
            &format!("{target_id}.md"),
            body,
            EvidenceLevel::Measured,
        );
        if let Some(tags) = value.get("tags")
            && let Some(array) = tags
                .as_array()
                .filter(|array| array.iter().all(Value::is_string))
        {
            record.tags = Some(
                array
                    .iter()
                    .map(|value| value.as_str().unwrap().into())
                    .collect(),
            );
        }
        Ok(Some(record))
    }

    fn target_hash(&self, target_id: &str) -> mem_adaptor_core::Result<Option<String>> {
        self.inspect(target_id)?;
        Ok(
            target::read_file(&self.location, &format!("{target_id}.md"))?
                .map(|bytes| content_hash(&bytes)),
        )
    }

    /// Renders the exact bytes a write of `planned` would currently produce (DEC-21 B rule five);
    /// `None` only when an anchored file disappeared, which planning reports through inspection.
    fn project(&self, planned: &Planned) -> mem_adaptor_core::Result<Option<Vec<u8>>> {
        let path = format!("{}.md", planned.target_id);
        let previous_bytes = target::read_file(&self.location, &path)?;
        if planned.previous_write.is_some() && previous_bytes.is_none() {
            return Ok(None);
        }
        Ok(Some(self.render(planned)?))
    }

    fn artifacts(&self, target_ids: &[String]) -> mem_adaptor_core::Result<Vec<TargetArtifact>> {
        target_ids
            .iter()
            .map(|id| target::artifact(&self.location, &format!("{id}.md")))
            .collect()
    }
    /// basic-memory owns its index and database; this Writer has no shared artifacts to guard.
    fn shared_artifact_paths(&self) -> &'static [&'static str] {
        &[]
    }
}
