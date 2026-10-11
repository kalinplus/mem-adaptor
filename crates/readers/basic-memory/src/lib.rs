//! Reader for basic-memory project directories (Markdown + frontmatter, `permalink` identity).
//! Claims notes whose frontmatter carries basic-memory's stable identity and converts them into
//! canonical records with `permalink` as the source identity; the derived SQLite index is
//! registration-only, and the vector layer (model name lives in tool config, not records) is
//! reported rather than migrated. Relations stay as body bytes only and are reported as uncarried.

use anyhow::Context;
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::okf;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use regex::Regex;
use serde_json::{Value, json};

pub struct BasicMemoryReader;

/// Returns the raw frontmatter YAML text of a Markdown document, if any parses at all.
fn frontmatter_text(text: &str) -> Option<&str> {
    match normalize::markdown_document(text) {
        Ok((Some(yaml), _)) => Some(yaml),
        _ => None,
    }
}

impl Reader for BasicMemoryReader {
    /// Registers this adapter under basic-memory; records keep this source-system identity.
    fn id(&self) -> &'static str {
        "basic-memory"
    }
    /// Reports the compiled adapter version for reproducible source interpretation.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    /// A basic-memory project is a directory the user keeps in place, so its path can bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
    }

    /// Claims the derived SQLite index registration-only, plus notes whose frontmatter carries a
    /// string `permalink` (basic-memory's identity field) without an OKF envelope. Files without
    /// the signature stay with the Markdown reader; parse failures likewise fall through to it.
    /// An OKF home's native `index.md`/`log.md` stay with the Markdown reader even if basic-memory
    /// stamped a permalink onto them, mirroring that reader's home exemption.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        let home = inventory.get("index.md").is_some_and(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .is_some_and(|text| okf::native_index(text).unwrap_or(false))
        });
        let mut claims = Vec::new();
        if inventory.contains_key("memory.db") {
            claims.push(Claim {
                path: "memory.db".into(),
                layer: "index".into(),
                registered_only: true,
            });
        }
        for path in inventory.keys() {
            if !path.ends_with(".md")
                || path.starts_with(".mem-adaptor/")
                || path.ends_with(".chatgpt.md")
                || path.rsplit('/').next() == Some("MEMORY.md")
                || home && matches!(path.as_str(), "index.md" | "log.md")
            {
                continue;
            }
            let Some(text) = inventory
                .get(path)
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            else {
                continue;
            };
            let Some(yaml) = frontmatter_text(text) else {
                continue;
            };
            let Ok(Some(frontmatter)) = normalize::frontmatter(yaml) else {
                continue;
            };
            let is_ours = frontmatter.get("permalink").is_some_and(Value::is_string)
                && !frontmatter
                    .get("mem_adaptor_envelope")
                    .is_some_and(Value::is_string);
            if is_ours {
                claims.push(Claim {
                    path: path.clone(),
                    layer: "knowledge".into(),
                    registered_only: false,
                });
            }
        }
        claims
    }

    /// Parses a claimed UTF-8 note and preserves its body, identity, tags, and time; custom
    /// frontmatter fields stay as reported source metadata. Wiki-link relations are reported as
    /// body-only; this method does not write or approve migration.
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        self.read_claim(claim, source).with_context(|| {
            format!(
                "basic-memory source at {}",
                mem_adaptor_core::gate::mask(&claim.path)
            )
        })
    }
}

impl BasicMemoryReader {
    /// Extracts the permalink identity and mapped metadata; the derived index claim reports the
    /// unmigratable vector layer instead of parsing SQLite bytes.
    fn read_claim(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        if claim.registered_only {
            // The index is derived state (rebuildable via `basic-memory reindex`) and its vectors
            // come from an embedding model whose name lives in tool config, not in the records
            // (pattern two of the horizontal findings), so neither is migrated.
            normalize::anomaly(
                &mut output,
                &claim.path,
                "derived_vector_index_not_migrated",
                "/memory.db",
                None,
            );
            return Ok(output);
        }
        let text = std::str::from_utf8(source.file(&claim.path)).context("Invalid UTF-8 source")?;
        let (frontmatter, body) = normalize::markdown_document(text)?;
        let mut fields = json!({"body": body});
        let Some(yaml) = frontmatter else {
            anyhow::bail!("Claimed note has no frontmatter; the claim signature check is stale");
        };
        let Some(value) = normalize::frontmatter(yaml)? else {
            anyhow::bail!("Claimed note frontmatter is empty; the claim signature check is stale");
        };
        fields["frontmatter"] = value;
        let permalink = fields
            .pointer("/frontmatter/permalink")
            .and_then(Value::as_str)
            .context("Claimed note has no permalink; the claim signature check is stale")?
            .to_owned();
        let mut record = normalize::record(
            self.id(),
            self.version(),
            source.satellite_id.as_deref(),
            &permalink,
            &claim.path,
            body,
            EvidenceLevel::Measured,
        );
        // A filesystem scan is provenance for this import, not evidence that the scanner authored
        // the memory; notes themselves were authored by the user or their agent in basic-memory.
        record.provenance.actor = "process:filesystem".into();
        record.provenance.actor_kind = ActorKind::Scan;
        record.provenance.method = "filesystem".into();
        let mut original = normalize::source(&record, &claim.path, fields);
        normalize::map(&mut original, "/body", "/content");
        normalize::map(&mut original, "/frontmatter/permalink", "/source_record_id");
        // The display title is a rendering of the body's heading, not a canonical fact.
        normalize::map(&mut original, "/frontmatter/title", "");
        normalize::classify(&mut record, &mut original, "/frontmatter/type")?;
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
        // basic-memory's authoritative `modified` (falling back to `created`) is the record time;
        // absent or invalid time is not promoted to a fact timestamp.
        for time_path in ["/frontmatter/modified", "/frontmatter/created"] {
            if let Some(value) = original.fields.pointer(time_path)
                && let Some(time) = normalize::time(value)
            {
                record.updated_at = Some(time);
                normalize::map(&mut original, time_path, "/updated_at");
                break;
            }
        }
        // Relations live in body bytes (`[[wiki links]]`); the canonical model has no edge shape,
        // so links survive only as text and the loss is reported instead of silently dropped.
        let links = Regex::new(r"\[\[[^\]]+\]\]").unwrap();
        if links.is_match(body) {
            normalize::anomaly(
                &mut output,
                &claim.path,
                "relations_body_only",
                "/body",
                None,
            );
        }
        normalize::finish(&mut output, record, original)?;
        Ok(output)
    }
}
