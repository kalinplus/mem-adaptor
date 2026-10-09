//! Regressions for OKF native envelopes and approved filesystem boundaries using synthetic sources.
//! Real Engine runs issue WriteTokens; narrow Writer wrappers inject faults at explicit call boundaries.
//! Native expectations are checked directly from YAML and bytes, not from the production OKF projection.
//! Snapshots cover complete isolated regular-file trees; these tests do not establish transaction or lock guarantees.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use mem_adaptor_core::Result;
use mem_adaptor_core::canonical::{
    ActorKind, CandidateOrigin, CanonicalRecord, EvidenceLevel, Verdict,
};
use mem_adaptor_core::engine::{
    Engine, WriteToken, content_hash, record_hash, timestamp, write_json_new,
};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::okf;
use mem_adaptor_core::plugins::*;
use mem_adaptor_core::reader as source;
use mem_adaptor_core::reports::*;
use mem_adaptor_core::writer as target;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use serde_json::{Value, json};
use tempfile::TempDir;

mod common;

struct SyntheticReader(Vec<CanonicalRecord>);

impl Reader for SyntheticReader {
    /// Identifies this synthetic source consistently with its records.
    fn id(&self) -> &'static str {
        "okf-repair-test"
    }
    /// Keeps the source adapter identity deterministic.
    fn version(&self) -> &'static str {
        "test"
    }
    /// Fixture sources are directories held in place for the run, so their path may bind a satellite.
    fn source_kind(&self) -> SourceKind {
        SourceKind::Directory
    }
    /// Claims the single fixture file without involving source-format heuristics.
    fn claim(&self, inventory: &FileInventory) -> Vec<Claim> {
        inventory
            .keys()
            .map(|path| Claim {
                path: path.clone(),
                layer: "memory".into(),
                registered_only: false,
            })
            .collect()
    }
    /// Supplies valid source mappings and records to the real validation and approval pipeline.
    fn read(&self, claim: &Claim, _: &SourceFs) -> Result<ReaderOutput> {
        let mut output = source::output();
        for record in &self.0 {
            let mut original = source::source(record, &claim.path, json!({"body": record.content}));
            source::map(&mut original, "/body", "/content");
            output.source_records.push(original);
            output.records.push(record.clone());
        }
        Ok(output)
    }
}

/// Creates a stable source record without inventing native target author or time fields.
fn record(id: &str, body: &str) -> CanonicalRecord {
    source::record(
        "okf-repair-test",
        "test",
        None,
        id,
        "synthetic.json",
        body,
        EvidenceLevel::Measured,
    )
}

/// Creates an isolated source and leaves its target absent.
fn fixture() -> TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    fs::write(directory.path().join("source/synthetic.json"), "{}").unwrap();
    directory
}

/// Registers the selected Writer seam while retaining the real engine.
fn engine(records: Vec<CanonicalRecord>, writer: impl Writer + 'static) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(SyntheticReader(records)).unwrap();
    registry.register_writer("home".into(), writer).unwrap();
    Engine { registry }
}

/// Builds the real OKF Writer at a normalized fixture target.
fn okf_engine(directory: &TempDir, records: Vec<CanonicalRecord>) -> Engine {
    engine(
        records,
        OkfWriter::new(directory.path().join("target")).unwrap(),
    )
}

/// Makes policy explicit while using no real private data or remote model.
fn policy() -> GatePolicy {
    GatePolicy {
        secrets: GateAction::Pass,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}

/// Plans from the real source loader, optionally with actual receipt history.
fn plan(engine: &Engine, directory: &TempDir, previous: Option<&Path>) -> PlanReport {
    engine
        .plan_with_previous(&directory.path().join("source"), policy(), previous)
        .unwrap()
}

/// Obtains the real WriteToken through approved application, without persisting a fake success receipt.
fn apply(engine: &Engine, report: &PlanReport) -> Result<ReceiptReport> {
    engine.apply(
        report,
        &ApprovalReceipt {
            schema_version: "0.1.0".into(),
            receipt_id: "synthetic".into(),
            plan_digest: report.plan_digest.clone(),
            approved_at: timestamp().unwrap(),
            backend: "local".into(),
            approver: "synthetic".into(),
        },
        "synthetic-plan.json".into(),
        "synthetic-approval.json".into(),
    )
}

/// Captures every regular file and directory without following links or opening special files.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    /// Visits only ordinary fixture entries and preserves their exact relative names and bytes.
    fn visit(root: &Path, path: &Path, output: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            if metadata.is_dir() {
                output.insert(format!("{relative}/"), vec![]);
                visit(root, &path, output);
            } else {
                assert!(
                    metadata.is_file(),
                    "Snapshot must not follow links or open special files"
                );
                output.insert(relative, fs::read(path).unwrap());
            }
        }
    }
    let mut output = BTreeMap::new();
    if root.exists() {
        visit(root, root, &mut output);
    }
    output
}

/// Splits only standalone delimiter lines, retaining exact body bytes independently of the production parser.
fn native(text: &str) -> (Value, &str) {
    let first = text.find('\n').unwrap() + 1;
    assert_eq!(text[..first].trim_end_matches(['\r', '\n']), "---");
    let mut offset = first;
    for line in text[first..].split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return (
                serde_saphyr::from_str(&text[first..offset]).unwrap(),
                &text[offset + line.len()..],
            );
        }
        offset += line.len();
    }
    panic!("Native envelope has no standalone closing delimiter")
}

/// Reads a real managed native record without using its implementation's projection.
fn note(directory: &TempDir, id: &str) -> String {
    fs::read_to_string(directory.path().join(format!("target/memories/{id}.md"))).unwrap()
}

/// Saves genuine receipt history under a unique synthetic name.
fn history(directory: &TempDir, receipt: &ReceiptReport, name: &str) -> PathBuf {
    let path = directory.path().join(name);
    write_json_new(&path, receipt).unwrap();
    path
}

/// Requires both real read-back verification and the intended execution-evidence references.
fn verified(receipt: &ReceiptReport) {
    assert!(
        receipt
            .entries
            .iter()
            .filter(|entry| entry.target_id.is_some())
            .all(|entry| { entry.verification == Some(Verification::Verified) })
    );
    assert_eq!(receipt.plan_ref, "synthetic-plan.json");
    assert_eq!(receipt.approval_receipt_ref, "synthetic-approval.json");
}

/// Group 5 (DEC-21 A): removing a whole envelope de-manages exactly that file. Its entry asks for a
/// human decision (`target_unmanaged`), the remaining entries and the plan continue, the degraded note
/// reads back as an ordinary note with a new path identity, and the file's bytes survive untouched.
#[test]
fn de_managed_target_files_report_target_unmanaged_and_the_plan_continues() {
    let directory = fixture();
    let keep = record("keep", "Managed body that stays managed.\n");
    let gone = record("gone", "Body whose envelope disappears.\n");
    let initial = okf_engine(&directory, vec![keep.clone(), gone.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let gone_id = gone.canonical_id.clone();
    let gone_path = directory
        .path()
        .join(format!("target/memories/{gone_id}.md"));
    let plain = "Plain user note replacing the managed file.\n";
    fs::write(&gone_path, plain).unwrap();
    let fresh = record("fresh", "A brand-new third memory.\n");
    let engine = okf_engine(&directory, vec![keep, gone, fresh]);
    let next = plan(&engine, &directory, Some(&previous));
    fn by_source<'a>(report: &'a PlanReport, id: &str) -> &'a PlanEntry {
        report
            .entries
            .iter()
            .find(|entry| entry.source_record_id == id)
            .unwrap()
    }
    assert_eq!(
        by_source(&next, "gone").disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetUnmanaged,
        }
    );
    assert_eq!(
        by_source(&next, "keep").disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated,
        }
    );
    assert_eq!(by_source(&next, "fresh").disposition, Disposition::Accepted);
    apply(&engine, &next).unwrap();
    assert_eq!(fs::read_to_string(&gone_path).unwrap(), plain);
    // The degraded file is an ordinary note again: reading the home as a source gives it a fresh
    // path-derived identity instead of the managed canonical id.
    let inventory: FileInventory = fs::read_dir(directory.path().join("target/memories"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                format!("memories/{}", entry.file_name().to_str().unwrap()),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect();
    let source = SourceFs {
        root: "/synthetic".into(),
        files: inventory,
        satellite_id: None,
    };
    let degraded_claim = MarkdownReader
        .claim(&source.files)
        .into_iter()
        .find(|claim| claim.path == format!("memories/{gone_id}.md"))
        .unwrap();
    let degraded = MarkdownReader.read(&degraded_claim, &source).unwrap();
    assert_eq!(degraded.records.len(), 1);
    assert_ne!(degraded.records[0].canonical_id, gone_id);
    assert!(
        degraded.records[0]
            .source_record_id
            .starts_with("memories/")
    );
}

/// Group 5 (DEC-21 A/D): a parseable but invalid envelope is reported per file (anomaly plus an
/// unresolved entry) while the plan keeps running; the write then refuses before any target change
/// because the index rebuild cannot represent the damaged managed record (testing-policy D).
#[test]
fn invalid_envelopes_report_per_file_and_the_plan_continues() {
    let directory = fixture();
    let keep = record("keep", "Managed body that stays managed.\n");
    let broken = record("broken", "Body whose envelope stops validating.\n");
    let initial = okf_engine(&directory, vec![keep.clone(), broken.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let broken_id = broken.canonical_id.clone();
    let text = note(&directory, &broken_id);
    let (mut metadata, body) = native(&text);
    metadata["mem_adaptor"]["source_record_id"] = json!("tampered-identity");
    fs::write(
        directory
            .path()
            .join(format!("target/memories/{broken_id}.md")),
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let before = snapshot(&directory.path().join("target"));
    let fresh = record("fresh", "A brand-new third memory.\n");
    let engine = okf_engine(&directory, vec![keep, broken, fresh]);
    let next = plan(&engine, &directory, Some(&previous));
    fn by_source<'a>(report: &'a PlanReport, id: &str) -> &'a PlanEntry {
        report
            .entries
            .iter()
            .find(|entry| entry.source_record_id == id)
            .unwrap()
    }
    assert_eq!(
        by_source(&next, "broken").disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified,
        }
    );
    assert_eq!(
        next.anomalies
            .iter()
            .filter(|anomaly| anomaly.code == "managed_envelope_invalid"
                && anomaly.source_locator == format!("memories/{broken_id}.md"))
            .count(),
        1,
        "{:?}",
        next.anomalies
    );
    assert_eq!(by_source(&next, "fresh").disposition, Disposition::Accepted);
    let error = apply(&engine, &next).unwrap_err();
    assert!(
        format!("{error:#}").contains("Managed OKF envelope is invalid"),
        "{error:#}"
    );
    assert_eq!(snapshot(&directory.path().join("target")), before);
}

/// Group 3 (DEC-21 A): advisory values the user changed relative to the old envelope survive an
/// approved update; the planned baseline is crafted to match the edited file, which is how a
/// resolved home-value verdict will anchor the next round (#34b).
#[test]
fn sticky_advisory_values_survive_approved_updates() {
    let directory = fixture();
    let sticky = record("sticky", "First body.\n");
    let initial = okf_engine(&directory, vec![sticky.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", sticky.canonical_id));
    let text = fs::read_to_string(&path).unwrap();
    let edited = text.replacen("title: First body", "title: Kept user title", 1);
    assert_ne!(edited, text);
    fs::write(&path, &edited).unwrap();
    let (metadata, body) = native(&edited);
    let edited_record = okf::restore(&metadata, body).unwrap();
    let mut anchored = serde_json::to_value(&receipt).unwrap();
    anchored["entries"][0]["prior_write"] = json!({
        "target_id": format!("memories/{}", sticky.canonical_id),
        "content_hash": edited_record.content_hash,
        "record_hash": record_hash(&edited_record).unwrap(),
        "target_hash": content_hash(edited.as_bytes()),
        "verification": {"status": "verified"}
    });
    let previous = directory.path().join("anchored.json");
    write_json_new(&previous, &anchored).unwrap();
    let updated = record("sticky", "Second body with a satellite update.\n");
    let engine = okf_engine(&directory, vec![updated]);
    let next = plan(&engine, &directory, Some(&previous));
    assert_eq!(next.entries[0].disposition, Disposition::Accepted);
    apply(&engine, &next).unwrap();
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("title: Kept user title"), "{after}");
    assert!(
        after.contains("Second body with a satellite update."),
        "{after}"
    );
    let (final_metadata, final_body) = native(&after);
    let final_record = okf::restore(&final_metadata, final_body).unwrap();
    assert_eq!(final_record.canonical_id, sticky.canonical_id);
}

/// Exercises legal embedded delimiters, CRLF native scanning, and historical updates through all real boundaries.
#[test]
fn standalone_delimiters_preserve_typed_metadata_and_crlf_updates() {
    let directory = fixture();
    let mut original = record("delimiter", "Heading---\nBody remains exact.\r\n");
    original.source_extra = Some(
        json!({"frontmatter": {
            "unknown": {"block": "before\n---\nafter\n", "typed": [true, 3, null]}
        }})
        .as_object()
        .unwrap()
        .clone(),
    );
    let first_engine = engine(
        vec![original.clone()],
        BoundaryWriter {
            inner: OkfWriter::new(directory.path().join("target")).unwrap(),
            injection: Injection::Crlf,
        },
    );
    let first = apply(&first_engine, &plan(&first_engine, &directory, None)).unwrap();
    verified(&first);
    let text = note(&directory, &original.canonical_id);
    let (metadata, body) = native(&text);
    assert_eq!(metadata["title"], "Heading---");
    assert_eq!(body, original.content);
    assert_eq!(
        metadata["mem_adaptor"]["source_extra"],
        json!(original.source_extra)
    );
    assert!(
        text.contains("  ---\r\n"),
        "Fixture must actually contain an indented delimiter"
    );
    assert!(text.starts_with("---\r\n"));
    let writer = first_engine.registry.writer("home").unwrap();
    assert_eq!(
        writer
            .inspect(&format!("memories/{}", original.canonical_id))
            .unwrap(),
        Some(original.clone())
    );
    let source_fs = SourceFs {
        root: directory.path().join("target"),
        files: [(
            format!("memories/{}.md", original.canonical_id),
            text.into_bytes(),
        )]
        .into_iter()
        .collect(),
        satellite_id: None,
    };
    let read = MarkdownReader
        .read(&MarkdownReader.claim(&source_fs.files)[0], &source_fs)
        .unwrap();
    assert_eq!(read.records.len(), 1);
    assert_eq!(read.records[0].content, original.content);
    assert_eq!(read.records[0].canonical_id, original.canonical_id);
    assert_eq!(read.records[0].provenance, original.provenance);
    assert_eq!(
        read.records[0].source_extra.as_ref().unwrap()["frontmatter"]["unknown"],
        original.source_extra.as_ref().unwrap()["frontmatter"]["unknown"]
    );
    let previous = history(&directory, &first, "crlf-history.json");
    let mut updated = original;
    updated.content.push_str("Updated exact body.\n");
    updated.content_hash = content_hash(updated.content.as_bytes());
    let second_engine = okf_engine(&directory, vec![updated.clone()]);
    let second = apply(
        &second_engine,
        &plan(&second_engine, &directory, Some(&previous)),
    )
    .unwrap();
    verified(&second);
    let text = note(&directory, &updated.canonical_id);
    let (metadata, body) = native(&text);
    assert_eq!(body, updated.content);
    assert_eq!(
        metadata["mem_adaptor"]["source_extra"],
        json!(updated.source_extra)
    );
}

/// Checks actor/time projection and its report declarations against explicit native expectations.
#[test]
fn native_actor_time_and_mapping_conditions_do_not_invent_provenance() {
    for (kind, actor, updated, expected_actor) in [
        (ActorKind::User, "Alice", true, Some("human:Alice")),
        (ActorKind::User, "human:Alice", true, Some("human:Alice")),
        (ActorKind::User, "user", true, Some("human:user")),
        (ActorKind::User, "Alice", false, Some("human:Alice")),
        (
            ActorKind::Agent,
            "synthetic-agent",
            true,
            Some("synthetic-agent"),
        ),
        (
            ActorKind::Model,
            "synthetic-model",
            true,
            Some("synthetic-model"),
        ),
        (ActorKind::Scan, "scan", true, None),
        (ActorKind::Import, "import", true, None),
    ] {
        let directory = fixture();
        let mut original = record("actor", "Native author test.\n");
        original.provenance.actor_kind = kind;
        original.provenance.actor = actor.into();
        original.tags = Some(vec!["synthetic-tag".into()]);
        original.updated_at = updated.then(|| "2026-01-02T03:04:05Z".into());
        let engine = okf_engine(&directory, vec![original.clone()]);
        let report = plan(&engine, &directory, None);
        let receipt = apply(&engine, &report).unwrap();
        verified(&receipt);
        let text = note(&directory, &original.canonical_id);
        let (metadata, body) = native(&text);
        assert_eq!(body, original.content);
        assert_eq!(metadata["sources"][0]["id"], original.source_record_id);
        assert_eq!(metadata["sources"][0]["resource"], original.source_locator);
        assert_eq!(metadata["tags"], json!(["synthetic-tag"]));
        assert_eq!(
            metadata["sources"][0].get("author").and_then(Value::as_str),
            expected_actor
        );
        assert_eq!(
            metadata["sources"][0].get("last_modified"),
            original
                .updated_at
                .as_ref()
                .map(|time| json!(time))
                .as_ref()
        );
        if let (true, Some(author)) = (updated, expected_actor) {
            assert_eq!(
                metadata["generated"],
                json!({"by": author, "at": original.updated_at})
            );
        } else {
            assert!(metadata.get("generated").is_none());
        }
        assert!(!text.contains("human:human:"));
        assert_eq!(
            engine
                .registry
                .writer("home")
                .unwrap()
                .inspect(&format!("memories/{}", original.canonical_id))
                .unwrap(),
            Some(original.clone())
        );
        let paths: Vec<_> = report.entries[0]
            .target_map
            .iter()
            .map(|mapping| mapping.target_path.as_str())
            .collect();
        assert_eq!(
            paths.contains(&"/frontmatter/generated/at"),
            updated && expected_actor.is_some()
        );
        assert_eq!(
            paths.contains(&"/frontmatter/generated/by"),
            updated && expected_actor.is_some()
        );
        assert_eq!(
            paths.contains(&"/frontmatter/sources/0/author"),
            expected_actor.is_some()
        );
        assert_eq!(
            paths.contains(&"/frontmatter/sources/0/last_modified"),
            updated
        );
        for path in [
            "/frontmatter/sources/0/id",
            "/frontmatter/sources/0/resource",
            "/frontmatter/tags",
        ] {
            assert!(
                paths.contains(&path),
                "Native field must be declared: {path}"
            );
        }
        assert!(
            !serde_json::to_string(&report.entries[0].target_map)
                .unwrap()
                .contains("synthetic-tag")
        );
    }
}

/// Checks native version-only index metadata and newest-first grouped logs across actual updates.
#[test]
fn native_index_and_log_follow_format_and_two_round_counts() {
    let directory = fixture();
    fs::create_dir(directory.path().join("target")).unwrap();
    let old_group = "\n## 2000-01-02\n\n- [00:00:00Z] mem-adaptor: +0 ~0\n\nFirst paragraph.\r\n\r\nSecond paragraph.\n\n```\nlet value = 1;\n\nvalue\n```\n";
    fs::write(
        directory.path().join("target/log.md"),
        format!("<!-- mem-adaptor:okf-log:v1 -->\n# Directory Update Log\n{old_group}"),
    )
    .unwrap();
    let original = record("log", "First native title.\n");
    let first_engine = okf_engine(&directory, vec![original.clone()]);
    let first = apply(&first_engine, &plan(&first_engine, &directory, None)).unwrap();
    let index = fs::read_to_string(directory.path().join("target/index.md")).unwrap();
    let (metadata, body) = native(&index);
    assert_eq!(metadata, json!({"okf_version": "0.2"}));
    assert!(
        body.starts_with("<!-- mem-adaptor:okf-index:v1 -->\n"),
        "Ownership belongs in the body"
    );
    assert!(body.contains(&format!("memories/{}.md", original.canonical_id)));
    let log = fs::read_to_string(directory.path().join("target/log.md")).unwrap();
    assert!(log.contains("mem-adaptor: +1 ~0"));
    let date = timestamp().unwrap()[..10].to_owned();
    assert!(log.contains(&format!("## {date}")));
    let previous = history(&directory, &first, "first.json");
    let mut changed = original;
    changed.content = "Updated native title.\n".into();
    changed.content_hash = content_hash(changed.content.as_bytes());
    let second_engine = okf_engine(&directory, vec![changed]);
    let second = apply(
        &second_engine,
        &plan(&second_engine, &directory, Some(&previous)),
    )
    .unwrap();
    verified(&second);
    let log = fs::read_to_string(directory.path().join("target/log.md")).unwrap();
    assert_eq!(log.matches(&format!("## {date}")).count(), 1);
    assert!(log.find("mem-adaptor: +0 ~1").unwrap() < log.find("mem-adaptor: +1 ~0").unwrap());
    assert!(log.find(&format!("## {date}")).unwrap() < log.find("## 2000-01-02").unwrap());
    assert!(
        log.ends_with(old_group),
        "Existing older log group must remain byte-exact"
    );
    let source_fs = SourceFs {
        root: directory.path().join("target"),
        files: [
            ("index.md".into(), index.into_bytes()),
            ("log.md".into(), log.into_bytes()),
        ]
        .into_iter()
        .collect(),
        satellite_id: None,
    };
    let claims = MarkdownReader.claim(&source_fs.files);
    assert_eq!(claims.len(), 2);
    assert!(claims.iter().all(|claim| claim.registered_only));
}

/// Preserves same-day prose and CRLF blank lines, including a final line without a newline.
#[test]
fn same_day_log_prose_keeps_exact_blank_lines_and_final_line() {
    for prose in [
        "\r\nFirst paragraph.\r\n\r\nSecond paragraph.\n\nFinal line.",
        "First paragraph.\r\n\r\nSecond paragraph.\n\nFinal line.",
    ] {
        let directory = fixture();
        fs::create_dir(directory.path().join("target")).unwrap();
        let date = timestamp().unwrap()[..10].to_owned();
        fs::write(
            directory.path().join("target/log.md"),
            format!(
                "<!-- mem-adaptor:okf-log:v1 -->\n# Directory Update Log\n\n## {date}\n{prose}"
            ),
        )
        .unwrap();
        let engine = okf_engine(
            &directory,
            vec![record("log-prose", "New native memory.\n")],
        );
        let receipt = apply(&engine, &plan(&engine, &directory, None)).unwrap();
        verified(&receipt);
        let log = fs::read_to_string(directory.path().join("target/log.md")).unwrap();
        let after_event = log.split_once("mem-adaptor: +1 ~0").unwrap().1;
        assert!(
            after_event.starts_with("\n\n") || after_event.starts_with("\n\r\n"),
            "{log}"
        );
        assert!(
            log.ends_with(prose),
            "Old body must stay contiguous and exact: {log}"
        );
        assert_eq!(log.matches(&format!("## {date}")).count(), 1);
    }
}

/// Refuses visibly managed block/flow YAML damage instead of classifying it as an ordinary user record.
#[test]
fn damaged_mapping_declarations_refuse_before_unrelated_writes() {
    for (yaml, closed) in [
        (
            "{mem_adaptor_envelope: \"okf:0.2\", mem_adaptor: [broken",
            true,
        ),
        (
            "{\"mem_adaptor_envelope\": \"okf:0.2\", \"mem_adaptor\": [broken",
            true,
        ),
        (
            "  mem_adaptor_envelope: okf:0.2\n  mem_adaptor: [broken",
            true,
        ),
        (
            "{mem_adaptor_envelope: \"okf:0.2\", mem_adaptor: [broken",
            false,
        ),
    ] {
        let directory = fixture();
        let original = record("old", "Old managed memory.\n");
        let initial = okf_engine(&directory, vec![original.clone()]);
        apply(&initial, &plan(&initial, &directory, None)).unwrap();
        let damaged = format!("---\n{yaml}\n{}Body.\n", if closed { "---\n" } else { "" });
        fs::write(
            directory
                .path()
                .join(format!("target/memories/{}.md", original.canonical_id)),
            &damaged,
        )
        .unwrap();
        let before = snapshot(directory.path());
        let engine = okf_engine(
            &directory,
            vec![original, record("new", "Unrelated new memory.\n")],
        );
        let error = engine
            .plan(&directory.path().join("source"), policy())
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains(if closed {
                "Invalid managed OKF frontmatter"
            } else {
                "Unclosed managed OKF frontmatter"
            }),
            "{message}"
        );
        assert!(!message.contains("broken"));
        assert_eq!(snapshot(directory.path()), before);
        for name in ["plan.json", "plan.approval.json", "plan.receipt.json"] {
            assert!(!directory.path().join(name).exists());
        }
    }
    assert!(
        mem_adaptor_core::okf::managed_metadata(
            "---\n{custom: {mem_adaptor: ordinary-user-value}}\n---\nUser body.\n",
        )
        .unwrap()
        .is_none()
    );
}

/// Native source/tag/time edits no longer abort Writer-side planning; the edited record alone refuses.
#[test]
fn native_projection_edits_do_not_abort_unrelated_plans() {
    for field in ["sources", "generated", "tags"] {
        let directory = fixture();
        let original = record("old", "Existing managed record.\n");
        let initial = okf_engine(&directory, vec![original.clone()]);
        apply(&initial, &plan(&initial, &directory, None)).unwrap();
        let text = note(&directory, &original.canonical_id);
        let (mut metadata, body) = native(&text);
        metadata[field] = match field {
            "sources" => json!([{"id":"different-record","resource":"different-file"}]),
            "generated" => json!({"by":"human:different-author","at":"2026-01-01T00:00:00Z"}),
            _ => json!(["different-tag"]),
        };
        fs::write(
            directory
                .path()
                .join(format!("target/memories/{}.md", original.canonical_id)),
            format!(
                "---\n{}---\n{body}",
                serde_saphyr::to_string(&metadata).unwrap()
            ),
        )
        .unwrap();
        // DEC-21 A: a parseable native-field edit is classified, not fatal, so an unrelated plan keeps
        // running; the edited record itself is refused by its own round (record-level modified), and
        // the edited file survives untouched because this round never plans it.
        let engine = okf_engine(&directory, vec![record("new", "Unrelated new memory.\n")]);
        let next = engine
            .plan(&directory.path().join("source"), policy())
            .unwrap();
        assert_eq!(next.entries.len(), 1);
        assert_eq!(next.entries[0].disposition, Disposition::Accepted);
        let edited_path = directory
            .path()
            .join(format!("target/memories/{}.md", original.canonical_id));
        let edited_before = fs::read(&edited_path).unwrap();
        apply(&engine, &next).unwrap();
        assert_eq!(fs::read(&edited_path).unwrap(), edited_before);
    }
}

/// Group 2 (DEC-21 B rule three): a home-only tags edit keeps the home value, is omitted as
/// home_modified with the edited fields listed, advances nothing, and never touches the bytes.
#[test]
fn home_only_edits_are_omitted_home_modified_and_keep_the_basis() {
    let directory = fixture();
    let original = record("kept", "Existing body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", original.canonical_id));
    let text = fs::read_to_string(&path).unwrap();
    let (mut metadata, body) = native(&text);
    metadata["tags"] = json!(["home-tag"]);
    fs::write(
        &path,
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let engine = okf_engine(&directory, vec![original.clone()]);
    let next = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::HomeModified {
                home_changed_fields: vec!["/frontmatter/tags".into()]
            }
        }
    );
    assert_eq!(
        next.entries[0].prior_write, receipt.entries[0].prior_write,
        "an omission alone must not advance the recorded basis"
    );
    let before = snapshot(&directory.path().join("target"));
    apply(&engine, &next).unwrap();
    assert_eq!(snapshot(&directory.path().join("target")), before);

    // A second home-only round that also rewrites the body reports both pointers in the fixed order
    // (body, then tags), so the report bytes stay reproducible.
    let text = fs::read_to_string(&path).unwrap();
    let (mut metadata, _) = native(&text);
    metadata["title"] = json!("Home rewritten.");
    fs::write(
        &path,
        format!(
            "---\n{}---\nHome rewritten.\n",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let engine = okf_engine(&directory, vec![original.clone()]);
    let both = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        both.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::HomeModified {
                home_changed_fields: vec!["/body".into(), "/frontmatter/tags".into()]
            }
        }
    );
}

/// Group 6 rule four (DEC-21 B): both sides changed differently, so the entry is a conflict in a
/// reported cluster; the cluster id follows the pinned formula and stays deterministic, and an
/// unadjudicated cluster writes nothing.
#[test]
fn both_sides_changed_conflicts_cluster_and_reuses_verdicts() {
    let directory = fixture();
    let original = record("clash", "Original body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", original.canonical_id));
    let text = fs::read_to_string(&path).unwrap();
    let (mut metadata, _) = native(&text);
    metadata["title"] = json!("Edited at home.");
    fs::write(
        &path,
        format!(
            "---\n{}---\nEdited at home.\n",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let mut updated = record("clash", "Updated by the satellite.\n");
    updated.content_hash = content_hash(updated.content.as_bytes());
    let engine = okf_engine(&directory, vec![updated.clone()]);
    let next = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::Conflict {
                cluster_id: next.conflict_clusters[0].cluster_id.clone()
            }
        }
    );
    // The pinned formula (DEC-21 B): prefix, canonical id, both record hashes, home bytes hash.
    let prior = receipt.entries[0].prior_write.as_ref().unwrap();
    let home_bytes = fs::read(&path).unwrap();
    let expected_cluster = content_hash(
        &[
            b"mem-adaptor:conflict:v1".as_slice(),
            original.canonical_id.as_bytes(),
            prior.record_hash.as_bytes(),
            record_hash(&updated).unwrap().as_bytes(),
            content_hash(&home_bytes).as_bytes(),
        ]
        .concat(),
    );
    assert_eq!(next.conflict_clusters.len(), 1);
    assert_eq!(next.conflict_clusters[0].cluster_id, expected_cluster);
    assert_eq!(
        next.digest_inputs.conflict_clusters, next.conflict_clusters,
        "the digest binds the observed cluster evidence"
    );
    let candidates = &next.conflict_clusters[0].candidates;
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].basis, "satellite:direct");
    assert_eq!(candidates[0].content_hash, updated.content_hash);
    assert_eq!(candidates[0].record_hash, record_hash(&updated).unwrap());
    assert_eq!(
        candidates[1].basis,
        format!("home:memories/{}", original.canonical_id)
    );
    assert_eq!(
        candidates[1].origin,
        CandidateOrigin::Home {
            path: format!("memories/{}.md", original.canonical_id)
        }
    );
    // The home candidate describes the observed home state (body and tags already adopted), not the
    // satellite record and not the pre-adoption envelope.
    let home_text = fs::read_to_string(&path).unwrap();
    let (home_metadata, home_body) = native(&home_text);
    let mut observed_home = okf::restore(&home_metadata, home_body).unwrap();
    okf::apply_home_edits(&mut observed_home, &home_metadata, home_body);
    assert_eq!(candidates[1].content_hash, observed_home.content_hash);
    assert_eq!(
        candidates[1].record_hash,
        record_hash(&observed_home).unwrap()
    );
    // Re-planning the same state reproduces the same cluster id (deterministic ids, DEC-6).
    assert_eq!(
        plan(&engine, &directory, Some(&previous)).conflict_clusters[0].cluster_id,
        expected_cluster
    );
    let before = snapshot(&directory.path().join("target"));
    apply(&engine, &next).unwrap();
    assert_eq!(snapshot(&directory.path().join("target")), before);

    // A verdict naming both sides cannot choose between two values of one record, so the cluster
    // stays unresolved instead of silently writing the satellite over the home value (DEC-6 keeps
    // "all are right" for member sets, which has no meaning for the two sides of one record).
    let mut both_sides = receipt.clone();
    both_sides.verdicts = vec![Verdict::Keep {
        cluster_id: expected_cluster.clone(),
        canonical_ids: vec![original.canonical_id.clone()],
        bases: Some(vec![
            "satellite:direct".into(),
            format!("home:memories/{}", original.canonical_id),
        ]),
    }];
    let both_path = history(&directory, &both_sides, "verdict-both.json");
    let undecided = plan(&engine, &directory, Some(&both_path));
    assert!(matches!(
        undecided.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::Conflict { .. }
        }
    ));
    assert_eq!(undecided.conflict_clusters.len(), 1);

    // A human verdict that keeps the home value records the home state as the new verified basis
    // without touching a byte (DEC-21 B take-home).
    let cluster_id = expected_cluster;
    let home_basis = format!("home:memories/{}", original.canonical_id);
    let mut adjudicated = receipt.clone();
    adjudicated.verdicts = vec![Verdict::Keep {
        cluster_id: cluster_id.clone(),
        canonical_ids: vec![original.canonical_id.clone()],
        bases: Some(vec![home_basis]),
    }];
    let verdict_path = history(&directory, &adjudicated, "verdict-home.json");
    let resolved = plan(&engine, &directory, Some(&verdict_path));
    assert_eq!(
        resolved.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::HomeModified {
                home_changed_fields: vec!["/body".into()]
            }
        }
    );
    let advanced = resolved.entries[0].prior_write.as_ref().unwrap();
    assert_eq!(advanced.target_hash, content_hash(&home_bytes));
    // Take-home settles both sides: the home bytes are the observed target state and the current
    // satellite record becomes the satellite-side reference, so nothing is left pending.
    assert_eq!(advanced.content_hash, updated.content_hash);
    assert_eq!(advanced.record_hash, record_hash(&updated).unwrap());
    assert_eq!(advanced.verification, Verification::Verified);
    assert_eq!(snapshot(&directory.path().join("target")), before);
    let settled = apply(&engine, &resolved).unwrap();
    assert_eq!(snapshot(&directory.path().join("target")), before);
    assert_eq!(
        settled.entries[0].prior_write.as_ref().unwrap().target_hash,
        content_hash(&home_bytes),
        "the receipt carries the advanced home basis"
    );
    // The settled basis describes both sides as observed, so the adjudicated state stops being a
    // conflict: later rounds keep reporting the home divergence as a quiet omission, never a routine
    // update that would overwrite the value the user chose to keep, and the settled basis holds.
    let settled_path = history(&directory, &settled, "settled.json");
    let calm = plan(&engine, &directory, Some(&settled_path));
    assert_eq!(
        calm.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::HomeModified {
                home_changed_fields: vec!["/body".into()]
            }
        }
    );
    assert!(calm.conflict_clusters.is_empty());
    assert_eq!(
        calm.entries[0].prior_write, settled.entries[0].prior_write,
        "a settled basis does not move on its own"
    );

    // A verdict that keeps the satellite value turns the conflict into a normal approved update
    // that overwrites the edited home file (DEC-21 B take-satellite).
    let mut take_satellite = receipt.clone();
    take_satellite.verdicts = vec![Verdict::Keep {
        cluster_id,
        canonical_ids: vec![original.canonical_id.clone()],
        bases: Some(vec!["satellite:direct".into()]),
    }];
    let verdict_path = history(&directory, &take_satellite, "verdict-satellite.json");
    let overwrite = plan(&engine, &directory, Some(&verdict_path));
    assert_eq!(overwrite.entries[0].disposition, Disposition::Accepted);
    apply(&engine, &overwrite).unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("Updated by the satellite.")
    );
}

/// Group 6 rule four, field variant (DEC-21 B/iron rule 7): changes on different fields still
/// conflict; field-level merging is never invented.
#[test]
fn both_sides_changed_on_different_fields_still_conflict() {
    let directory = fixture();
    let original = record("split", "Shared body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", original.canonical_id));
    let text = fs::read_to_string(&path).unwrap();
    let (mut metadata, body) = native(&text);
    metadata["tags"] = json!(["home-tag"]);
    fs::write(
        &path,
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let mut updated = record("split", "Shared body.\nSatellite appended a fact.\n");
    updated.content_hash = content_hash(updated.content.as_bytes());
    let engine = okf_engine(&directory, vec![updated]);
    let next = plan(&engine, &directory, Some(&previous));
    assert!(matches!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::Conflict { .. }
        }
    ));
    assert_eq!(next.conflict_clusters.len(), 1);
}

/// Group 6 rule five (DEC-21 B): when the home file already equals this round's projection byte
/// for byte, the entry converges to already_migrated without a write or a basis advance.
#[test]
fn a_home_that_matches_the_projection_converges_to_already_migrated() {
    let directory = fixture();
    let original = record("conv", "First body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let mut updated = record("conv", "Second body from the satellite.\n");
    updated.content_hash = content_hash(updated.content.as_bytes());
    let engine = okf_engine(&directory, vec![updated.clone()]);
    // Render what a write would produce, then make the home exactly those bytes by hand.
    let writer = engine.registry.writer("home").unwrap();
    let mut planned = writer.plan(&updated, Some(&receipt.entries[0])).unwrap();
    planned.previous_write = receipt.entries[0].prior_write.clone();
    let bytes = writer.project(&planned).unwrap().unwrap();
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", original.canonical_id));
    fs::write(&path, &bytes).unwrap();
    let next = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::AlreadyMigrated
        }
    );
    assert_eq!(
        next.entries[0].prior_write, receipt.entries[0].prior_write,
        "convergence is an omission and does not advance the basis"
    );
    assert!(next.conflict_clusters.is_empty());
    let before = snapshot(&directory.path().join("target"));
    apply(&engine, &next).unwrap();
    assert_eq!(snapshot(&directory.path().join("target")), before);
}

/// A schema-valid edit of the tool-owned envelope block is attributable (DEC-21 A) and omitted as a
/// home-only change; a change outside every attributable pointer keeps the human-decision refusal.
#[test]
fn envelope_edits_are_attributed_and_unknown_fields_stay_a_human_decision() {
    let directory = fixture();
    let original = record("opaque", "Stable body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", original.canonical_id));
    let text = fs::read_to_string(&path).unwrap();
    let (mut metadata, body) = native(&text);
    metadata["mem_adaptor"]["scope"] = json!("project");
    fs::write(
        &path,
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let engine = okf_engine(&directory, vec![original.clone()]);
    let next = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Omitted {
            reason: OmissionReason::HomeModified {
                home_changed_fields: vec!["/frontmatter/mem_adaptor".into()]
            }
        }
    );

    // The same file with an added unknown frontmatter key changes bytes without touching the
    // envelope or any declared pointer: nothing attributes that edit, so it stays refused.
    let (mut metadata, body) = native(&text);
    metadata["neighbour_note"] = json!("added by hand");
    fs::write(
        &path,
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let next = plan(&engine, &directory, Some(&previous));
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetModified
        }
    );
}

/// A verdict cannot take home a change that has no field list to report: the entry stays unresolved
/// instead of emitting an omission the report schema rejects (which would fail the whole plan).
#[test]
fn take_home_verdicts_need_an_attributable_home_change() {
    let directory = fixture();
    let original = record("vague", "Stable body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let previous = history(&directory, &receipt, "previous.json");
    let path = directory
        .path()
        .join(format!("target/memories/{}.md", original.canonical_id));
    let text = fs::read_to_string(&path).unwrap();
    let (mut metadata, body) = native(&text);
    metadata["neighbour_note"] = json!("added by hand");
    fs::write(
        &path,
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let mut updated = record("vague", "Satellite rewrite.\n");
    updated.content_hash = content_hash(updated.content.as_bytes());
    let engine = okf_engine(&directory, vec![updated]);
    let conflict = plan(&engine, &directory, Some(&previous));
    let cluster_id = conflict.conflict_clusters[0].cluster_id.clone();
    let mut adjudicated = receipt.clone();
    adjudicated.verdicts = vec![Verdict::Keep {
        cluster_id,
        canonical_ids: vec![original.canonical_id.clone()],
        bases: Some(vec![format!("home:memories/{}", original.canonical_id)]),
    }];
    let verdict_path = history(&directory, &adjudicated, "verdict.json");
    let next = plan(&engine, &directory, Some(&verdict_path));
    assert!(matches!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::Conflict { .. }
        }
    ));
    assert_eq!(next.conflict_clusters.len(), 1);
    let before = snapshot(&directory.path().join("target"));
    apply(&engine, &next).unwrap();
    assert_eq!(snapshot(&directory.path().join("target")), before);
}

/// An internally inconsistent managed identity lets unrelated planning continue but refuses writes.
#[test]
fn corrupt_managed_source_identity_plans_on_but_refuses_writes_without_changes() {
    let directory = fixture();
    let original = record("old", "Existing managed body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let text = note(&directory, &original.canonical_id);
    let (mut metadata, body) = native(&text);
    metadata["mem_adaptor"]["source_record_id"] = json!("different-source-record");
    fs::write(
        directory
            .path()
            .join(format!("target/memories/{}.md", original.canonical_id)),
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let before = snapshot(&directory.path().join("target"));
    let engine = okf_engine(
        &directory,
        vec![record("unrelated", "Other distinct body.\n")],
    );
    // DEC-21 A/D: planning classifies the damaged envelope per file instead of aborting, so the
    // unrelated record plans on; the damaged managed record then refuses the write before any target
    // byte changes, because the index rebuild cannot represent it (testing-policy D).
    let next = engine
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert_eq!(next.entries[0].disposition, Disposition::Accepted);
    let error = apply(&engine, &next).unwrap_err();
    assert!(
        format!("{error:#}").contains("Managed OKF envelope is invalid"),
        "{error:#}"
    );
    assert_eq!(snapshot(&directory.path().join("target")), before);
    assert!(!directory.path().join("synthetic-receipt.json").exists());
}

/// Refuses a managed envelope whose stored satellite ID disagrees with its own canonical identity (DEC-20);
/// the stored satellite participates in the consistency check, so corrupting it alone is already a mismatch.
#[test]
fn corrupt_managed_satellite_id_plans_on_but_refuses_writes_without_changes() {
    let directory = fixture();
    let original = record("old", "Existing managed body.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let text = note(&directory, &original.canonical_id);
    let (mut metadata, body) = native(&text);
    metadata["mem_adaptor"]["source"]["satellite_id"] = json!("aaaaaaa2");
    fs::write(
        directory
            .path()
            .join(format!("target/memories/{}.md", original.canonical_id)),
        format!(
            "---\n{}---\n{body}",
            serde_saphyr::to_string(&metadata).unwrap()
        ),
    )
    .unwrap();
    let before = snapshot(&directory.path().join("target"));
    let engine = okf_engine(
        &directory,
        vec![record("unrelated", "Other distinct body.\n")],
    );
    // DEC-21 A/D: planning classifies the damaged envelope per file instead of aborting, so the
    // unrelated record plans on; the damaged managed record then refuses the write before any target
    // byte changes, because the index rebuild cannot represent it (testing-policy D).
    let next = engine
        .plan(&directory.path().join("source"), policy())
        .unwrap();
    assert_eq!(next.entries[0].disposition, Disposition::Accepted);
    let error = apply(&engine, &next).unwrap_err();
    assert!(
        format!("{error:#}").contains("Managed OKF envelope is invalid"),
        "{error:#}"
    );
    assert_eq!(snapshot(&directory.path().join("target")), before);
    assert!(!directory.path().join("synthetic-receipt.json").exists());
}

/// Keeps same-name ordinary Markdown untracked while applying an independently accepted record.
#[test]
fn ordinary_same_name_markdown_is_partial_not_a_global_parse_failure() {
    let directory = fixture();
    let blocked = record("user-owned", "Source wants this name.\n");
    let accepted = record("accepted", "Independent accepted body.\n");
    fs::create_dir_all(directory.path().join("target/memories")).unwrap();
    let owned = directory
        .path()
        .join(format!("target/memories/{}.md", blocked.canonical_id));
    let bytes = b"---\ntitle: User-owned ordinary note\n---\nKeep all bytes.\r\n\0";
    fs::write(&owned, bytes).unwrap();
    let engine = okf_engine(&directory, vec![blocked.clone(), accepted.clone()]);
    let report = plan(&engine, &directory, None);
    assert_eq!(
        report
            .entries
            .iter()
            .find(|entry| entry.canonical_id == blocked.canonical_id)
            .unwrap()
            .disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetUntracked
        }
    );
    assert_eq!(
        report
            .entries
            .iter()
            .find(|entry| entry.canonical_id == accepted.canonical_id)
            .unwrap()
            .disposition,
        Disposition::Accepted
    );
    let receipt = apply(&engine, &report).unwrap();
    assert_eq!(fs::read(owned).unwrap(), bytes);
    assert_eq!(
        native(&note(&directory, &accepted.canonical_id)).1,
        accepted.content
    );
    assert_eq!(
        receipt
            .entries
            .iter()
            .find(|entry| entry.canonical_id == accepted.canonical_id)
            .unwrap()
            .verification,
        Some(Verification::Verified)
    );
    let index = fs::read_to_string(directory.path().join("target/index.md")).unwrap();
    assert!(!index.contains(&blocked.canonical_id));
}

/// Separately invalidates filename length and alphabet so neither predicate can be accidentally weakened.
#[test]
fn invalid_length_only_and_alphabet_only_names_are_not_parsed() {
    let directory = fixture();
    fs::create_dir_all(directory.path().join("target/memories")).unwrap();
    for name in ["a".repeat(31), format!("0{}", "a".repeat(31))] {
        fs::write(
            directory.path().join(format!("target/memories/{name}.md")),
            b"---\nmem_adaptor_envelope: [bad YAML\n---\n",
        )
        .unwrap();
    }
    let before = snapshot(&directory.path().join("target"));
    let engine = okf_engine(&directory, vec![record("new", "New native body.\n")]);
    let report = plan(&engine, &directory, None);
    let receipt = apply(&engine, &report).unwrap();
    verified(&receipt);
    let after = snapshot(&directory.path().join("target"));
    for (path, bytes) in before {
        assert_eq!(after.get(&path), Some(&bytes));
    }
    assert!(
        !fs::read_to_string(directory.path().join("target/index.md"))
            .unwrap()
            .contains(&"a".repeat(31))
    );
}

/// Distinguishes actual target enumeration and inspect failures from an absent record.
#[test]
fn not_a_directory_is_neither_empty_inventory_nor_missing_record() {
    let directory = fixture();
    fs::create_dir(directory.path().join("target")).unwrap();
    fs::write(
        directory.path().join("target/memories"),
        b"Protected obstruction",
    )
    .unwrap();
    let engine = okf_engine(&directory, vec![record("new", "New native body.\n")]);
    let before = snapshot(&directory.path().join("target"));
    let error = engine
        .plan(&directory.path().join("source"), policy())
        .unwrap_err();
    assert!(format!("{error:#}").contains("directory"));
    let writer = engine.registry.writer("home").unwrap();
    let error = writer
        .inspect(&format!("memories/{}", "a".repeat(32)))
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("regular file with directory ancestors")
    );
    let error = target::read_file(
        writer.location(),
        &format!("memories/{}.md", "a".repeat(32)),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("regular file with directory ancestors")
    );
    assert_eq!(snapshot(&directory.path().join("target")), before);
}

/// Treats an existing plain replacement as de-managed: refused for a human decision, not deleted.
#[test]
fn history_with_plain_replacement_is_unmanaged_not_deleted() {
    let directory = fixture();
    let original = record("existing", "Original managed body.\n");
    let engine = okf_engine(&directory, vec![original.clone()]);
    let receipt = apply(&engine, &plan(&engine, &directory, None)).unwrap();
    let history = directory.path().join("previous.json");
    write_json_new(&history, &receipt).unwrap();
    let history_bytes = fs::read(&history).unwrap();
    fs::write(
        directory
            .path()
            .join(format!("target/memories/{}.md", original.canonical_id)),
        "User replacement without an envelope.\n",
    )
    .unwrap();
    let before = snapshot(directory.path());
    let next = plan(&engine, &directory, Some(&history));
    // DEC-21 A: the envelope is gone, so the file is an ordinary note and the entry asks for a human
    // decision instead of the record-level modified refusal.
    assert_eq!(
        next.entries[0].disposition,
        Disposition::Unresolved {
            reason: UnresolvedReason::TargetUnmanaged,
        }
    );
    let skipped = apply(&engine, &next).unwrap();
    assert!(skipped.entries[0].verification.is_none());
    assert_eq!(
        skipped.entries[0].prior_write,
        receipt.entries[0].prior_write
    );
    assert_eq!(snapshot(directory.path()), before);
    assert_eq!(fs::read(history).unwrap(), history_bytes);
}

/// Isolates read_dir and metadata PermissionDenied guards from each other's earlier control checks.
/// Search-only permissions keep a known record readable while enumeration fails; no-search permissions
/// then exercise relative-file and full-directory metadata guards, with snapshots after restoration.
#[cfg(unix)]
#[test]
fn enumeration_and_inspect_io_failures_do_not_become_absence() {
    use std::os::unix::fs::PermissionsExt;
    let directory = fixture();
    let original = record("existing", "Protected managed payload.\n");
    let initial = okf_engine(&directory, vec![original.clone()]);
    apply(&initial, &plan(&initial, &directory, None)).unwrap();
    let memories = directory.path().join("target/memories");
    fs::create_dir(memories.join("inner")).unwrap();
    let before = snapshot(&directory.path().join("target"));
    let path = memories.join(format!("{}.md", original.canonical_id));
    let record_bytes = fs::read(&path).unwrap();
    let permissions = fs::metadata(&memories).unwrap().permissions();
    fs::set_permissions(&memories, fs::Permissions::from_mode(0o111)).unwrap();
    let probe = fs::read_dir(&memories);
    if probe.is_ok() {
        fs::set_permissions(&memories, permissions).unwrap();
        eprintln!("Skipping read permission faults: this user bypasses directory permissions");
        return;
    }
    assert_eq!(
        probe.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    assert_eq!(fs::read(&path).unwrap(), record_bytes);
    let planned = initial.plan(&directory.path().join("source"), policy());
    fs::set_permissions(&memories, permissions.clone()).unwrap();
    let error = planned.unwrap_err();
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    }));
    assert_eq!(snapshot(&directory.path().join("target")), before);
    fs::set_permissions(&memories, fs::Permissions::from_mode(0o000)).unwrap();
    let inspected = initial
        .registry
        .writer("home")
        .unwrap()
        .inspect(&format!("memories/{}", original.canonical_id));
    let directory_result = target::directory_exists(
        &initial
            .registry
            .writer("home")
            .unwrap()
            .location()
            .join("memories/inner"),
    );
    fs::set_permissions(&memories, permissions).unwrap();
    let error = inspected.unwrap_err();
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    }));
    let error = directory_result.unwrap_err();
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    }));
    assert_eq!(snapshot(&directory.path().join("target")), before);
    assert!(!directory.path().join("synthetic-receipt.json").exists());
}

enum Injection {
    Crlf,
    Body,
    Duplicate,
    Destination(PathBuf),
    BeforePersist,
    AfterOnePersist,
    ReadBack,
    AfterReadBack,
    #[cfg(unix)]
    SwapParent {
        parent: PathBuf,
        moved: PathBuf,
        outside: PathBuf,
    },
}

struct BoundaryWriter {
    inner: OkfWriter,
    injection: Injection,
}

impl Writer for BoundaryWriter {
    /// Preserves plugin identity so the real engine binds its token to the actual OKF implementation.
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    /// Preserves production adapter version during approval recomputation.
    fn version(&self) -> &'static str {
        self.inner.version()
    }
    /// Keeps the approved destination unchanged until a deliberate write-boundary attack.
    fn location(&self) -> &Path {
        self.inner.location()
    }
    /// Retains actual OKF supported-field declarations.
    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }
    /// Delegates planning so only execution, not the approved proposal, is perturbed.
    fn plan(&self, record: &CanonicalRecord, previous: Option<&ReceiptEntry>) -> Result<Planned> {
        self.inner.plan(record, previous)
    }
    /// Mutates only at the real token boundary or injects an ordinary error at a specified persistence stage.
    fn write(&self, batch: &[Planned], token: &WriteToken) -> Result<WriteResult> {
        match &self.injection {
            Injection::Crlf => {
                let mut result = self.inner.write(batch, token)?;
                for written in &mut result.written {
                    let path = format!("{}.md", written.target_id);
                    let text = fs::read_to_string(self.location().join(&path))?;
                    let (_, body) = native(&text);
                    let end = text.len() - body.len();
                    let crlf = format!("{}{}", text[..end].replace('\n', "\r\n"), body);
                    fs::write(self.location().join(&path), &crlf)?;
                    written.target_hash = content_hash(crlf.as_bytes());
                    let artifact = result
                        .artifacts
                        .iter_mut()
                        .find(|artifact| artifact.path == path)
                        .unwrap();
                    *artifact = target::output_artifact(&path, crlf.as_bytes());
                }
                Ok(result)
            }
            Injection::Body | Injection::Duplicate => {
                let mut altered: Vec<Planned> = batch.iter().map(copy_planned).collect();
                if matches!(self.injection, Injection::Body) {
                    altered[0].record.content.push_str("Unapproved body");
                    altered[0].record.content_hash =
                        content_hash(altered[0].record.content.as_bytes());
                } else {
                    altered.push(copy_planned(&batch[0]));
                }
                self.inner.write(&altered, token)
            }
            Injection::Destination(path) => OkfWriter::new(path.clone())?.write(batch, token),
            Injection::BeforePersist => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Synthetic first persistence refused",
            )
            .into()),
            Injection::AfterOnePersist => {
                token.authorize(&self.inner, batch)?;
                let planned = &batch[0];
                let mut metadata = serde_json::to_value(&planned.record)?;
                metadata.as_object_mut().unwrap().remove("content");
                let envelope = json!({"type": "Memory", "mem_adaptor_envelope": "okf:0.2", "mem_adaptor": metadata});
                let bytes = format!(
                    "---\n{}---\n{}",
                    serde_saphyr::to_string(&envelope)?,
                    planned.record.content
                );
                let path = format!("{}.md", planned.target_id);
                token.authorize_artifact(&self.inner, &path, None)?;
                target::atomic_file(self.location(), &path, bytes.as_bytes(), None)?;
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "Synthetic shared index persistence refused",
                )
                .into())
            }
            #[cfg(unix)]
            Injection::SwapParent {
                parent,
                moved,
                outside,
            } => {
                fs::rename(parent, moved)?;
                std::os::unix::fs::symlink(outside, parent)?;
                self.inner.write(batch, token)
            }
            _ => self.inner.write(batch, token),
        }
    }
    /// Injects either an ordinary read-back failure or invalidates the output proof after successful reading.
    fn read_back(&self, written: &[Written]) -> Result<Vec<ReadBack>> {
        if matches!(self.injection, Injection::ReadBack) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Synthetic read-back refused",
            )
            .into());
        }
        let result = self.inner.read_back(written)?;
        if matches!(self.injection, Injection::AfterReadBack) {
            let path = self.location().join("log.md");
            let mut bytes = fs::read(&path)?;
            bytes.extend_from_slice(b"\nUnapproved log change.\n");
            fs::write(path, bytes)?;
        }
        Ok(result)
    }
    /// Keeps pre-write inspection real and error-propagating.
    fn inspect(&self, id: &str) -> Result<Option<CanonicalRecord>> {
        self.inner.inspect(id)
    }
    /// Keeps the approved record byte proofs real.
    fn target_hash(&self, id: &str) -> Result<Option<String>> {
        self.inner.target_hash(id)
    }
    /// Uses production inventory for approval and post-write proof checks.
    fn artifacts(&self, ids: &[String]) -> Result<Vec<TargetArtifact>> {
        self.inner.artifacts(ids)
    }
    /// Keeps actual shared-artifact ownership boundaries.
    fn shared_artifact_paths(&self) -> &'static [&'static str] {
        self.inner.shared_artifact_paths()
    }
}

/// Copies the explicit token-bound proposal without adding a production Clone abstraction.
fn copy_planned(planned: &Planned) -> Planned {
    Planned {
        record: planned.record.clone(),
        disposition: planned.disposition.clone(),
        target_id: planned.target_id.clone(),
        previous_write: planned.previous_write.clone(),
        duplicate_write: planned.duplicate_write.clone(),
        target_map: planned.target_map.clone(),
    }
}

/// Rejects tampered content, injected entries, and destination changes with complete no-write snapshots.
#[test]
fn real_write_token_rejects_body_duplicate_and_destination_tampering() {
    for mode in 0..3 {
        let directory = fixture();
        fs::create_dir(directory.path().join("target")).unwrap();
        fs::write(
            directory.path().join("target/user.txt"),
            b"All protected bytes",
        )
        .unwrap();
        fs::create_dir(directory.path().join("outside")).unwrap();
        fs::write(
            directory.path().join("outside/user.txt"),
            b"Outside protected bytes",
        )
        .unwrap();
        let injection = match mode {
            0 => Injection::Body,
            1 => Injection::Duplicate,
            _ => Injection::Destination(directory.path().join("outside")),
        };
        let engine = engine(
            vec![record("token", "Approved body.\n")],
            BoundaryWriter {
                inner: OkfWriter::new(directory.path().join("target")).unwrap(),
                injection,
            },
        );
        let approved = plan(&engine, &directory, None);
        let before = snapshot(directory.path());
        let error = apply(&engine, &approved).unwrap_err();
        assert!(format!("{error:#}").contains("WriteToken does not authorize"));
        assert_eq!(snapshot(directory.path()), before);
        assert!(!directory.path().join("synthetic-receipt.json").exists());
    }
}

/// Checks actual persisted bytes and no returned receipt for first-write, partial-write, and verification faults.
#[test]
fn engine_failure_stages_distinguish_prewrite_partial_and_readback_results() {
    for injection in [
        Injection::BeforePersist,
        Injection::AfterOnePersist,
        Injection::ReadBack,
        Injection::AfterReadBack,
    ] {
        let directory = fixture();
        let original = record("fault", "Actual approved payload.\n");
        let before_persist = matches!(injection, Injection::BeforePersist);
        let partial = matches!(injection, Injection::AfterOnePersist);
        let readback = matches!(injection, Injection::ReadBack | Injection::AfterReadBack);
        let proof_change = matches!(injection, Injection::AfterReadBack);
        let engine = engine(
            vec![original.clone()],
            BoundaryWriter {
                inner: OkfWriter::new(directory.path().join("target")).unwrap(),
                injection,
            },
        );
        let approved = plan(&engine, &directory, None);
        let before = snapshot(directory.path());
        let error = apply(&engine, &approved).unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains(if readback { "[S9]" } else { "[S8]" }),
            "{message}"
        );
        for expected in [
            "targets may be partially changed",
            "No reliable final receipt",
            "saved approval",
            "do not blindly retry",
        ] {
            assert!(message.contains(expected), "{message}");
        }
        if proof_change {
            assert!(
                message.contains("Writer output changed before"),
                "{message}"
            );
            assert!(
                fs::read(directory.path().join("target/log.md"))
                    .unwrap()
                    .ends_with(b"\nUnapproved log change.\n")
            );
        } else {
            assert!(error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
            }));
        }
        if before_persist {
            assert_eq!(snapshot(directory.path()), before);
        } else {
            assert_ne!(snapshot(directory.path()), before);
            assert_eq!(
                native(&note(&directory, &original.canonical_id)).1,
                original.content
            );
            assert_eq!(directory.path().join("target/index.md").exists(), !partial);
            assert_eq!(directory.path().join("target/log.md").exists(), !partial);
        }
        assert!(!directory.path().join("synthetic-receipt.json").exists());
    }
}

/// Swaps an approved parent at write entrance and proves both original and outside trees stay untouched.
#[cfg(unix)]
#[test]
fn changed_target_parent_is_refused_without_outside_write_or_success_receipt() {
    let directory = fixture();
    let parent = directory.path().join("approved-parent");
    let moved = directory.path().join("moved-parent");
    let outside = directory.path().join("outside");
    fs::create_dir_all(parent.join("target")).unwrap();
    fs::write(parent.join("target/user.txt"), b"Original protected target").unwrap();
    fs::create_dir_all(outside.join("target")).unwrap();
    fs::write(outside.join("target/user.txt"), b"Outside protected target").unwrap();
    let before_original = snapshot(&parent);
    let before_outside = snapshot(&outside);
    let engine = engine(
        vec![record("swap", "Never write outside.\n")],
        BoundaryWriter {
            inner: OkfWriter::new(parent.join("target")).unwrap(),
            injection: Injection::SwapParent {
                parent: parent.clone(),
                moved: moved.clone(),
                outside: outside.clone(),
            },
        },
    );
    let approved = plan(&engine, &directory, None);
    let error = apply(&engine, &approved).unwrap_err();
    assert!(format!("{error:#}").to_lowercase().contains("symlink"));
    assert!(
        fs::symlink_metadata(&parent)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(snapshot(&moved), before_original);
    assert_eq!(snapshot(&outside), before_outside);
    assert!(!directory.path().join("synthetic-receipt.json").exists());
}

/// Runs actual CLI commands with captured logs, preserving noninteractive approval semantics.
/// The isolated configuration root keeps direct-mode runs away from the real user configuration.
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .args(args)
        .env("LOG_LEVEL", "info")
        .env("XDG_CONFIG_HOME", common::config_home())
        .output()
        .unwrap()
}

/// Checks that a CLI failure is ordinary and contains no synthetic secret value on either output stream.
fn failure(output: &Output, secret: &str) -> String {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(!stderr.contains("panicked"));
    assert!(!stderr.contains(secret));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    stderr
}

/// Uses actual CLI permission faults before the first persist and after a record but before the shared index.
#[cfg(unix)]
#[test]
fn cli_write_failures_preserve_evidence_and_honestly_report_partial_targets() {
    use std::os::unix::fs::PermissionsExt;
    for partial in [false, true] {
        let directory = fixture();
        let root = directory.path();
        fs::remove_file(root.join("source/synthetic.json")).unwrap();
        let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
        fs::write(
            root.join("source/note.md"),
            format!("Synthetic CLI payload.\n{secret}\n"),
        )
        .unwrap();
        let target_root = root.join("target");
        fs::create_dir(&target_root).unwrap();
        fs::write(target_root.join("user.txt"), b"Protected user bytes").unwrap();
        if partial {
            fs::create_dir(target_root.join("memories")).unwrap();
        }
        let output = cli(&[
            "plan",
            root.join("source").to_str().unwrap(),
            "--to",
            &format!("okf:{}", target_root.display()),
            "--report",
            root.join("plan.json").to_str().unwrap(),
        ]);
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&secret));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(&secret));
        let before = snapshot(root);
        let original_permissions = fs::metadata(&target_root).unwrap().permissions();
        fs::set_permissions(&target_root, fs::Permissions::from_mode(0o500)).unwrap();
        // Elevated users can bypass permission bits; prove the fault is usable instead of claiming false coverage.
        let probe = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target_root.join("permission-probe"));
        if probe.is_ok() {
            drop(probe);
            fs::set_permissions(&target_root, original_permissions).unwrap();
            fs::remove_file(target_root.join("permission-probe")).unwrap();
            eprintln!("Skipping permission fault: this user bypasses directory write permissions");
            continue;
        }
        assert_eq!(
            probe.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
        fs::set_permissions(&target_root, original_permissions).unwrap();
        let message = failure(&output, &secret);
        for expected in [
            "[S8]",
            "targets may",
            "No reliable final receipt",
            "Inspect all targets",
            "approval was saved",
            "do not blindly retry",
        ] {
            assert!(
                message.contains(expected),
                "Missing failure context: {expected}"
            );
        }
        let after = snapshot(root);
        assert_eq!(after.get("plan.json"), before.get("plan.json"));
        assert_eq!(after.get("target/user.txt"), before.get("target/user.txt"));
        let approval_bytes = fs::read(root.join("plan.approval.json")).unwrap();
        assert!(!String::from_utf8_lossy(&approval_bytes).contains(&secret));
        assert!(!String::from_utf8_lossy(after.get("plan.json").unwrap()).contains(&secret));
        let approval: ApprovalReceipt = serde_json::from_slice(&approval_bytes).unwrap();
        let report: PlanReport = serde_json::from_slice(before.get("plan.json").unwrap()).unwrap();
        assert_eq!(approval.plan_digest, report.plan_digest);
        assert!(!root.join("plan.receipt.json").exists());
        assert!(!target_root.join("index.md").exists());
        assert!(!target_root.join("log.md").exists());
        if partial {
            assert_eq!(
                fs::read_dir(target_root.join("memories")).unwrap().count(),
                1
            );
            let text = fs::read_to_string(
                fs::read_dir(target_root.join("memories"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path(),
            )
            .unwrap();
            assert_eq!(
                native(&text).1,
                format!("Synthetic CLI payload.\n{secret}\n")
            );
        } else {
            let protected: BTreeMap<_, _> = before
                .into_iter()
                .filter(|(path, _)| path.starts_with("target/"))
                .collect();
            let actual: BTreeMap<_, _> = after
                .into_iter()
                .filter(|(path, _)| path.starts_with("target/"))
                .collect();
            assert_eq!(actual, protected);
        }
    }
}

/// Keeps approval-time refusals separate from failures after the Writer was entered.
#[test]
fn cli_changed_source_is_s7_no_write_and_no_secret_leak() {
    let directory = fixture();
    let root = directory.path();
    fs::remove_file(root.join("source/synthetic.json")).unwrap();
    let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    fs::write(
        root.join("source/note.md"),
        format!("Synthetic body.\n{secret}\n"),
    )
    .unwrap();
    fs::create_dir(root.join("target")).unwrap();
    fs::write(root.join("target/user.txt"), b"Protected unchanged bytes").unwrap();
    let output = cli(&[
        "plan",
        root.join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", root.join("target").display()),
        "--report",
        root.join("plan.json").to_str().unwrap(),
    ]);
    assert!(output.status.success());
    fs::write(
        root.join("source/note.md"),
        format!("Changed body.\n{secret}\n"),
    )
    .unwrap();
    let before = snapshot(root);
    let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
    let message = failure(&output, &secret);
    assert!(message.contains("[S7]"), "{message}");
    assert!(message.contains("before target writes began"), "{message}");
    let after = snapshot(root);
    for (path, bytes) in &before {
        assert_eq!(
            after.get(path),
            Some(bytes),
            "Existing bytes changed: {path}"
        );
    }
    let approval = fs::read(root.join("plan.approval.json")).unwrap();
    assert!(!String::from_utf8_lossy(&approval).contains(&secret));
    assert!(!root.join("plan.receipt.json").exists());
    assert_eq!(
        snapshot(&root.join("target")),
        [("user.txt".into(), b"Protected unchanged bytes".to_vec())]
            .into_iter()
            .collect()
    );
}

/// Refuses a newly linked approved ancestor before saving approval, without resolving a replacement destination.
#[cfg(unix)]
#[test]
fn cli_approved_parent_link_is_not_renormalized_into_a_new_destination() {
    let directory = fixture();
    let root = directory.path();
    fs::remove_file(root.join("source/synthetic.json")).unwrap();
    fs::write(root.join("source/note.md"), "Approved path stays fixed.\n").unwrap();
    let parent = root.join("parent");
    let moved = root.join("moved");
    let outside = root.join("outside");
    fs::create_dir_all(parent.join("target")).unwrap();
    fs::write(parent.join("target/user.txt"), b"Original target bytes").unwrap();
    fs::create_dir_all(outside.join("target")).unwrap();
    fs::write(outside.join("target/user.txt"), b"Outside target bytes").unwrap();
    let output = cli(&[
        "plan",
        root.join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", parent.join("target").display()),
        "--report",
        root.join("plan.json").to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan_bytes = fs::read(root.join("plan.json")).unwrap();
    let before_original = snapshot(&parent);
    let before_outside = snapshot(&outside);
    fs::rename(&parent, &moved).unwrap();
    std::os::unix::fs::symlink(&outside, &parent).unwrap();
    let output = cli(&["apply", root.join("plan.json").to_str().unwrap(), "--yes"]);
    let message = failure(&output, "unused-synthetic-secret");
    assert!(
        message.contains("[S7] Approved target path changed before target writes"),
        "{message}"
    );
    assert!(message.contains("before approval was saved"), "{message}");
    assert_eq!(snapshot(&moved), before_original);
    assert_eq!(snapshot(&outside), before_outside);
    assert_eq!(fs::read(root.join("plan.json")).unwrap(), plan_bytes);
    assert!(!root.join("plan.approval.json").exists());
    assert!(!root.join("plan.receipt.json").exists());
}

/// Refuses an explicit target-root link without changing outside bytes or creating any migration credentials.
#[cfg(unix)]
#[test]
fn cli_root_symlink_refuses_before_plan_and_credentials() {
    let directory = fixture();
    fs::write(
        directory.path().join("source/note.md"),
        "Synthetic ordinary note.\n",
    )
    .unwrap();
    fs::remove_file(directory.path().join("source/synthetic.json")).unwrap();
    fs::create_dir(directory.path().join("outside")).unwrap();
    fs::write(
        directory.path().join("outside/user.txt"),
        b"Outside sentinel",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        directory.path().join("outside"),
        directory.path().join("target"),
    )
    .unwrap();
    let before = snapshot(&directory.path().join("outside"));
    let secret = format!("sk-{}T3BlbkFJ{}", "A".repeat(20), "B".repeat(20));
    let report = directory.path().join(format!("plan-{secret}.json"));
    let output = cli(&[
        "plan",
        directory.path().join("source").to_str().unwrap(),
        "--to",
        &format!("okf:{}", directory.path().join("target").display()),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(failure(&output, &secret).to_lowercase().contains("symlink"));
    assert_eq!(snapshot(&directory.path().join("outside")), before);
    assert!(!report.exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 3);
}

/// Bounds a FIFO refusal in an owned test subprocess so a future blocking-read regression cannot hang the suite.
#[cfg(unix)]
#[test]
fn fifo_is_refused_before_open_with_bounded_child() {
    use std::os::unix::fs::FileTypeExt;
    use std::time::{Duration, Instant};
    const CHILD_ROOT: &str = "MEM_ADAPTOR_OKF_FIFO_CHILD_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let error = target::read_file(Path::new(&root), "index.md").unwrap_err();
        assert!(format!("{error:#}").contains("regular file"));
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(directory.path()).unwrap();
    let fifo = root.join("index.md");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo());
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "fifo_is_refused_before_open_with_bounded_child",
            "--nocapture",
        ])
        .env(CHILD_ROOT, &root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "FIFO refusal child failed");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("FIFO read blocked instead of refusing a special file");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(fs::symlink_metadata(&fifo).unwrap().file_type().is_fifo());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}
