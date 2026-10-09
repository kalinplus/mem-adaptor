//! Shared OKF envelope classification and native projection for Markdown Reader and OKF Writer.
//! Preserves canonical source identity and raw actors; native author/time fields never invent provenance.

use anyhow::{Context, ensure};
use regex::Regex;
use serde_json::{Value, json};
use std::sync::LazyLock;

use crate::canonical::{ActorKind, CanonicalRecord};

pub const INDEX_MARKER: &str = "<!-- mem-adaptor:okf-index:v1 -->";
pub const LOG_MARKER: &str = "<!-- mem-adaptor:okf-log:v1 -->";

/// Identifies the fixed canonical-id alphabet without accepting partial or extended filenames.
pub fn valid_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte))
}

/// Conservatively recognizes reserved mapping keys in block/flow YAML before malformed metadata can be skipped.
/// Valid YAML is classified structurally; this fallback only refuses a damaged explicit management declaration.
fn claims_envelope(yaml: &str) -> bool {
    static CLAIM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?m)(?:^[ \t]*|[,{][ \t]*)["']?mem_adaptor(?:_envelope)?["']?[ \t]*:"#)
            .unwrap()
    });
    CLAIM.is_match(yaml)
}

/// Returns managed metadata, leaves ordinary Markdown unowned, and rejects damaged claimed envelopes.
/// Only the shared standalone-line parser decides frontmatter boundaries, including CRLF documents.
pub fn managed_metadata(text: &str) -> crate::Result<Option<Value>> {
    let (yaml, _) = crate::reader::markdown_document(text)?;
    let Some(yaml) = yaml else {
        let opened = text
            .strip_prefix("---\r\n")
            .or_else(|| text.strip_prefix("---\n"));
        ensure!(
            !opened.is_some_and(claims_envelope),
            "Unclosed managed OKF frontmatter"
        );
        return Ok(None);
    };
    let claimed = claims_envelope(yaml);
    let parsed = crate::reader::frontmatter(yaml);
    let metadata = match parsed {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return Ok(None),
        Err(error) if claimed => return Err(error.context("Invalid managed OKF frontmatter")),
        Err(_) => return Ok(None),
    };
    if metadata.get("mem_adaptor").is_none() && metadata.get("mem_adaptor_envelope").is_none() {
        return Ok(None);
    }
    ensure!(
        metadata["mem_adaptor_envelope"] == "okf:0.2",
        "Unsupported managed OKF envelope"
    );
    ensure!(metadata["type"] == "Memory", "Unexpected OKF memory type");
    ensure!(
        metadata["mem_adaptor"].is_object(),
        "OKF mem_adaptor must be an object"
    );
    Ok(Some(metadata))
}

/// Restores the canonical extension from a home copy without re-deriving identity from run context (DEC-20).
/// The stored canonical_id and satellite_id are authoritative: schema validation enforces their format, and
/// the consistency check only verifies that the stored identity agrees with the record's own stored source
/// fields under the one unconditional formula; a never-substituted, internally inconsistent envelope is an
/// error rather than a silent repair. Satellite-chain attribution belongs to engine receipt binding, and
/// body-hash mismatches surface through callers rather than being repaired here.
pub fn restore(metadata: &Value, body: &str) -> crate::Result<CanonicalRecord> {
    let mut extension = metadata["mem_adaptor"].clone();
    let object = extension
        .as_object_mut()
        .context("Missing OKF metadata extension")?;
    object.insert("content".into(), Value::String(body.into()));
    let record: CanonicalRecord = serde_json::from_value(extension)
        .map_err(|_| anyhow::anyhow!("Invalid OKF record fields"))?;
    crate::schema::validate("canonical-record", &record)?;
    ensure!(
        record.canonical_id
            == crate::engine::canonical_id(
                &record.source.system,
                record.source.satellite_id.as_deref().unwrap_or(""),
                &record.source_record_id
            ),
        "Managed OKF source identity mismatch"
    );
    Ok(record)
}

/// Converts only known authors to native actors while leaving scan/import provenance out of authorship.
fn author(record: &CanonicalRecord) -> Option<String> {
    match record.provenance.actor_kind {
        ActorKind::User => Some(if record.provenance.actor.starts_with("human:") {
            record.provenance.actor.clone()
        } else {
            format!("human:{}", record.provenance.actor)
        }),
        ActorKind::Agent | ActorKind::Model => Some(record.provenance.actor.clone()),
        _ => None,
    }
}

/// Projects the shared native fields; source modification time survives even when authorship is unavailable.
pub fn native_projection(record: &CanonicalRecord) -> Value {
    let author = author(record);
    let mut source = json!({"id": record.source_record_id, "resource": record.source_locator});
    if let Some(author) = &author {
        source["author"] = json!(author);
    }
    if let Some(time) = &record.updated_at {
        source["last_modified"] = json!(time);
    }
    let mut native = json!({
        "type": "Memory",
        "title": crate::writer::title(record),
        "sources": [source],
    });
    if let (Some(author), Some(time)) = (author, &record.updated_at) {
        native["generated"] = json!({"by": author, "at": time});
    }
    if let Some(tags) = &record.tags {
        native["tags"] = json!(tags);
    }
    native
}

/// Reports which advisory native fields diverge from the envelope-stored record (DEC-21 A).
/// Provenance (`sources`/`generated`) is derived from envelope-stored fields, so its divergence is
/// unambiguous. The `title` judgement is deliberately the display-value rule DEC-21 A describes: it
/// compares against the value derived from the current body, so a body edit makes a stale title line
/// diverge — both readings get the same advisory treatment, so the ambiguity never changes a decision.
/// Divergence is not an error; body/tags adoptions are handled by `apply_home_edits`.
pub struct NativeDivergence {
    pub title: bool,
    pub sources: bool,
    pub generated: bool,
}

/// Classifies advisory native-field divergence of a managed file against its envelope-stored record.
pub fn classify_native(metadata: &Value, record: &CanonicalRecord) -> NativeDivergence {
    let expected = native_projection(record);
    NativeDivergence {
        title: metadata.get("title") != expected.get("title"),
        sources: metadata.get("sources") != expected.get("sources"),
        generated: metadata.get("generated") != expected.get("generated"),
    }
}

/// Outcome of applying home-side edits to a restored record, so callers can report what was adopted.
pub struct HomeEdits {
    pub body_changed: bool,
    pub tags_adopted: bool,
}

/// Applies home-side edits to a restored managed record (DEC-21 A): the current body is the fact, so its
/// hash is re-homed, and frontmatter tags diverging from the envelope are a new fact adopted into the
/// canonical record. Provenance stays envelope-authoritative and is never adopted here. A tags value
/// that is neither absent nor an all-string array cannot be adopted and keeps the envelope value.
pub fn apply_home_edits(record: &mut CanonicalRecord, metadata: &Value, body: &str) -> HomeEdits {
    let body_changed = record.content_hash != crate::engine::content_hash(body.as_bytes());
    if body_changed {
        record.content_hash = crate::engine::content_hash(body.as_bytes());
    }
    // A missing or null tags key states "no tags". A bare string adopts as a one-element list, the
    // same interpretation the ordinary-note path uses; any other non-string-array shape is not
    // adoptable and keeps the envelope value instead of guessing a coercion.
    let home_tags = match metadata.get("tags") {
        None => Some(None),
        Some(tags) if tags.is_null() => Some(None),
        Some(tags) if tags.is_string() => Some(Some(vec![tags.as_str().unwrap().to_owned()])),
        Some(tags)
            if tags
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)) =>
        {
            Some(Some(
                tags.as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            ))
        }
        Some(_) => None,
    };
    let tags_adopted = home_tags.as_ref().is_some_and(|home| home != &record.tags);
    if tags_adopted && let Some(home) = home_tags {
        record.tags = home;
    }
    HomeEdits {
        body_changed,
        tags_adopted,
    }
}

/// Recognizes the native version-only root index independently of Writer ownership.
pub fn native_index(text: &str) -> crate::Result<bool> {
    let (yaml, _) = crate::reader::markdown_document(text)?;
    let Some(yaml) = yaml else { return Ok(false) };
    let Some(metadata) = crate::reader::frontmatter(yaml)? else {
        return Ok(false);
    };
    Ok(metadata.as_object().is_some_and(|object| {
        object.len() == 1 && object.get("okf_version").and_then(Value::as_str) == Some("0.2")
    }))
}

/// Requires both the native declaration and a leading body ownership marker before rebuilding an index.
pub fn owned_index(text: &str) -> crate::Result<bool> {
    let (_, body) = crate::reader::markdown_document(text)?;
    Ok(body.lines().next() == Some(INDEX_MARKER) && native_index(text)?)
}

/// Parses only our versioned native log, rejecting corrupt owned history rather than dropping old entries.
/// Native ISO date headings are ordered newest first; group bodies retain original lines and blank paragraphs.
pub fn owned_log(
    text: &str,
) -> crate::Result<Option<std::collections::BTreeMap<String, Vec<String>>>> {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(|line| line.trim_end_matches(['\r', '\n'])) != Some(LOG_MARKER) {
        return Ok(None);
    }
    ensure!(
        lines.next().map(|line| line.trim_end_matches(['\r', '\n']))
            == Some("# Directory Update Log"),
        "Invalid managed OKF log heading"
    );
    let date_format = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]")?;
    let mut groups = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut current: Option<String> = None;
    for raw in lines {
        let line = raw.trim_end_matches(['\r', '\n']);
        if let Some(date) = line.strip_prefix("## ") {
            ensure!(
                date.len() == 10 && time::Date::parse(date, &date_format).is_ok(),
                "Invalid managed OKF log date"
            );
            ensure!(
                current
                    .as_ref()
                    .is_none_or(|previous| previous.as_str() > date),
                "Managed OKF log dates must be unique and newest first"
            );
            if let Some(previous) = &current {
                ensure!(
                    groups[previous].iter().any(|line| !line.trim().is_empty()),
                    "Empty managed OKF log date group"
                );
            }
            groups.insert(date.into(), Vec::new());
            current = Some(date.into());
        } else if current.is_some() || !line.trim().is_empty() {
            let date = current
                .as_ref()
                .context("Managed OKF log entry lacks date")?;
            groups.get_mut(date).unwrap().push(raw.into());
        }
    }
    ensure!(
        current
            .as_ref()
            .is_some_and(|date| groups[date].iter().any(|line| !line.trim().is_empty())),
        "Empty managed OKF log"
    );
    Ok(Some(groups))
}
