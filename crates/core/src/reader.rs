use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::canonical::*;
use crate::engine::{canonical_id, content_hash};
use crate::plugins::*;
use crate::reports::*;

pub fn output() -> ReaderOutput {
    ReaderOutput {
        source_records: vec![],
        records: vec![],
        anomalies: vec![],
        registered_count: 0,
        deleted_count: 0,
        source_unavailable: vec![],
    }
}

pub fn record(
    system: &str,
    version: &str,
    id: &str,
    locator: &str,
    content: &str,
    evidence: EvidenceLevel,
) -> CanonicalRecord {
    CanonicalRecord {
        canonical_id: canonical_id(system, id),
        source: SourceIdentity {
            system: system.into(),
            adapter_version: version.into(),
            export_version: "unknown".into(),
        },
        source_record_id: id.into(),
        source_locator: locator.into(),
        scope: Scope::User,
        content: content.into(),
        content_hash: content_hash(content.as_bytes()),
        dna_class: DnaClass::Standard,
        provenance: Provenance {
            actor: format!("process:{system}"),
            actor_kind: ActorKind::Import,
            method: "import".into(),
            source_ref: Some(locator.into()),
            evidence: None,
        },
        evidence_level: evidence,
        scope_qualifier: None,
        owner_declared: None,
        source_kind: None,
        source_extra: None,
        tags: None,
        entities: None,
        relations: None,
        created_at: None,
        updated_at: None,
        observed_at: None,
        valid_from: None,
        valid_to: None,
        expires_at: None,
        ttl: None,
        consent: None,
        approval: None,
        sensitive_findings: None,
        deletion_intent: None,
        tombstone: None,
        embedding: None,
        reembed_plan: None,
        conflict_cluster_id: None,
        conflict_candidates: None,
        verdict: None,
    }
}

pub fn source(record: &CanonicalRecord, locator: &str, fields: Value) -> SourceRecord {
    SourceRecord {
        canonical_id: record.canonical_id.clone(),
        source_record_id: record.source_record_id.clone(),
        source_locator: locator.into(),
        fields,
        field_map: vec![],
        unmapped: vec![],
    }
}

pub fn map(source: &mut SourceRecord, path: &str, canonical: &str) {
    source.field_map.push(FieldMapping {
        source_path: path.into(),
        canonical_path: canonical.into(),
        rule: None,
    });
}

pub fn classify(record: &mut CanonicalRecord, source: &mut SourceRecord, path: &str) {
    let kind = source
        .fields
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::to_owned);
    record.source_kind = kind.clone();
    let rule = match kind.as_deref() {
        Some("profile") => {
            record.dna_class = DnaClass::Dna;
            "profile_to_identity_dna"
        }
        Some("preference") => {
            record.dna_class = DnaClass::Dna;
            "preference_to_preference_dna"
        }
        Some("instruction") => {
            record.dna_class = DnaClass::Dna;
            "instruction_to_procedure_dna"
        }
        Some("project" | "tool") => "explicit_standard",
        _ => {
            record.evidence_level = EvidenceLevel::Inferred;
            "unknown_standard"
        }
    };
    if kind.is_some() {
        map(source, path, "/source_kind");
        source.field_map.push(FieldMapping {
            source_path: path.into(),
            canonical_path: "/dna_class".into(),
            rule: Some(rule.into()),
        });
        if rule == "unknown_standard" {
            source.unmapped.push(UnmappedField {
                source_path: path.into(),
                reason: UnmappedReason::SourceUnknown,
            });
        }
    }
}

pub fn time(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        OffsetDateTime::parse(text, &Rfc3339)
            .ok()?
            .format(&Rfc3339)
            .ok()
    } else {
        let seconds = value.as_f64()?;
        if !seconds.is_finite() {
            return None;
        }
        OffsetDateTime::from_unix_timestamp_nanos((seconds * 1e9) as i128)
            .ok()?
            .format(&Rfc3339)
            .ok()
    }
}

pub fn finish(
    output: &mut ReaderOutput,
    mut record: CanonicalRecord,
    mut source: SourceRecord,
) -> crate::Result<()> {
    fn remove(value: &mut Value, parts: &[&str]) {
        if let Some((head, tail)) = parts.split_first() {
            let key = head.replace("~1", "/").replace("~0", "~");
            if let Some(object) = value.as_object_mut() {
                if tail.is_empty() {
                    object.remove(&key);
                } else if let Some(child) = object.get_mut(&key) {
                    remove(child, tail);
                    if child.as_object().is_some_and(Map::is_empty) {
                        object.remove(&key);
                    }
                }
            }
        }
    }
    fn unknown(value: &Value, path: &str, fields: &mut Vec<UnmappedField>) {
        if let Some(object) = value.as_object() {
            for (key, value) in object {
                let path = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                if value.as_object().is_some_and(|object| !object.is_empty()) {
                    unknown(value, &path, fields);
                } else {
                    fields.push(UnmappedField {
                        source_path: path,
                        reason: UnmappedReason::SourceUnknown,
                    });
                }
            }
        }
    }
    let mut extra = source.fields.clone();
    for mapping in &source.field_map {
        remove(
            &mut extra,
            &mapping.source_path.split('/').skip(1).collect::<Vec<_>>(),
        );
    }
    unknown(&extra, "", &mut source.unmapped);
    for field in &source.unmapped {
        if extra.pointer(&field.source_path).is_some() {
            source.field_map.push(FieldMapping {
                source_path: field.source_path.clone(),
                canonical_path: format!("/source_extra{}", field.source_path),
                rule: Some("preserved_source_extra".into()),
            });
        }
    }
    if let Some(extra) = extra.as_object().filter(|extra| !extra.is_empty()) {
        let mut preserved = record.source_extra.take().unwrap_or_default();
        fn merge(target: &mut Value, incoming: &Value) -> crate::Result<()> {
            if let (Some(target), Some(incoming)) = (target.as_object_mut(), incoming.as_object()) {
                for (key, value) in incoming {
                    if let Some(old) = target.get_mut(key) {
                        merge(old, value)?;
                    } else {
                        target.insert(key.clone(), value.clone());
                    }
                }
            } else {
                anyhow::ensure!(
                    target == incoming,
                    "Conflicting original and envelope source metadata"
                );
            }
            Ok(())
        }
        for (key, value) in extra {
            if let Some(old) = preserved.get_mut(key) {
                merge(old, value)?;
            } else {
                preserved.insert(key.clone(), value.clone());
            }
        }
        record.source_extra = Some(preserved);
    }
    output.records.push(record);
    output.source_records.push(source);
    Ok(())
}

pub fn anomaly(
    output: &mut ReaderOutput,
    locator: &str,
    code: &str,
    path: &str,
    line: Option<u64>,
) {
    output.anomalies.push(Anomaly {
        source_locator: locator.into(),
        code: code.into(),
        field_path: Some(path.into()),
        line,
    });
}

pub fn markdown_document(text: &str) -> crate::Result<(Option<&str>, &str)> {
    let opening = if text.starts_with("---\r\n") {
        5
    } else if text.starts_with("---\n") {
        4
    } else {
        return Ok((None, text));
    };
    let mut offset = opening;
    for line in text[opening..].split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Ok((Some(&text[opening..offset]), &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    anyhow::bail!("Unclosed Markdown frontmatter")
}
