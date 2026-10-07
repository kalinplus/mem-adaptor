//! Exercises historical mapping and privacy regressions through real local adapters.
//! All sources and targets are isolated synthetic fixtures; these tests do not establish conformance.

use std::fs;
use std::path::Path;

use mem_adaptor_core::engine::{Engine, canonical_id, timestamp, write_json_new};
use mem_adaptor_core::governance::*;
use mem_adaptor_core::plugins::Registry;
use mem_adaptor_core::reports::*;
use mem_adaptor_reader_markdown::MarkdownReader;
use mem_adaptor_writer_okf::OkfWriter;
use mem_adaptor_writer_ump::UmpWriter;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    directory
}

/// Registers a validated physical test target for the selected local Writer.
fn engine(directory: &TempDir, writer: &str) -> Engine {
    let mut registry = Registry::default();
    registry.register_reader(MarkdownReader).unwrap();
    let target = directory.path().join("target");
    if writer == "okf" {
        registry
            .register_writer("home".into(), OkfWriter::new(target).unwrap())
            .unwrap();
    } else {
        registry
            .register_writer("home".into(), UmpWriter::new(target).unwrap())
            .unwrap();
    }
    Engine { registry }
}

fn policy(action: GateAction) -> GatePolicy {
    GatePolicy {
        secrets: action,
        high_risk_pii: GateAction::Pass,
        rule_allowlist: vec![],
        origin: PolicyOrigin::Default,
        user_selected: false,
    }
}
fn plan(engine: &Engine, directory: &TempDir, previous: Option<&Path>) -> PlanReport {
    engine
        .plan_with_previous(
            &directory.path().join("source"),
            policy(GateAction::Pass),
            previous,
        )
        .unwrap()
}
fn apply(engine: &Engine, report: &PlanReport) -> ReceiptReport {
    engine
        .apply(
            report,
            &ApprovalReceipt {
                schema_version: "0.1.0".into(),
                receipt_id: "synthetic".into(),
                plan_digest: report.plan_digest.clone(),
                approved_at: timestamp().unwrap(),
                backend: "local".into(),
                approver: "synthetic".into(),
            },
            "plan".into(),
            "approval".into(),
        )
        .unwrap()
}
fn history(directory: &TempDir, receipt: &ReceiptReport, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    write_json_new(&path, receipt).unwrap();
    path
}

#[test]
fn nested_secret_keys_are_detected_in_every_policy_without_leaking_report_paths() {
    let secret = format!("ghp_TEST{}", "A".repeat(32));
    for writer in ["okf", "ump"] {
        for (action, allowlisted) in [
            (GateAction::Block, false),
            (GateAction::Pass, false),
            (GateAction::Block, true),
        ] {
            let directory = fixture();
            fs::write(
                directory.path().join("source/note.md"),
                format!("---\ncustom:\n  items:\n  - {secret}: synthetic\n---\nSynthetic body.\n"),
            )
            .unwrap();
            let engine = engine(&directory, writer);
            let mut policy = policy(action);
            if allowlisted {
                policy.rule_allowlist.push("github-pat".into());
            }
            let report = engine
                .plan(&directory.path().join("source"), policy)
                .unwrap();
            assert!(
                report.entries[0]
                    .sensitive_findings
                    .iter()
                    .any(|finding| finding.key_hash.is_some())
            );
            assert!(!serde_json::to_string(&report).unwrap().contains(&secret));
            let receipt = apply(&engine, &report);
            assert!(!serde_json::to_string(&receipt).unwrap().contains(&secret));
            if action == GateAction::Block && !allowlisted {
                assert!(matches!(
                    report.entries[0].disposition,
                    Disposition::Rejected { .. }
                ));
                assert!(!directory.path().join("target").exists());
            } else {
                assert_eq!(
                    receipt.entries[0].verification,
                    Some(Verification::Verified)
                );
            }
        }
    }
}

#[test]
fn unsafe_metadata_integers_fail_before_planning_or_writing() {
    for writer in ["okf", "ump"] {
        for number in [
            "9007199254740992",
            "9007199254740993",
            "-9007199254740992",
            "1e20",
        ] {
            let directory = fixture();
            fs::write(
                directory.path().join("source/note.md"),
                format!("---\ncustom:\n  count: {number}\n---\nSynthetic body.\n"),
            )
            .unwrap();
            let error = engine(&directory, writer)
                .plan(&directory.path().join("source"), policy(GateAction::Pass))
                .unwrap_err();
            assert!(error.to_string().contains("JCS safe range"));
            assert!(!error.to_string().contains(number));
            assert!(!directory.path().join("target").exists());
        }
    }
}

#[test]
fn changing_only_the_duplicate_survivor_keeps_the_alias_old_body() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        for name in ["a.md", "b.md"] {
            fs::write(
                directory.path().join("source").join(name),
                "Original synthetic body.\n",
            )
            .unwrap();
        }
        let engine = engine(&directory, writer);
        let first = apply(&engine, &plan(&engine, &directory, None));
        let survivor = first
            .entries
            .iter()
            .find(|entry| entry.verification == Some(Verification::Verified))
            .unwrap();
        fs::write(
            directory
                .path()
                .join("source")
                .join(&survivor.source_record_id),
            "Changed survivor body.\n",
        )
        .unwrap();
        let previous = history(&directory, &first, "first.json");
        let second = apply(&engine, &plan(&engine, &directory, Some(&previous)));
        let mut bodies: Vec<_> = second
            .entries
            .iter()
            .filter(|entry| entry.verification == Some(Verification::Verified))
            .map(|entry| {
                engine
                    .registry
                    .writer("home")
                    .unwrap()
                    .inspect(entry.target_id.as_ref().unwrap())
                    .unwrap()
                    .unwrap()
                    .content
            })
            .collect();
        bodies.sort();
        assert_eq!(
            bodies,
            ["Changed survivor body.\n", "Original synthetic body.\n"]
        );
        let previous = history(&directory, &second, "second.json");
        assert!(
            plan(&engine, &directory, Some(&previous))
                .entries
                .iter()
                .all(|entry| entry.disposition
                    == Disposition::Omitted {
                        reason: OmissionReason::AlreadyMigrated
                    })
        );
    }
}

#[test]
fn a_new_earlier_duplicate_never_replaces_the_existing_written_identity() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        fs::write(
            directory.path().join("source/a.md"),
            "Original synthetic body.\n",
        )
        .unwrap();
        let engine = engine(&directory, writer);
        let first = apply(&engine, &plan(&engine, &directory, None));
        let new_name = (0..100)
            .map(|n| format!("new{n}.md"))
            .find(|name| canonical_id("markdown", name) < first.entries[0].canonical_id)
            .unwrap();
        fs::write(
            directory.path().join("source").join(&new_name),
            "Original synthetic body.\n",
        )
        .unwrap();
        let previous = history(&directory, &first, "first.json");
        let second = apply(&engine, &plan(&engine, &directory, Some(&previous)));
        assert_eq!(
            second
                .entries
                .iter()
                .find(|entry| entry.source_record_id == "a.md")
                .unwrap()
                .prior_write,
            first.entries[0].prior_write
        );
        assert!(
            second
                .entries
                .iter()
                .all(|entry| entry.verification.is_none())
        );
        let previous = history(&directory, &second, "second.json");
        fs::write(
            directory.path().join("source/a.md"),
            "Changed existing body.\n",
        )
        .unwrap();
        let third = apply(&engine, &plan(&engine, &directory, Some(&previous)));
        let prior = third
            .entries
            .iter()
            .find(|entry| entry.source_record_id == "a.md")
            .unwrap()
            .prior_write
            .as_ref()
            .unwrap();
        if writer == "okf" {
            fs::remove_file(
                directory
                    .path()
                    .join("target")
                    .join(format!("{}.md", prior.target_id)),
            )
            .unwrap();
        } else {
            let path = directory.path().join("target/records.ump.json");
            let mut records: Vec<serde_json::Value> =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            records.retain(|record| record["id"] != prior.target_id);
            fs::write(path, serde_json::to_vec(&records).unwrap()).unwrap();
        }
        let previous = history(&directory, &third, "third.json");
        fs::write(
            directory.path().join("source/a.md"),
            "Another source change.\n",
        )
        .unwrap();
        let report = plan(&engine, &directory, Some(&previous));
        assert_eq!(
            report
                .entries
                .iter()
                .find(|entry| entry.source_record_id == "a.md")
                .unwrap()
                .disposition,
            Disposition::Omitted {
                reason: OmissionReason::DeletedInTarget
            }
        );
        apply(&engine, &report);
        assert!(
            engine
                .registry
                .writer("home")
                .unwrap()
                .inspect(&prior.target_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn okf_skips_unmanaged_base32_named_notes_and_preflights_damaged_managed_notes() {
    for (body, managed) in [
        ("Unrelated Markdown.\n", false),
        (
            "---\ntype: Memory\ntitle: Unrelated\n---\nUnrelated body.\n",
            false,
        ),
        (
            "---\ntype: Memory\nmem_adaptor_envelope: okf:0.2\nmem_adaptor: {}\n---\nBroken managed record.\n",
            true,
        ),
    ] {
        let directory = fixture();
        fs::write(
            directory.path().join("source/note.md"),
            "Synthetic new body.\n",
        )
        .unwrap();
        fs::create_dir_all(directory.path().join("target/memories")).unwrap();
        let path = directory
            .path()
            .join("target/memories/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md");
        fs::write(&path, body).unwrap();
        let engine = engine(&directory, "okf");
        let report = engine.plan(&directory.path().join("source"), policy(GateAction::Pass));
        if managed {
            assert!(report.is_err());
            assert_eq!(
                fs::read_dir(directory.path().join("target/memories"))
                    .unwrap()
                    .count(),
                1
            );
            assert!(!directory.path().join("target/index.md").exists());
        } else {
            let report = report.unwrap();
            assert!(!report.targets[0].artifacts.iter().any(|artifact| {
                artifact
                    .path
                    .ends_with("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.md")
            }));
            apply(&engine, &report);
            assert!(
                !fs::read_to_string(directory.path().join("target/index.md"))
                    .unwrap()
                    .contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            );
        }
        assert_eq!(fs::read_to_string(path).unwrap(), body);
    }
}

#[test]
fn alias_references_follow_the_final_representative_without_erasing_own_history() {
    for writer in ["okf", "ump"] {
        let directory = fixture();
        fs::write(directory.path().join("source/a.md"), "Old A content.\n").unwrap();
        fs::write(directory.path().join("source/b.md"), "Stable B content.\n").unwrap();
        let engine = engine(&directory, writer);
        let first = apply(&engine, &plan(&engine, &directory, None));
        let own_a = first
            .entries
            .iter()
            .find(|entry| entry.source_record_id == "a.md")
            .unwrap()
            .prior_write
            .clone();
        fs::write(directory.path().join("source/c.md"), "Old A content.\n").unwrap();
        let previous = history(&directory, &first, "first.json");
        let second = apply(&engine, &plan(&engine, &directory, Some(&previous)));
        for name in ["a.md", "c.md"] {
            fs::write(
                directory.path().join("source").join(name),
                "Stable B content.\n",
            )
            .unwrap();
        }
        let previous = history(&directory, &second, "second.json");
        let third = apply(&engine, &plan(&engine, &directory, Some(&previous)));
        let representative = canonical_id("markdown", "b.md");
        for name in ["a.md", "c.md"] {
            let entry = third
                .entries
                .iter()
                .find(|entry| entry.source_record_id == name)
                .unwrap();
            assert_eq!(
                entry.duplicate_write.as_ref().unwrap().canonical_id,
                representative
            );
            let actual = engine
                .registry
                .writer("home")
                .unwrap()
                .inspect(
                    &entry
                        .duplicate_write
                        .as_ref()
                        .unwrap()
                        .prior_write
                        .target_id,
                )
                .unwrap()
                .unwrap();
            assert_eq!(actual.content, "Stable B content.\n");
        }
        assert_eq!(
            third
                .entries
                .iter()
                .find(|entry| entry.source_record_id == "a.md")
                .unwrap()
                .prior_write,
            own_a
        );
        assert!(
            third
                .entries
                .iter()
                .all(|entry| entry.verification.is_none())
        );
        fs::write(
            directory.path().join("source/a.md"),
            "Independent A update.\n",
        )
        .unwrap();
        let previous = history(&directory, &third, "third.json");
        let fourth = apply(&engine, &plan(&engine, &directory, Some(&previous)));
        assert_eq!(
            fourth
                .entries
                .iter()
                .find(|entry| entry.source_record_id == "a.md")
                .unwrap()
                .verification,
            Some(Verification::Verified)
        );
    }
}

/// Refuses unsafe foreign JSON numbers with the intended JCS cause and preserves all native bytes.
#[test]
fn unsafe_integers_in_existing_ump_records_are_rejected_before_update() {
    let directory = fixture();
    fs::write(
        directory.path().join("source/note.md"),
        "New synthetic content.\n",
    )
    .unwrap();
    fs::create_dir(directory.path().join("target")).unwrap();
    let native = serde_json::json!([{
        "ump": "1.0", "id": "urn:ump:foreignrecord", "kind": "semantic",
        "body": {"text": "Synthetic existing memory.", "structured": {"count": 9007199254740993_u64}},
        "scope": {"owner": "foreign-owner", "visibility": "private"},
        "time": {"created": "2026-01-01T00:00:00Z"}
    }]);
    let path = directory.path().join("target/records.ump.json");
    let bytes = serde_json::to_vec(&native).unwrap();
    fs::write(&path, &bytes).unwrap();
    let error = engine(&directory, "ump")
        .plan(&directory.path().join("source"), policy(GateAction::Pass))
        .unwrap_err();
    assert!(format!("{error:#}").contains("JCS safe range"));
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(directory.path().join("target"))
            .unwrap()
            .count(),
        1
    );
}
