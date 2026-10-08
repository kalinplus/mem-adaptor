//! Converts Claude memory and project exports into paired records before engine planning.
//! Recognition uses local source evidence; damaged recognized exports fail without exposing source values.
//! New memory files supersede legacy blocks; this adapter neither extracts transcripts nor writes targets.

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::content_hash;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use serde_json::{Value, json};

pub struct ClaudeReader;

impl Reader for ClaudeReader {
    /// Identifies this source adapter in inventories and records.
    fn id(&self) -> &'static str {
        "claude"
    }
    /// Records the compiled interpretation version.
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    /// Claude exports are downloaded bundles; their path changes per download and never binds a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::ExportBundle
    }
    /// Recognizes shared names by shape or same-directory evidence without treating parse failure as absence.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .iter()
            .filter_map(|(path, bytes)| {
                let name = path.rsplit('/').next()?;
                let layer = match name {
                    "memories.json" => {
                        let data = serde_json::from_slice::<Value>(bytes).ok();
                        if !data.as_ref().is_some_and(|data| {
                            memory_shape(data)
                                || data.as_array().is_some_and(Vec::is_empty)
                                    && !chatgpt_marker(inventory, path)
                        }) && !source_evidence(inventory, path)
                        {
                            return None;
                        }
                        "saved_memory"
                    }
                    "projects.json" => "project_doc",
                    "users.json" => "account_metadata",
                    "conversations.json" => {
                        let recognized = serde_json::from_slice::<Value>(bytes).is_ok_and(|data| {
                            data.as_array().is_some_and(|entries| {
                                entries.iter().any(|entry| {
                                    entry.get("chat_messages").is_some()
                                        || entry.get("messages").is_some()
                                })
                            })
                        });
                        if !recognized && !source_evidence(inventory, path) {
                            return None;
                        }
                        "transcript"
                    }
                    _ => return None,
                };
                Some(Claim {
                    path: path.clone(),
                    layer: layer.into(),
                    registered_only: matches!(layer, "transcript" | "account_metadata"),
                })
            })
            .collect()
    }
    /// Locates ordinary export errors without including source values.
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        self.read_claim(claim, source).with_context(|| {
            format!(
                "Claude source at {}",
                mem_adaptor_core::gate::mask(&claim.path)
            )
        })
    }
}

impl ClaudeReader {
    /// Parses recognized arrays, retaining unused record metadata and refusing damaged content/protection fields.
    fn read_claim(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        let data: Value = serde_json::from_slice(source.file(&claim.path))
            .map_err(|error| anyhow::anyhow!("Invalid Claude JSON at line {}", error.line()))?;
        let entries = data
            .as_array()
            .context("Claude export must be a JSON array")?;
        if claim.registered_only {
            if claim.layer == "transcript" {
                ensure!(
                    entries.iter().all(|entry| entry.is_object()
                        && (entry.get("chat_messages").is_some()
                            || entry.get("messages").is_some())),
                    "Claude conversation entries must contain messages"
                );
            }
            output.registered_count = entries.len() as u64;
            return Ok(output);
        }
        if claim.layer == "project_doc" {
            for (project_index, project) in entries.iter().enumerate() {
                ensure!(
                    project.is_object(),
                    "Claude project at /{project_index} must be an object"
                );
                if let Some(prompt) = project.get("prompt_template") {
                    ensure!(
                        prompt.is_string(),
                        "Claude prompt_template at /{project_index}/prompt_template must be a string; fix the field before planning"
                    );
                }
                let project_id = project["uuid"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        content_hash(project["name"].as_str().unwrap_or("unknown").as_bytes())
                    });
                if let Some(docs) = project.get("docs") {
                    let docs = docs.as_array().with_context(|| {
                        format!("Claude project docs at /{project_index}/docs must be an array")
                    })?;
                    for (doc_index, doc) in docs.iter().enumerate() {
                        let locator = format!("{}#/{project_index}/docs/{doc_index}", claim.path);
                        let Some(content) = doc["content"].as_str() else {
                            normalize::anomaly(
                                &mut output,
                                &locator,
                                "missing_project_content",
                                "/content",
                                None,
                            );
                            continue;
                        };
                        let id = doc["uuid"].as_str().map(str::to_owned).unwrap_or_else(|| {
                            format!(
                                "{project_id}/{}",
                                doc["filename"]
                                    .as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| content_hash(content.as_bytes()))
                            )
                        });
                        let mut project_metadata = project.clone();
                        for key in ["docs", "prompt_template"] {
                            project_metadata.as_object_mut().unwrap().remove(key);
                        }
                        let fields = json!({"doc": doc, "project": project_metadata});
                        let mut record = normalize::record(
                            self.id(),
                            self.version(),
                            source.satellite_id.as_deref(),
                            &id,
                            &locator,
                            content,
                            EvidenceLevel::ThirdParty,
                        );
                        record.scope = Scope::Project;
                        record.scope_qualifier = Some(project_id.clone());
                        record.source_kind = Some("project_doc".into());
                        let mut original = normalize::source(&record, &locator, fields);
                        normalize::map(&mut original, "/doc/content", "/content");
                        if project["uuid"].is_string() {
                            normalize::map(&mut original, "/project/uuid", "/scope_qualifier");
                        }
                        if doc["uuid"].is_string() {
                            normalize::map(&mut original, "/doc/uuid", "/source_record_id");
                        }
                        if let Some(time) = normalize::time(&doc["updated_at"]) {
                            record.updated_at = Some(time);
                            normalize::map(&mut original, "/doc/updated_at", "/updated_at");
                        }
                        normalize::finish(&mut output, record, original)?;
                    }
                }
                if let Some(content) = project["prompt_template"]
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                {
                    let id = format!("{project_id}/prompt_template");
                    let locator = format!("{}#/{project_index}/prompt_template", claim.path);
                    let mut record = normalize::record(
                        self.id(),
                        self.version(),
                        source.satellite_id.as_deref(),
                        &id,
                        &locator,
                        content,
                        EvidenceLevel::ThirdParty,
                    );
                    record.scope = Scope::Project;
                    record.scope_qualifier = Some(project_id.clone());
                    record.source_kind = Some("instruction".into());
                    record.dna_class = DnaClass::Dna;
                    let mut fields = project.clone();
                    fields.as_object_mut().unwrap().remove("docs");
                    let mut original = normalize::source(&record, &locator, fields);
                    normalize::map(&mut original, "/prompt_template", "/content");
                    if project["uuid"].is_string() {
                        normalize::map(&mut original, "/uuid", "/scope_qualifier");
                    }
                    original
                        .field_map
                        .push(mem_adaptor_core::reports::FieldMapping {
                            source_path: "/prompt_template".into(),
                            canonical_path: "/dna_class".into(),
                            rule: Some("instruction_to_procedure_dna".into()),
                        });
                    normalize::finish(&mut output, record, original)?;
                }
            }
            return Ok(output);
        }
        for (account_index, account) in entries.iter().enumerate() {
            ensure!(
                account.is_object(),
                "Claude memory account at /{account_index} must be an object"
            );
            let account_id = account["account_uuid"]
                .as_str()
                .unwrap_or("unknown_account");
            if let Some(files) = account.get("memory_files") {
                let files = files.as_array().with_context(|| {
                    format!("Claude memory_files at /{account_index}/memory_files must be an array")
                })?;
                for (index, file) in files.iter().enumerate() {
                    let locator = format!("{}#/{account_index}/memory_files/{index}", claim.path);
                    let (Some(path), Some(content)) =
                        (file["path"].as_str(), file["content"].as_str())
                    else {
                        normalize::anomaly(&mut output, &locator, "invalid_memory_file", "", None);
                        continue;
                    };
                    let id = format!("{account_id}:{path}");
                    let mut record = normalize::record(
                        self.id(),
                        self.version(),
                        source.satellite_id.as_deref(),
                        &id,
                        &locator,
                        content,
                        EvidenceLevel::ThirdParty,
                    );
                    let mut account_metadata = account.clone();
                    for key in ["memory_files", "conversations_memory", "project_memories"] {
                        account_metadata.as_object_mut().unwrap().remove(key);
                    }
                    let mut fields = json!({"file": file, "account": account_metadata});
                    let (frontmatter, _) = normalize::markdown_document(content)?;
                    if let Some(yaml) = frontmatter {
                        if let Some(frontmatter) =
                            normalize::frontmatter(yaml).with_context(|| {
                                format!(
                                    "Claude memory at {}",
                                    mem_adaptor_core::gate::mask(&locator)
                                )
                            })?
                        {
                            fields["frontmatter"] = frontmatter;
                        }
                    } else if content.starts_with("---\n") || content.starts_with("---\r\n") {
                        normalize::anomaly(
                            &mut output,
                            &locator,
                            "frontmatter_unclosed",
                            "/file/content",
                            None,
                        );
                    }
                    let mut original = normalize::source(&record, &locator, fields);
                    normalize::map(&mut original, "/file/content", "/content");
                    normalize::map(&mut original, "/file/path", "/source_record_id");
                    if account["account_uuid"].is_string() {
                        record.owner_declared = Some(account_id.into());
                        normalize::map(&mut original, "/account/account_uuid", "/owner_declared");
                    }
                    let kind_path = if original.fields.pointer("/file/type").is_some() {
                        "/file/type"
                    } else {
                        "/frontmatter/type"
                    };
                    normalize::classify(&mut record, &mut original, kind_path)?;
                    if let Some(time) = normalize::time(&file["updated_at"]) {
                        record.updated_at = Some(time);
                        normalize::map(&mut original, "/file/updated_at", "/updated_at");
                    }
                    normalize::finish(&mut output, record, original)?;
                }
                if account.get("conversations_memory").is_some()
                    || account.get("project_memories").is_some()
                {
                    normalize::anomaly(
                        &mut output,
                        &claim.path,
                        "legacy_memory_fields_not_reimported",
                        &format!("/{account_index}"),
                        None,
                    );
                }
            } else {
                if let Some(content) = account.get("conversations_memory") {
                    ensure!(
                        content.is_string(),
                        "Claude conversations_memory at /{account_index}/conversations_memory must be a string; fix the field before planning"
                    );
                }
                if let Some(content) = account["conversations_memory"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    let id = format!("{account_id}:conversations_memory");
                    let locator = format!("{}#/{account_index}/conversations_memory", claim.path);
                    let mut record = normalize::record(
                        self.id(),
                        self.version(),
                        source.satellite_id.as_deref(),
                        &id,
                        &locator,
                        content,
                        EvidenceLevel::Inferred,
                    );
                    let mut fields = account.clone();
                    fields.as_object_mut().unwrap().remove("project_memories");
                    let mut original = normalize::source(&record, &locator, fields);
                    normalize::map(&mut original, "/conversations_memory", "/content");
                    if account["account_uuid"].is_string() {
                        record.owner_declared = Some(account_id.into());
                        normalize::map(&mut original, "/account_uuid", "/owner_declared");
                    }
                    normalize::finish(&mut output, record, original)?;
                }
                if let Some(projects) = account.get("project_memories") {
                    let projects = projects
                        .as_object()
                        .with_context(|| format!("Claude project_memories at /{account_index}/project_memories must be an object"))?;
                    for (project, value) in projects {
                        let locator = format!(
                            "{}#/{account_index}/project_memories/{}",
                            claim.path,
                            project.replace('~', "~0").replace('/', "~1")
                        );
                        let Some(content) = value.as_str() else {
                            normalize::anomaly(
                                &mut output,
                                &locator,
                                "invalid_project_memory",
                                "/project_memories",
                                None,
                            );
                            continue;
                        };
                        let id = format!("{account_id}:project:{project}");
                        let mut record = normalize::record(
                            self.id(),
                            self.version(),
                            source.satellite_id.as_deref(),
                            &id,
                            &locator,
                            content,
                            EvidenceLevel::ThirdParty,
                        );
                        record.scope = Scope::Project;
                        record.scope_qualifier = Some(project.clone());
                        record.source_kind = Some("project".into());
                        let mut account_metadata = account.clone();
                        for key in ["conversations_memory", "project_memories"] {
                            account_metadata.as_object_mut().unwrap().remove(key);
                        }
                        let mut original = normalize::source(
                            &record,
                            &locator,
                            json!({"content": content, "account": account_metadata, "project_uuid": project}),
                        );
                        normalize::map(&mut original, "/content", "/content");
                        if account["account_uuid"].is_string() {
                            record.owner_declared = Some(account_id.into());
                            normalize::map(
                                &mut original,
                                "/account/account_uuid",
                                "/owner_declared",
                            );
                        }
                        normalize::map(&mut original, "/project_uuid", "/scope_qualifier");
                        normalize::finish(&mut output, record, original)?;
                    }
                }
            }
        }
        Ok(output)
    }
}

/// Recognizes Claude-specific account fields even when the export outer container is damaged.
fn memory_shape(data: &Value) -> bool {
    let is_account = |entry: &Value| {
        [
            "account_uuid",
            "memory_files",
            "conversations_memory",
            "project_memories",
        ]
        .iter()
        .any(|key| entry.get(key).is_some())
    };
    is_account(data)
        || data
            .as_array()
            .is_some_and(|entries| entries.iter().any(is_account))
}

/// Uses only siblings, including legacy project-only memories, as source evidence for shared damaged files.
fn source_evidence(inventory: &FileInventory, path: &str) -> bool {
    let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
    inventory.iter().any(|(other, bytes)| {
        if other == path || other.rsplit_once('/').map_or("", |(parent, _)| parent) != parent {
            return false;
        }
        match other.rsplit('/').next().unwrap() {
            "projects.json" | "users.json" => true,
            "memories.json" => serde_json::from_slice::<Value>(bytes).is_ok_and(|data| {
                memory_shape(&data)
                    || data.as_array().is_some_and(Vec::is_empty)
                        && !chatgpt_marker(inventory, other)
            }),
            _ => false,
        }
    })
}

/// Avoids claiming ambiguous empty ChatGPT memories as Claude solely because their array contains no shape evidence.
fn chatgpt_marker(inventory: &FileInventory, path: &str) -> bool {
    let parent = path.rsplit_once('/').map(|(parent, _)| parent);
    inventory.keys().any(|other| {
        other.rsplit_once('/').map(|(parent, _)| parent) == parent
            && (matches!(
                other.rsplit('/').next().unwrap(),
                "user.json" | "memory.json" | "saved_memories.json" | "memories.chatgpt.json"
            ) || other.ends_with(".chatgpt.md"))
    })
}
