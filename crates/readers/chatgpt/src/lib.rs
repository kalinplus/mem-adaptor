//! Claims and normalizes local ChatGPT saved memories and source-marked Prompt exports before engine planning.
//! Shared JSON names require a recognizable shape or evidence in the exact same parent directory.
//! Transcripts/user metadata are registration-only; unknown global envelope fields are reported without values.
//! Prompt identities deduplicate across one SourceFs collection without persistent Reader state or remote calls.

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::content_hash;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use mem_adaptor_core::reports::SourceUnavailable;
use regex::Regex;
use serde_json::{Value, json};

pub struct ChatgptReader;

impl Reader for ChatgptReader {
    /// Identifies this source adapter for registry claims and canonical provenance.
    fn id(&self) -> &'static str {
        "chatgpt"
    }
    /// Records the implementation version without inventing a source export version.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    /// ChatGPT exports are downloaded bundles; their path changes per download and never binds a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::ExportBundle
    }
    /// Claims explicit source names even when corrupt so read can return a parse failure.
    /// Ambiguous JSON names need their own shape or same-parent evidence; descendant files are not evidence.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .iter()
            .filter_map(|(path, bytes)| {
                let name = path.rsplit('/').next()?;
                let layer = if matches!(
                    name,
                    "memory.json" | "saved_memories.json" | "memories.chatgpt.json"
                ) {
                    "saved_memory"
                } else if name == "memories.json" {
                    if serde_json::from_slice::<Value>(bytes).is_ok_and(|data| memory_shape(&data))
                        || same_parent_evidence(path, inventory)
                    {
                        "saved_memory"
                    } else {
                        return None;
                    }
                } else if path.ends_with(".chatgpt.md") {
                    "prompt_extract"
                } else if name == "user.json" {
                    "instruction"
                } else if name == "conversations.json" {
                    if !serde_json::from_slice::<Value>(bytes).is_ok_and(|data| {
                        data.as_array().is_some_and(|entries| {
                            entries.iter().any(|entry| entry.get("mapping").is_some())
                        })
                    }) && !same_parent_evidence(path, inventory)
                    {
                        return None;
                    }
                    "transcript"
                } else {
                    return None;
                };
                Some(Claim {
                    path: path.clone(),
                    layer: layer.into(),
                    registered_only: matches!(layer, "transcript" | "instruction"),
                })
            })
            .collect()
    }
    /// Parses a claimed file into paired records or registration counts without writing a target.
    /// File-level corruption fails; supported bad entries/Prompt lines produce value-free location diagnostics.
    /// Earlier source-marked Prompt files reserve identities, making representatives independent of read order.
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        self.read_claim(claim, source).with_context(|| {
            format!(
                "ChatGPT source at {}",
                mem_adaptor_core::gate::mask(&claim.path)
            )
        })
    }
}

impl ChatgptReader {
    /// Normalizes one recognized export while keeping collection deduplication deterministic and source fields record-local.
    fn read_claim(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        output.source_unavailable = vec![
            SourceUnavailable { system: self.id().into(), layer: "memory_summary".into(), reason: "The export cannot prove coverage of the live synthesized memory layer.".into(), evidence_level: EvidenceLevel::ThirdParty },
            SourceUnavailable { system: self.id().into(), layer: "provenance".into(), reason: "Per-memory originating conversations are not available; prompt dates are self-reported.".into(), evidence_level: EvidenceLevel::ThirdParty },
        ];
        if claim.layer == "prompt_extract" {
            let text = std::str::from_utf8(source.file(&claim.path))
                .context("ChatGPT prompt export is not UTF-8")?;
            let pattern = Regex::new(r"^\[([^\]]+)\]\s+\[([^\]]+)\]\s+(.+)$")?;
            let mut identities = std::collections::BTreeSet::new();
            for (path, bytes) in source
                .files
                .iter()
                .take_while(|(path, _)| path.as_str() < claim.path.as_str())
                .filter(|(path, _)| path.ends_with(".chatgpt.md"))
            {
                let earlier = std::str::from_utf8(bytes).with_context(|| {
                    format!(
                        "Earlier ChatGPT prompt export is not UTF-8: {}",
                        mem_adaptor_core::gate::mask(path)
                    )
                })?;
                for line in earlier.lines().map(str::trim) {
                    if let Some(captures) = pattern.captures(line) {
                        identities.insert(prompt_identity(&captures));
                    }
                }
            }
            for (index, raw) in text.lines().enumerate() {
                let line = raw.trim();
                if line.is_empty() || line.starts_with("```") {
                    continue;
                }
                let Some(captures) = pattern.captures(line) else {
                    normalize::anomaly(
                        &mut output,
                        &claim.path,
                        "invalid_prompt_line",
                        "",
                        Some(index as u64 + 1),
                    );
                    continue;
                };
                let date = captures[1].trim();
                let kind = captures[2].trim();
                let content = &captures[3];
                let id = prompt_identity(&captures);
                if !identities.insert(id.clone()) {
                    normalize::anomaly(
                        &mut output,
                        &claim.path,
                        "duplicate_prompt_line",
                        "",
                        Some(index as u64 + 1),
                    );
                    continue;
                }
                let locator = format!("{}:{}", claim.path, index + 1);
                let mut record = normalize::record(
                    self.id(),
                    self.version(),
                    source.satellite_id.as_deref(),
                    &id,
                    &locator,
                    content,
                    EvidenceLevel::ThirdParty,
                );
                record.provenance.method = "prompt_extract".into();
                let mut original = normalize::source(
                    &record,
                    &locator,
                    json!({"date": date, "kind": kind, "content": content}),
                );
                normalize::map(&mut original, "/content", "/content");
                normalize::classify(&mut record, &mut original, "/kind")?;
                if !matches!(
                    kind,
                    "profile" | "preference" | "instruction" | "project" | "tool"
                ) {
                    normalize::anomaly(
                        &mut output,
                        &locator,
                        "unknown_source_kind",
                        "/kind",
                        Some(index as u64 + 1),
                    );
                }
                if date != "unknown" && !valid_date(date) {
                    normalize::anomaly(
                        &mut output,
                        &locator,
                        "invalid_prompt_date",
                        "/date",
                        Some(index as u64 + 1),
                    );
                }
                normalize::finish(&mut output, record, original)?;
            }
            return Ok(output);
        }
        let data: Value = serde_json::from_slice(source.file(&claim.path))
            .map_err(|error| anyhow::anyhow!("Invalid ChatGPT JSON at line {}", error.line()))?;
        if claim.registered_only {
            if claim.layer == "transcript" {
                let entries = data
                    .as_array()
                    .context("ChatGPT conversations must be an array")?;
                for (index, entry) in entries.iter().enumerate() {
                    ensure!(
                        entry.get("mapping").is_some_and(Value::is_object),
                        "ChatGPT conversation at index {index} must contain a mapping object"
                    );
                }
                output.registered_count = entries.len() as u64;
            } else {
                ensure!(data.is_object(), "ChatGPT user metadata must be an object");
                output.registered_count = 1;
                output.source_unavailable.push(SourceUnavailable { system: self.id().into(), layer: "instruction".into(), reason: "User instruction field placement is not verified; file registered but not parsed.".into(), evidence_level: EvidenceLevel::ThirdParty });
            }
            return Ok(output);
        }
        let entries = if let Some(array) = data.as_array() {
            array
        } else {
            data.get("memory")
                .and_then(Value::as_array)
                .context("ChatGPT memory must be an array or an object containing memory")?
        };
        if claim.path.rsplit('/').next() == Some("memories.json") {
            ensure!(
                entries.is_empty()
                    || entries.iter().any(|item| {
                        item.get("content").is_some()
                            || item.get("text").is_some()
                            || item.get("deleted") == Some(&Value::Bool(true))
                    }),
                "ChatGPT memory entries have an incompatible shape"
            );
        }
        if let Some(envelope) = data.as_object() {
            for (key, value) in envelope {
                if key != "memory" {
                    report_envelope_field(&mut output, &claim.path, &pointer_key(key), value);
                }
            }
        }
        for (index, item) in entries.iter().enumerate() {
            if item["deleted"] == true {
                output.deleted_count += 1;
                continue;
            }
            let locator = format!(
                "{}#{}/{index}",
                claim.path,
                if data.is_array() { "" } else { "/memory" }
            );
            let content_key = if item["content"].is_string() {
                "content"
            } else {
                "text"
            };
            let Some(content) = item[content_key].as_str() else {
                normalize::anomaly(
                    &mut output,
                    &locator,
                    "missing_memory_content",
                    &format!("/{content_key}"),
                    None,
                );
                continue;
            };
            if item["content"].is_string()
                && item["text"].is_string()
                && item["content"] != item["text"]
            {
                normalize::anomaly(
                    &mut output,
                    &locator,
                    "conflicting_memory_content",
                    "/content",
                    None,
                );
                continue;
            }
            let id = item["id"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| content_hash(content.as_bytes()));
            let mut record = normalize::record(
                self.id(),
                self.version(),
                source.satellite_id.as_deref(),
                &id,
                &locator,
                content,
                EvidenceLevel::ThirdParty,
            );
            let mut original = normalize::source(&record, &locator, item.clone());
            normalize::map(&mut original, &format!("/{content_key}"), "/content");
            for key in ["content", "text"] {
                if key != content_key && item[key].as_str() == Some(content) {
                    normalize::map(&mut original, &format!("/{key}"), "/content");
                }
            }
            if item["id"].is_string() {
                normalize::map(&mut original, "/id", "/source_record_id");
            }
            let kind = if item.get("kind").is_some() {
                "/kind"
            } else {
                "/type"
            };
            normalize::classify(&mut record, &mut original, kind)?;
            if let Some(enabled) = item["enabled"].as_bool() {
                record.consent = Some(Consent {
                    memory_enabled: Some(enabled),
                    exportable: None,
                    retention: None,
                    redact: None,
                });
                normalize::map(&mut original, "/enabled", "/consent/memory_enabled");
            }
            for (key, canonical) in [("created_at", "/created_at"), ("updated_at", "/updated_at")] {
                if let Some(time) = normalize::time(&item[key]) {
                    if key == "created_at" {
                        record.created_at = Some(time);
                    } else {
                        record.updated_at = Some(time);
                    }
                    normalize::map(&mut original, &format!("/{key}"), canonical);
                }
            }
            normalize::finish(&mut output, record, original)?;
        }
        Ok(output)
    }
}

/// Recognizes a ChatGPT memory envelope or memory-shaped array without accepting an isolated empty array.
fn memory_shape(data: &Value) -> bool {
    data.get("memory").is_some()
        || data.as_array().is_some_and(|entries| {
            entries
                .iter()
                .any(|item| item.get("content").is_some() || item.get("text").is_some())
        })
}

/// Compares exact inventory parents so a child or sibling export cannot identify an ambiguous file.
fn same_parent_evidence(path: &str, inventory: &FileInventory) -> bool {
    let parent = path.rsplit_once('/').map(|(parent, _)| parent);
    inventory.iter().any(|(candidate, bytes)| {
        if candidate == path || candidate.rsplit_once('/').map(|(parent, _)| parent) != parent {
            return false;
        }
        let name = candidate.rsplit('/').next().unwrap();
        matches!(
            name,
            "user.json" | "memory.json" | "saved_memories.json" | "memories.chatgpt.json"
        ) || candidate.ends_with(".chatgpt.md")
            || (name == "memories.json"
                && serde_json::from_slice::<Value>(bytes).is_ok_and(|data| memory_shape(&data)))
    })
}

/// Hashes normalized date/category/body identity while leaving the representative's body bytes unchanged.
fn prompt_identity(captures: &regex::Captures<'_>) -> String {
    content_hash(
        format!(
            "{}\0{}\0{}",
            captures[1].trim(),
            captures[2].trim(),
            captures[3].split_whitespace().collect::<Vec<_>>().join(" ")
        )
        .as_bytes(),
    )
}

/// Escapes an object member as one JSON Pointer component without embedding its value.
fn pointer_key(key: &str) -> String {
    format!("/{}", key.replace('~', "~0").replace('/', "~1"))
}

/// Reports uncarried global envelope leaves, including empty containers, without copying global values into records.
fn report_envelope_field(output: &mut ReaderOutput, locator: &str, path: &str, value: &Value) {
    if let Some(object) = value.as_object().filter(|object| !object.is_empty()) {
        for (key, value) in object {
            report_envelope_field(
                output,
                locator,
                &format!("{path}{}", pointer_key(key)),
                value,
            );
        }
    } else {
        normalize::anomaly(output, locator, "uncarried_export_field", path, None);
    }
}

/// Validates a self-reported calendar date without promoting it to a timestamp or inventing a timezone.
fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(i, byte)| i == 4 || i == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let year: i32 = date[..4].parse().unwrap();
    let month: u8 = date[5..7].parse().unwrap();
    let day: u8 = date[8..].parse().unwrap();
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => return false,
    };
    day > 0 && day <= days
}
