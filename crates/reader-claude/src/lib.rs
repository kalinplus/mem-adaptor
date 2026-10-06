//! Reader for Claude memory and project exports.

use anyhow::{Context, ensure};
use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::*;
use mem_adaptor_core::engine::content_hash;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as normalize;
use serde_json::{Value, json};

pub struct ClaudeReader;

impl Reader for ClaudeReader {
    fn id(&self) -> &'static str {
        "claude"
    }
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .iter()
            .filter_map(|(path, bytes)| {
                let name = path.rsplit('/').next()?;
                let layer = match name {
                    "memories.json" => {
                        let data: Value = serde_json::from_slice(bytes).ok()?;
                        let entries = data.as_array()?;
                        if !entries.is_empty()
                            && !entries.iter().any(|entry| {
                                [
                                    "account_uuid",
                                    "memory_files",
                                    "conversations_memory",
                                    "project_memories",
                                ]
                                .iter()
                                .any(|key| entry.get(key).is_some())
                            })
                        {
                            return None;
                        }
                        "saved_memory"
                    }
                    "projects.json" => "project_doc",
                    "users.json" => "account_metadata",
                    "conversations.json" => {
                        let data: Value = serde_json::from_slice(bytes).ok()?;
                        let entries = data.as_array()?;
                        let prefix = path
                            .rsplit_once('/')
                            .map_or("", |(prefix, _)| &path[..prefix.len() + 1]);
                        if entries.is_empty()
                            && !["projects.json", "users.json"]
                                .iter()
                                .any(|name| inventory.contains_key(&format!("{prefix}{name}")))
                            && !inventory
                                .get(&format!("{prefix}memories.json"))
                                .is_some_and(|bytes| {
                                    serde_json::from_slice::<Value>(bytes).is_ok_and(|data| {
                                        data.as_array().is_some_and(|array| {
                                            array.is_empty()
                                                || array.iter().any(|item| {
                                                    item.get("account_uuid").is_some()
                                                        || item.get("memory_files").is_some()
                                                        || item
                                                            .get("conversations_memory")
                                                            .is_some()
                                                })
                                        })
                                    })
                                })
                        {
                            return None;
                        }
                        if !entries.is_empty()
                            && !entries.iter().any(|entry| {
                                entry.get("chat_messages").is_some()
                                    || entry.get("messages").is_some()
                            })
                        {
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
    fn read(&self, claim: &Claim, source: &SourceFs) -> Result<ReaderOutput> {
        let mut output = normalize::output();
        let data: Value = serde_json::from_slice(source.file(&claim.path))
            .map_err(|error| anyhow::anyhow!("Invalid Claude JSON at line {}", error.line()))?;
        let entries = data
            .as_array()
            .context("Claude export must be a JSON array")?;
        if claim.registered_only {
            output.registered_count = entries.len() as u64;
            return Ok(output);
        }
        if claim.layer == "project_doc" {
            for (project_index, project) in entries.iter().enumerate() {
                ensure!(project.is_object(), "Claude project must be an object");
                let project_id = project["uuid"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        content_hash(project["name"].as_str().unwrap_or("unknown").as_bytes())
                    });
                if let Some(docs) = project.get("docs") {
                    let docs = docs
                        .as_array()
                        .context("Claude project docs must be an array")?;
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
                    if project.get("uuid").is_some() {
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
                "Claude memory account must be an object"
            );
            let account_id = account["account_uuid"]
                .as_str()
                .unwrap_or("unknown_account");
            if let Some(files) = account.get("memory_files") {
                let files = files
                    .as_array()
                    .context("Claude memory_files must be an array")?;
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
                        let frontmatter: Value = serde_saphyr::from_str(yaml)
                            .map_err(|_| anyhow::anyhow!("Invalid Claude memory frontmatter"))?;
                        ensure!(
                            frontmatter.is_object(),
                            "Claude memory frontmatter must be an object"
                        );
                        fields["frontmatter"] = frontmatter;
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
                    normalize::classify(&mut record, &mut original, kind_path);
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
                if let Some(content) = account["conversations_memory"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    let id = format!("{account_id}:conversations_memory");
                    let locator = format!("{}#/{account_index}/conversations_memory", claim.path);
                    let mut record = normalize::record(
                        self.id(),
                        self.version(),
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
                        .context("Claude project_memories must be an object")?;
                    for (project, value) in projects {
                        let Some(content) = value.as_str() else {
                            normalize::anomaly(
                                &mut output,
                                &claim.path,
                                "invalid_project_memory",
                                "/project_memories",
                                None,
                            );
                            continue;
                        };
                        let id = format!("{account_id}:project:{project}");
                        let locator = format!(
                            "{}#/{account_index}/project_memories/{}",
                            claim.path,
                            project.replace('~', "~0").replace('/', "~1")
                        );
                        let mut record = normalize::record(
                            self.id(),
                            self.version(),
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
