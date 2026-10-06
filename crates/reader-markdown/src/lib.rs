//! Reader for local Markdown, Obsidian, Claude Code memory, and OKF homes.

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::content_hash;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use serde_json::{Value, json};

pub struct MarkdownReader;

impl Reader for MarkdownReader {
    fn id(&self) -> &'static str {
        "markdown"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        let home = inventory.get("index.md").is_some_and(|bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .and_then(|text| normalize::markdown_document(text).ok())
                .and_then(|(yaml, _)| yaml)
                .and_then(|yaml| serde_saphyr::from_str::<Value>(yaml).ok())
                .is_some_and(|value| value["type"] == "Index" && value.get("okf_version").is_some())
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

    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        let text = std::str::from_utf8(source.file(&claim.path))
            .with_context(|| format!("Invalid UTF-8 source: {}", claim.path))?;
        if claim.registered_only {
            output.registered_count = text
                .lines()
                .filter(|line| line.trim_start().starts_with("- ["))
                .count() as u64;
            return Ok(output);
        }
        let (frontmatter, body) = normalize::markdown_document(text)?;
        let mut fields = json!({"body": body});
        if let Some(yaml) = frontmatter {
            let value: Value = serde_saphyr::from_str(yaml)
                .map_err(|_| anyhow::anyhow!("Invalid YAML frontmatter: {}", claim.path))?;
            ensure!(value.is_object(), "Markdown frontmatter must be an object");
            fields["frontmatter"] = value;
        }
        let mut record;
        if let Some(extension) = fields.pointer("/frontmatter/mem_adaptor") {
            ensure!(
                fields["frontmatter"]["type"] == "Memory",
                "Unexpected OKF memory type"
            );
            let mut extension = extension.clone();
            extension["content"] = Value::String(body.into());
            record = serde_json::from_value::<CanonicalRecord>(extension)
                .map_err(|_| anyhow::anyhow!("Invalid OKF memory fields"))?;
            if record.content_hash != content_hash(body.as_bytes()) {
                normalize::anomaly(&mut output, &claim.path, "okf_body_changed", "/body", None);
                record.content_hash = content_hash(body.as_bytes());
            }
            let mut original = normalize::source(&record, &claim.path, fields);
            normalize::map(&mut original, "/body", "/content");
            normalize::map(&mut original, "/frontmatter/mem_adaptor", "");
            normalize::map(&mut original, "/frontmatter/type", "/source");
            if original
                .fields
                .pointer("/frontmatter/mem_adaptor_envelope")
                .and_then(Value::as_str)
                == Some("okf:0.2")
            {
                normalize::map(
                    &mut original,
                    "/frontmatter/mem_adaptor_envelope",
                    "/source",
                );
                if original
                    .fields
                    .pointer("/frontmatter/title")
                    .and_then(Value::as_str)
                    == Some(mem_adaptor_core::writer::title(&record).as_str())
                {
                    normalize::map(&mut original, "/frontmatter/title", "/content");
                }
                let authored = matches!(
                    record.provenance.actor_kind,
                    ActorKind::User | ActorKind::Agent | ActorKind::Model
                );
                let mut origin =
                    json!({"id": record.source_record_id, "resource": record.source_locator});
                if authored {
                    origin["author"] = Value::String(record.provenance.actor.clone());
                }
                if let Some(modified) = &record.updated_at {
                    origin["last_modified"] = Value::String(modified.clone());
                }
                let generated = record.updated_at.as_ref().map(|modified| {
                    let mut value = json!({"at": modified});
                    if authored {
                        value["by"] = Value::String(record.provenance.actor.clone());
                    }
                    value
                });
                for (field, expected, canonical) in [
                    ("sources", Some(json!([origin])), "/provenance"),
                    ("generated", generated, "/updated_at"),
                    (
                        "tags",
                        record.tags.as_ref().map(|tags| json!(tags)),
                        "/tags",
                    ),
                ] {
                    ensure!(
                        original.fields["frontmatter"].get(field) == expected.as_ref(),
                        "OKF native projection differs from canonical metadata"
                    );
                    if expected.is_some() {
                        normalize::map(&mut original, &format!("/frontmatter/{field}"), canonical);
                    }
                }
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
            &claim.path,
            &claim.path,
            body,
            EvidenceLevel::Measured,
        );
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
        normalize::classify(&mut record, &mut original, kind_path);
        if claude_code {
            record.scope = Scope::Project;
            record.scope_qualifier = Some(format!(
                "{}#{}",
                source.root.display(),
                claim.path.rsplit_once('/').map_or("", |(parent, _)| parent)
            ));
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
