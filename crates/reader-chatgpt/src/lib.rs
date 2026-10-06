//! Reader for ChatGPT exports and explicitly exported memory text.

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
    fn id(&self) -> &'static str {
        "chatgpt"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
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
                    let data: Value = serde_json::from_slice(bytes).ok()?;
                    if data.get("memory").is_some()
                        || data.as_array().is_some_and(|array| {
                            array.iter().any(|item| {
                                item.get("content").is_some() || item.get("text").is_some()
                            })
                        })
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
                    let array: Value = serde_json::from_slice(bytes).ok()?;
                    let entries = array.as_array()?;
                    let prefix = path
                        .rsplit_once('/')
                        .map_or("", |(prefix, _)| &path[..prefix.len() + 1]);
                    if entries.is_empty()
                        && !["user.json", "memory.json", "saved_memories.json"]
                            .iter()
                            .any(|name| inventory.contains_key(&format!("{prefix}{name}")))
                        && !inventory
                            .keys()
                            .any(|file| file.starts_with(prefix) && file.ends_with(".chatgpt.md"))
                    {
                        return None;
                    }
                    if !entries.is_empty()
                        && !entries.iter().any(|entry| entry.get("mapping").is_some())
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
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        output.source_unavailable = vec![
            SourceUnavailable { system: self.id().into(), layer: "memory_summary".into(), reason: "The export cannot prove coverage of the live synthesized memory layer.".into(), evidence_level: EvidenceLevel::ThirdParty },
            SourceUnavailable { system: self.id().into(), layer: "provenance".into(), reason: "Per-memory originating conversations are not available; prompt dates are self-reported.".into(), evidence_level: EvidenceLevel::ThirdParty },
        ];
        if claim.layer == "prompt_extract" {
            let text = std::str::from_utf8(source.file(&claim.path))
                .context("ChatGPT prompt export is not UTF-8")?;
            let pattern = Regex::new(r"^\[([^\]]+)\]\s+\[([^\]]+)\]\s+(.+)$").unwrap();
            let mut identities = std::collections::BTreeSet::new();
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
                let date = &captures[1];
                let kind = &captures[2];
                let content = &captures[3];
                let id = content_hash(
                    format!(
                        "{date}\0{kind}\0{}",
                        content.split_whitespace().collect::<Vec<_>>().join(" ")
                    )
                    .as_bytes(),
                );
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
                normalize::classify(&mut record, &mut original, "/kind");
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
                output.registered_count = data
                    .as_array()
                    .context("ChatGPT conversations must be an array")?
                    .len() as u64;
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
            data["memory"]
                .as_array()
                .context("ChatGPT memory must be an array or an object containing memory")?
        };
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
            let kind = if item["kind"].is_string() {
                "/kind"
            } else {
                "/type"
            };
            normalize::classify(&mut record, &mut original, kind);
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
