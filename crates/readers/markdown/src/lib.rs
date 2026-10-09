//! Reader for local Markdown, Obsidian, Claude Code memory, and OKF homes.
//! Converts claimed source files into paired canonical/original records before the engine validates and plans.
//! Preserves body text and unknown metadata; indices are registration-only, with no extraction model or target writes.

use anyhow::Context;
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::okf;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use regex::Regex;
use serde_json::{Value, json};

pub struct MarkdownReader;

impl Reader for MarkdownReader {
    /// Registers this adapter under Markdown while records may retain a more specific source-system identity.
    fn id(&self) -> &'static str {
        "markdown"
    }
    /// Reports the compiled adapter version for reproducible source interpretation.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    /// Markdown vaults and OKF homes are directories the user keeps in place, so their path can bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
    }

    /// Claims Markdown except runtime artifacts and ChatGPT Prompt files reserved for another adapter.
    /// Marks MEMORY.md and version-only native OKF root index/log files registration-only without requiring ownership.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        let home = inventory.get("index.md").is_some_and(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .is_some_and(|text| okf::native_index(text).unwrap_or(false))
        });
        inventory
            .keys()
            .filter(|path| {
                path.ends_with(".md")
                    && !path.starts_with(".mem-adaptor/")
                    && !path.ends_with(".chatgpt.md")
            })
            .map(|path| {
                let index = path.rsplit('/').next() == Some("MEMORY.md")
                    || home && matches!(path.as_str(), "index.md" | "log.md");
                Claim {
                    path: path.clone(),
                    layer: if index { "index" } else { "auto_memory" }.into(),
                    registered_only: index,
                }
            })
            .collect()
    }

    /// Parses a claimed UTF-8 document and preserves its body, known source metadata, and unknown-field coverage.
    /// Restores OKF identity or derives a filesystem record; malformed fields and conflicting envelope metadata fail.
    /// Registration-only claims count links but emit no memory; this method does not write or approve migration.
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        self.read_claim(claim, source).with_context(|| {
            format!(
                "Markdown source at {}",
                mem_adaptor_core::gate::mask(&claim.path)
            )
        })
    }
}

impl MarkdownReader {
    /// Parses shared standalone delimiters and restores only validated managed envelopes with original source identity.
    /// Native fields use the Writer's projection rules; ordinary source metadata retains existing normalization.
    fn read_claim(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        let text = std::str::from_utf8(source.file(&claim.path)).context("Invalid UTF-8 source")?;
        if claim.registered_only {
            let link = Regex::new(r"^\s*-\s+\[[^\]]+\]\([^)]+\)").unwrap();
            output.registered_count =
                text.lines().filter(|line| link.is_match(line)).count() as u64;
            return Ok(output);
        }
        let managed = okf::managed_metadata(text)?;
        let (frontmatter, body) = normalize::markdown_document(text)?;
        let mut fields = json!({"body": body});
        if let Some(yaml) = frontmatter {
            if let Some(value) = normalize::frontmatter(yaml)? {
                fields["frontmatter"] = value;
            }
        } else if text.starts_with("---\n") || text.starts_with("---\r\n") {
            normalize::anomaly(&mut output, &claim.path, "frontmatter_unclosed", "", None);
        }
        let mut record;
        if let Some(metadata) = managed {
            // A home copy keeps the original source identity instead of creating a new identity from its target path.
            // Home-side edits are facts (DEC-21 A): the body is re-homed and divergent tags adopted,
            // while advisory-field divergence is reported as anomalies instead of failing the plan.
            record = okf::restore(&metadata, body)?;
            let edits = okf::apply_home_edits(&mut record, &metadata, body);
            if edits.body_changed {
                normalize::anomaly(&mut output, &claim.path, "okf_body_changed", "/body", None);
            }
            let divergence = okf::classify_native(&metadata, &record);
            if divergence.title {
                normalize::anomaly(
                    &mut output,
                    &claim.path,
                    "okf_title_divergent",
                    "/frontmatter/title",
                    None,
                );
            }
            if divergence.sources {
                normalize::anomaly(
                    &mut output,
                    &claim.path,
                    "okf_provenance_divergent",
                    "/frontmatter/sources",
                    None,
                );
            }
            if divergence.generated {
                normalize::anomaly(
                    &mut output,
                    &claim.path,
                    "okf_provenance_divergent",
                    "/frontmatter/generated",
                    None,
                );
            }
            let mut original = normalize::source(&record, &claim.path, fields);
            normalize::map(&mut original, "/body", "/content");
            normalize::map(&mut original, "/frontmatter/mem_adaptor", "");
            normalize::map(&mut original, "/frontmatter/type", "/source");
            normalize::map(
                &mut original,
                "/frontmatter/mem_adaptor_envelope",
                "/source",
            );
            let native = okf::native_projection(&record);
            // Display titles are advisory (DEC-21 A): consumed unconditionally so a user-edited title
            // never surfaces as unknown source metadata or collides in source_extra.
            normalize::map(&mut original, "/frontmatter/title", "");
            // Coverage keeps arrays atomic; this validated projection spans identity, provenance, locator and time.
            normalize::map(&mut original, "/frontmatter/sources", "");
            for (source_path, canonical_path) in [
                ("/generated/by", "/provenance/actor"),
                ("/generated/at", "/updated_at"),
                ("/tags", "/tags"),
            ] {
                if native.pointer(source_path).is_some() {
                    normalize::map(
                        &mut original,
                        &format!("/frontmatter{source_path}"),
                        canonical_path,
                    );
                }
            }
            // A tags key the record no longer carries (edited to null) still needs coverage so the
            // deletion statement is not reported as an unknown field.
            if metadata.get("tags").is_some() && native.pointer("/tags").is_none() {
                normalize::map(&mut original, "/frontmatter/tags", "");
            }
            normalize::finish(&mut output, record, original)?;
            return Ok(output);
        }
        let claude_code = fields
            .pointer("/frontmatter/metadata/node_type")
            .and_then(Value::as_str)
            == Some("memory");
        let system = if claude_code {
            "claude_code"
        } else {
            self.id()
        };
        record = normalize::record(
            system,
            self.version(),
            source.satellite_id.as_deref(),
            &claim.path,
            &claim.path,
            body,
            EvidenceLevel::Measured,
        );
        // A filesystem scan is provenance for this import, not evidence that the scanner authored the memory.
        record.provenance.actor = "process:filesystem".into();
        record.provenance.actor_kind = ActorKind::Scan;
        record.provenance.method = "filesystem".into();
        let mut original = normalize::source(&record, &claim.path, fields);
        normalize::map(&mut original, "/body", "/content");
        let kind_path = if claude_code {
            "/frontmatter/metadata/type"
        } else {
            "/frontmatter/type"
        };
        normalize::classify(&mut record, &mut original, kind_path)?;
        if claude_code {
            record.scope = Scope::Project;
            let parent = claim.path.rsplit_once('/').map_or("", |(parent, _)| parent);
            let components = parent.split('/').collect::<Vec<_>>();
            let slug = components.windows(3).find_map(|parts| {
                (parts[0] == "projects" && parts[2] == "memory").then_some(parts[1])
            });
            let relative = slug.unwrap_or(parent);
            // Home mode anchors the project qualifier to the satellite (DEC-20) so identical relative
            // projects in different satellites stay distinguishable; direct mode keeps the plain
            // relative qualifier. A root-level file has no relative part and keeps the satellite alone.
            record.scope_qualifier = Some(match &source.satellite_id {
                Some(satellite) if relative.is_empty() => satellite.clone(),
                Some(satellite) => format!("{satellite}/{relative}"),
                None => relative.into(),
            });
            if slug.is_none() {
                normalize::anomaly(
                    &mut output,
                    &claim.path,
                    "project_scope_relative_fallback",
                    "/scope_qualifier",
                    None,
                );
            }
            normalize::map(&mut original, "/frontmatter/metadata/node_type", "/source");
            if let Some(session) = original
                .fields
                .pointer("/frontmatter/metadata/originSessionId")
                .and_then(Value::as_str)
            {
                record.provenance.evidence = Some(vec![Evidence {
                    source_ref: format!("session:{session}"),
                    weight: None,
                }]);
                normalize::map(
                    &mut original,
                    "/frontmatter/metadata/originSessionId",
                    "/provenance/evidence",
                );
            }
        }
        let time_path = if claude_code {
            "/frontmatter/metadata/modified"
        } else {
            "/frontmatter/updated_at"
        };
        // Source modification time remains record time; absent or invalid time is not promoted to a fact timestamp.
        if let Some(value) = original.fields.pointer(time_path)
            && let Some(time) = normalize::time(value)
        {
            record.updated_at = Some(time);
            normalize::map(&mut original, time_path, "/updated_at");
        }
        if let Some(tags) = original.fields.pointer("/frontmatter/tags") {
            if let Some(array) = tags
                .as_array()
                .filter(|array| array.iter().all(Value::is_string))
            {
                record.tags = Some(
                    array
                        .iter()
                        .map(|value| value.as_str().unwrap().into())
                        .collect(),
                );
                normalize::map(&mut original, "/frontmatter/tags", "/tags");
            } else if let Some(text) = tags.as_str() {
                record.tags = Some(vec![text.into()]);
                normalize::map(&mut original, "/frontmatter/tags", "/tags");
            }
        }
        normalize::finish(&mut output, record, original)?;
        Ok(output)
    }
}
