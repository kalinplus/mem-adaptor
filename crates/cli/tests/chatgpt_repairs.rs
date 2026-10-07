//! Exercises ChatGPT source boundaries before engine planning using synthetic in-memory inventories and temporary files.
//! Independently fixed identity vectors protect normalized Prompt IDs and no-ID fallbacks; CLI tests inspect actual files.
//! No real memory, remote calls, schema changes, or persistent Reader state are involved.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use mem_adaptor_core::canonical::{DnaClass, EvidenceLevel};
use mem_adaptor_core::plugins::{FileInventory, Reader, ReaderOutput, SourceFs};
use mem_adaptor_core::reports::PlanReport;
use mem_adaptor_reader_chatgpt::ChatgptReader;
use serde_json::{Value, json};
use tempfile::TempDir;

// Independent vectors: "sha256:" + hashlib.sha256(identity).hexdigest(), then lower-case Base32 of
// hashlib.sha256(b"chatgpt\0" + source_id.encode()).digest()[:20], with no padding.
const PROMPT_ID: &str = "sha256:578e4926b8fbdd607170a09c7e7f4a160dbf31ac1f59a5a0fd7f8c72d8ee37e4";
const PROMPT_CANONICAL_ID: &str = "jwat6vpradaw5ader6apwe5743ldzcjp";
const PRIVATE: &str = "synthetic-chatgpt-private-marker";

/// Builds one collection from inline synthetic file bytes without opening user data.
fn source(entries: &[(&str, &str)]) -> SourceFs {
    SourceFs {
        root: "/synthetic/chatgpt".into(),
        files: entries
            .iter()
            .map(|(path, text)| ((*path).into(), text.as_bytes().to_vec()))
            .collect(),
    }
}

/// Reads one real claim selected by path; absence, ambiguity, or unexpected parse failures fail test setup.
fn read_path(source: &SourceFs, path: &str) -> ReaderOutput {
    let claims = ChatgptReader.claim(&source.files);
    let claim = claims.iter().find(|claim| claim.path == path).unwrap();
    ChatgptReader.read(claim, source).unwrap()
}

/// Copies synthetic source files and creates protected target sentinels in an isolated temporary directory.
fn sandbox(entries: &[(&str, &str)]) -> TempDir {
    let directory = TempDir::new().unwrap();
    fs::create_dir(directory.path().join("source")).unwrap();
    for (path, text) in entries {
        let path = directory.path().join("source").join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fs::create_dir_all(directory.path().join("target/nested")).unwrap();
    fs::write(directory.path().join("target/sentinel.txt"), b"keep me\n").unwrap();
    fs::write(directory.path().join("target/nested/binary"), [0, 255, 17]).unwrap();
    directory
}

/// Snapshots all synthetic file paths and bytes, including nested sentinels and any newly created report artifacts.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    /// Walks only test-created directories, preserving paths relative to the sandbox root.
    fn visit(root: &Path, current: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).unwrap().to_str().unwrap().into(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

/// Runs the actual CLI's dry-run plan against temporary input and existing target files.
fn cli_plan(directory: &TempDir) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
        .arg("plan")
        .arg(directory.path().join("source"))
        .arg("--to")
        .arg(format!("okf:{}", directory.path().join("target").display()))
        .arg("--report")
        .arg(directory.path().join("plan.json"))
        .output()
        .unwrap()
}

/// Verifies ordinary planning failure, value-free diagnostics, unchanged full file inventory, and absent credentials.
fn assert_plan_refused(entries: &[(&str, &str)], reason: &str) {
    let directory = sandbox(entries);
    let before = snapshot(directory.path());
    let result = cli_plan(&directory);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert_eq!(result.status.code(), Some(1));
    assert!(
        stderr.contains(reason),
        "missing expected safe failure reason"
    );
    assert!(stderr.contains("Planning failed before target writes"));
    assert!(stderr.contains("target unchanged"));
    assert!(stderr.contains("Check the source"));
    assert!(!stderr.contains(PRIVATE));
    assert!(!stdout.contains(PRIVATE));
    assert!(!stdout.contains("Plan:"));
    assert_eq!(snapshot(directory.path()), before);
    assert!(!directory.path().join("plan.json").exists());
    assert!(!directory.path().join("plan.approval.json").exists());
    assert!(!directory.path().join("plan.receipt.json").exists());
}

/// Known source names remain claimed when malformed, and a good neighboring Prompt cannot hide their failure.
#[test]
fn known_corrupt_json_fails_planning_without_target_or_report_changes() {
    for (name, layer) in [
        ("memory.json", "saved_memory"),
        ("saved_memories.json", "saved_memory"),
        ("memories.chatgpt.json", "saved_memory"),
        ("user.json", "instruction"),
    ] {
        let text = r#"{"password":"synthetic-chatgpt-private-marker", broken}"#;
        let entries = [
            (name, text),
            (
                "good.chatgpt.md",
                "[unknown] [tool] Synthetic good memory.\n",
            ),
        ];
        let source = source(&entries);
        let claims = ChatgptReader.claim(&source.files);
        let claim = claims.iter().find(|claim| claim.path == name).unwrap();
        assert_eq!(claim.layer, layer);
        let error = ChatgptReader.read(claim, &source).err().unwrap();
        assert!(format!("{error:#}").contains("Invalid ChatGPT JSON at line"));
        assert!(!format!("{error:#}").contains(PRIVATE));
        assert_plan_refused(&entries, "Invalid ChatGPT JSON at line");
    }
}

/// Explicit names also reject wrong outer JSON types rather than treating a valid JSON scalar as an empty export.
#[test]
fn known_names_reject_incompatible_outer_shapes() {
    for name in [
        "memory.json",
        "saved_memories.json",
        "memories.chatgpt.json",
    ] {
        for text in [
            "null",
            "17",
            r#""synthetic-chatgpt-private-marker""#,
            r#"{"memory":{"private":"synthetic-chatgpt-private-marker"}}"#,
        ] {
            assert_plan_refused(
                &[(name, text)],
                "ChatGPT memory must be an array or an object containing memory",
            );
        }
    }
    for text in ["null", "17", r#"["synthetic-chatgpt-private-marker"]"#] {
        assert_plan_refused(
            &[("user.json", text)],
            "ChatGPT user metadata must be an object",
        );
    }
}

/// Invalid UTF-8 cannot be skipped while scanning earlier Prompt identities, and source-path secrets stay masked.
#[test]
fn invalid_utf8_prompt_fails_without_partial_plan_or_leaked_path() {
    let path = "a/password=synthetic-chatgpt-private-marker.chatgpt.md";
    let mut collection = source(&[("z.chatgpt.md", "[unknown] [tool] Same text.\n")]);
    collection.files.insert(path.into(), vec![255, 254]);
    let claims = ChatgptReader.claim(&collection.files);
    let claim = claims
        .iter()
        .find(|claim| claim.path == "z.chatgpt.md")
        .unwrap();
    let error = ChatgptReader.read(claim, &collection).err().unwrap();
    assert!(format!("{error:#}").contains("Earlier ChatGPT prompt export is not UTF-8"));
    assert!(format!("{error:#}").contains("a/password=[secret]"));
    assert!(!format!("{error:#}").contains(PRIVATE));

    let directory = sandbox(&[("z.chatgpt.md", "[unknown] [tool] Same text.\n")]);
    fs::create_dir(directory.path().join("source/a")).unwrap();
    // Use a nonsensitive CLI path to reach parsing rather than the engine's earlier sensitive-filename refusal.
    fs::write(
        directory.path().join("source/a/corrupt.chatgpt.md"),
        [255, 254],
    )
    .unwrap();
    let before = snapshot(directory.path());
    let result = cli_plan(&directory);
    assert_eq!(result.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("ChatGPT prompt export is not UTF-8"));
    assert!(stderr.contains("Planning failed before target writes"));
    assert!(!stderr.contains(PRIVATE));
    assert!(!String::from_utf8_lossy(&result.stdout).contains(PRIVATE));
    assert_eq!(snapshot(directory.path()), before);
    assert!(!directory.path().join("plan.json").exists());
}

/// Every approved sibling alias identifies empty or corrupt shared files only at the exact same parent.
#[test]
fn shared_names_use_all_same_parent_evidence_aliases() {
    for (evidence, text) in [
        ("user.json", "{}"),
        ("memory.json", "[]"),
        ("saved_memories.json", "[]"),
        ("memories.chatgpt.json", "[]"),
        ("extract.chatgpt.md", "[unknown] [tool] Evidence.\n"),
    ] {
        for shared in ["memories.json", "conversations.json"] {
            let evidence_path = format!("export/{evidence}");
            let shared_path = format!("export/{shared}");
            let collection = source(&[(&evidence_path, text), (&shared_path, "[]")]);
            let claims = ChatgptReader.claim(&collection.files);
            let claim = claims
                .iter()
                .find(|claim| claim.path == shared_path)
                .unwrap();
            assert_eq!(claim.registered_only, shared == "conversations.json");
            let output = ChatgptReader.read(claim, &collection).unwrap();
            assert!(output.records.is_empty());
            assert_eq!(output.registered_count, 0);

            let collection = source(&[
                (&evidence_path, text),
                (
                    &shared_path,
                    r#"{"password":"synthetic-chatgpt-private-marker", broken}"#,
                ),
            ]);
            let claims = ChatgptReader.claim(&collection.files);
            let claim = claims
                .iter()
                .find(|claim| claim.path == shared_path)
                .unwrap();
            let error = ChatgptReader.read(claim, &collection).err().unwrap();
            assert!(format!("{error:#}").contains("Invalid ChatGPT JSON at line"));
            assert!(!format!("{error:#}").contains(PRIVATE));
        }
    }
    for memory in [
        r#"{"memory":[]}"#,
        r#"[{"content":"Recognizable generic memory."}]"#,
    ] {
        let collection = source(&[
            ("export/memories.json", memory),
            ("export/conversations.json", "[]"),
        ]);
        assert_eq!(ChatgptReader.claim(&collection.files).len(), 2);
        assert_eq!(
            read_path(&collection, "export/conversations.json").registered_count,
            0
        );
    }
}

/// Parent, descendant, prefix-sharing sibling, and unrelated source markers must not claim ambiguous files.
#[test]
fn source_evidence_never_crosses_directory_boundaries() {
    for marker in [
        "extract.chatgpt.md",
        "export/child/extract.chatgpt.md",
        "export-other/extract.chatgpt.md",
        "elsewhere/extract.chatgpt.md",
    ] {
        let collection = source(&[
            (marker, "[unknown] [tool] Evidence.\n"),
            ("export/conversations.json", "[]"),
            ("export/memories.json", "[]"),
        ]);
        let claims = ChatgptReader.claim(&collection.files);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].path, marker);
    }
    for marker in [
        "child/user.json",
        "child/memory.json",
        "child/saved_memories.json",
        "child/memories.chatgpt.json",
        "child/memories.json",
    ] {
        let collection = source(&[
            (marker, r#"{"memory":[]}"#),
            ("conversations.json", "[]"),
            ("memories.json", "[]"),
        ]);
        assert!(
            ChatgptReader
                .claim(&collection.files)
                .iter()
                .all(|claim| claim.path == marker)
        );
    }
    assert!(
        ChatgptReader
            .claim(&FileInventory::from([
                ("conversations.json".into(), b"[]".to_vec()),
                ("memories.json".into(), b"[]".to_vec()),
            ]))
            .is_empty()
    );
}

/// Recognizable shared shapes need no marker, but malformed and incompatible shared shapes fail with own evidence.
#[test]
fn shared_shapes_are_validated_before_registration_or_memory_success() {
    let collection = source(&[
        (
            "a/conversations.json",
            r#"[{"mapping":{}},{"mapping":{"node":{}}}]"#,
        ),
        ("b/memories.json", r#"[{"content":"Synthetic memory."}]"#),
    ]);
    assert_eq!(ChatgptReader.claim(&collection.files).len(), 2);
    assert_eq!(
        read_path(&collection, "a/conversations.json").registered_count,
        2
    );
    assert_eq!(read_path(&collection, "b/memories.json").records.len(), 1);

    for (path, bad, reason) in [
        (
            "conversations.json",
            r#"{"password":"synthetic-chatgpt-private-marker", broken}"#,
            "Invalid ChatGPT JSON at line",
        ),
        (
            "conversations.json",
            r#"{"password":"synthetic-chatgpt-private-marker"}"#,
            "ChatGPT conversations must be an array",
        ),
        (
            "conversations.json",
            r#"[{"messages":"synthetic-chatgpt-private-marker"}]"#,
            "must contain a mapping object",
        ),
        (
            "conversations.json",
            r#"[{"mapping":{}},{"messages":"synthetic-chatgpt-private-marker"}]"#,
            "at index 1 must contain a mapping object",
        ),
        (
            "conversations.json",
            r#"[{"mapping":"synthetic-chatgpt-private-marker"}]"#,
            "must contain a mapping object",
        ),
        (
            "memories.json",
            r#"{"password":"synthetic-chatgpt-private-marker", broken}"#,
            "Invalid ChatGPT JSON at line",
        ),
        (
            "memories.json",
            r#"{"password":"synthetic-chatgpt-private-marker"}"#,
            "ChatGPT memory must be an array",
        ),
        (
            "memories.json",
            r#"{"memory":"synthetic-chatgpt-private-marker"}"#,
            "ChatGPT memory must be an array",
        ),
        (
            "memories.json",
            r#"[{"account_uuid":"synthetic-chatgpt-private-marker"}]"#,
            "ChatGPT memory entries have an incompatible shape",
        ),
        (
            "memories.json",
            r#"[17,"synthetic-chatgpt-private-marker"]"#,
            "ChatGPT memory entries have an incompatible shape",
        ),
    ] {
        let entries = [
            (
                "own/memory.json",
                r#"[{"id":"good","content":"Good memory."}]"#,
            ),
            (
                "own/good.chatgpt.md",
                "[unknown] [tool] Another good memory.\n",
            ),
        ];
        let bad_path = format!("own/{path}");
        let mut mixed = entries.to_vec();
        mixed.push((&bad_path, bad));
        let collection = source(&mixed);
        let claims = ChatgptReader.claim(&collection.files);
        let claim = claims.iter().find(|claim| claim.path == bad_path).unwrap();
        let error = ChatgptReader.read(claim, &collection).err().unwrap();
        assert!(format!("{error:#}").contains(reason));
        assert!(!format!("{error:#}").contains(PRIVATE));
        assert_plan_refused(&mixed, reason);
    }
}

/// Corrupt generic memories cannot be hidden by a differently named valid saved-memory source.
#[test]
fn aliases_make_corrupt_generic_memory_fail_in_mixed_exports() {
    for (alias, evidence) in [
        ("user.json", "{}"),
        (
            "saved_memories.json",
            r#"[{"content":"Good alias memory."}]"#,
        ),
        (
            "memories.chatgpt.json",
            r#"[{"content":"Good alias memory."}]"#,
        ),
        (
            "extract.chatgpt.md",
            "[unknown] [tool] Good alias memory.\n",
        ),
    ] {
        for shared in ["memories.json", "conversations.json"] {
            assert_plan_refused(
                &[
                    (alias, evidence),
                    (
                        shared,
                        r#"{"password":"synthetic-chatgpt-private-marker", broken}"#,
                    ),
                ],
                "Invalid ChatGPT JSON at line",
            );
        }
    }
}

/// Envelope metadata is reported by escaped leaf paths even with zero entries, never copied into each record or report.
#[test]
fn global_envelope_fields_are_explicitly_uncarried_without_values() {
    for memory in [
        json!([]),
        json!([{"id":"one","content":"First."},{"id":"two","content":"Second."}]),
    ] {
        let data = json!({
            "memory": memory,
            "global": {"a/b": {"~leaf": PRIVATE}, "empty": {}},
            "global_array": [PRIVATE, {"private": PRIVATE}],
            "empty_array": [],
            "nullable": null
        });
        let text = data.to_string();
        let collection = source(&[("memory.json", &text)]);
        let output = read_path(&collection, "memory.json");
        assert_eq!(
            output.records.len(),
            data["memory"].as_array().unwrap().len()
        );
        assert!(
            output
                .records
                .iter()
                .all(|record| record.source_extra.is_none())
        );
        assert!(
            output
                .source_records
                .iter()
                .all(|record| record.fields.get("global").is_none())
        );
        let mut diagnostics: Vec<_> = output
            .anomalies
            .iter()
            .map(|anomaly| {
                assert_eq!(anomaly.source_locator, "memory.json");
                assert_eq!(anomaly.code, "uncarried_export_field");
                assert_eq!(anomaly.line, None);
                anomaly.field_path.as_deref().unwrap()
            })
            .collect();
        diagnostics.sort_unstable();
        assert_eq!(
            diagnostics,
            [
                "/empty_array",
                "/global/a~1b/~0leaf",
                "/global/empty",
                "/global_array",
                "/nullable"
            ]
        );
        assert!(
            !serde_json::to_string(&output.anomalies)
                .unwrap()
                .contains(PRIVATE)
        );

        let directory = sandbox(&[("memory.json", &text)]);
        let before_target = snapshot(&directory.path().join("target"));
        let result = cli_plan(&directory);
        assert!(result.status.success());
        assert!(!String::from_utf8_lossy(&result.stdout).contains(PRIVATE));
        assert!(!String::from_utf8_lossy(&result.stderr).contains(PRIVATE));
        assert_eq!(snapshot(&directory.path().join("target")), before_target);
        let bytes = fs::read(directory.path().join("plan.json")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(PRIVATE));
        let report: PlanReport = serde_json::from_slice(&bytes).unwrap();
        mem_adaptor_core::schema::validate("plan-report", &report).unwrap();
        assert_eq!(report.anomalies, output.anomalies);
        assert!(report.model_calls.is_empty());
        assert_eq!(report.entries.len(), output.records.len());
    }
}

/// Cross-file duplicates choose the first path and line, regardless of claim read order, while preserving distinct identities.
#[test]
fn prompt_dedup_uses_collection_wide_sorted_paths_and_reports_every_discarded_line() {
    let collection = source(&[
        (
            "z.chatgpt.md",
            "[ unknown ] [ tool ] Same\ttext.\n[unknown] [tool] Same text.\n[unknown] [tool] Changed text.\n",
        ),
        (
            "a.chatgpt.md",
            "```text\n[unknown] [tool] Same  text.\n[unknown] [tool] Same text.\n[2026-02-28] [preference] Same text.\n[unknown] [preference] Same text.\n```\n",
        ),
        ("middle/second.chatgpt.md", "[unknown] [tool] Same text.\n"),
        ("not-marked.md", "[unknown] [tool] Changed text.\n"),
    ]);
    // Reading the final claim first must not change which source location represents the identity.
    let last = read_path(&collection, "z.chatgpt.md");
    let first = read_path(&collection, "a.chatgpt.md");
    let middle = read_path(&collection, "middle/second.chatgpt.md");
    assert_eq!(first.records.len(), 3);
    assert_eq!(last.records.len(), 1);
    assert!(middle.records.is_empty());
    assert_eq!(first.records[0].source_record_id, PROMPT_ID);
    assert_eq!(first.records[0].canonical_id, PROMPT_CANONICAL_ID);
    assert_eq!(first.records[0].source_locator, "a.chatgpt.md:2");
    assert_eq!(first.records[0].content, "Same  text.");
    assert_eq!(
        first.records[1].source_record_id,
        "sha256:98dd62c9f34213a31e7e5968762d471fa58d46043308a529eea0773037ac5042"
    );
    assert_eq!(
        first.records[1].canonical_id,
        "33623bvpq62mebyheiifzvww22c32gwq"
    );
    assert_eq!(
        first.records[2].source_record_id,
        "sha256:77b2b85d5cc8abc18a0de50d6d930590427f2f1ece02616d172721b07b37b001"
    );
    assert_eq!(
        first.records[2].canonical_id,
        "2x5dkjn5r3vunq6e6evprp75y6uowgyt"
    );
    assert_eq!(
        last.records[0].source_record_id,
        "sha256:4fdd213c034175cea96d27986d43924feb4413491d6b88f59467a40ab6262d92"
    );
    assert_eq!(
        last.records[0].canonical_id,
        "h3476yvm4xyavhk7pz4nvlwjjqrc7dub"
    );
    assert_eq!(last.records[0].source_locator, "z.chatgpt.md:3");
    for (output, path, lines) in [
        (&first, "a.chatgpt.md", vec![3]),
        (&middle, "middle/second.chatgpt.md", vec![1]),
        (&last, "z.chatgpt.md", vec![1, 2]),
    ] {
        assert_eq!(output.anomalies.len(), lines.len());
        for (anomaly, line) in output.anomalies.iter().zip(lines) {
            assert_eq!(anomaly.code, "duplicate_prompt_line");
            assert_eq!(anomaly.source_locator, path);
            assert_eq!(anomaly.line, Some(line));
        }
    }
    // A fresh collection with the same Reader must not inherit previous identities.
    let fresh = source(&[("z.chatgpt.md", "[unknown] [tool] Same text.\n")]);
    let output = read_path(&fresh, "z.chatgpt.md");
    assert_eq!(output.records.len(), 1);
    assert_eq!(output.records[0].source_record_id, PROMPT_ID);
    assert_eq!(output.records[0].source_locator, "z.chatgpt.md:1");
    assert!(output.anomalies.is_empty());

    let entries: Vec<_> = collection
        .files
        .iter()
        .map(|(path, bytes)| (path.as_str(), std::str::from_utf8(bytes).unwrap()))
        .filter(|(path, _)| path.ends_with(".chatgpt.md"))
        .collect();
    let directory = sandbox(&entries);
    let before = snapshot(&directory.path().join("target"));
    let result = cli_plan(&directory);
    assert!(result.status.success());
    let report: PlanReport =
        serde_json::from_slice(&fs::read(directory.path().join("plan.json")).unwrap()).unwrap();
    assert_eq!(report.entries.len(), 4);
    assert_eq!(report.anomalies.len(), 4);
    assert_eq!(snapshot(&directory.path().join("target")), before);
    assert_eq!(
        report
            .entries
            .iter()
            .find(|entry| entry.source_record_id == PROMPT_ID)
            .unwrap()
            .source_locator,
        "a.chatgpt.md:2"
    );
}

/// Bad Prompt lines do not reserve identities; retained invalid-date/unknown-kind lines deduplicate without leaking values.
#[test]
fn prompt_dedup_reserves_only_parseable_lines_and_preserves_line_diagnostics() {
    let collection = source(&[
        (
            "a.chatgpt.md",
            "bad synthetic-chatgpt-private-marker\n[unknown] [tool]\n```\n",
        ),
        (
            "b.chatgpt.md",
            "[unknown] [tool] Same text.\n[2026-02-30] [other] Kept.\n",
        ),
        (
            "c.chatgpt.md",
            "[2026-02-30] [other] Kept.\n[unknown] [tool] Same text.\n",
        ),
    ]);
    let first = read_path(&collection, "a.chatgpt.md");
    assert!(first.records.is_empty());
    assert_eq!(
        first
            .anomalies
            .iter()
            .map(|anomaly| (anomaly.code.as_str(), anomaly.line))
            .collect::<Vec<_>>(),
        [
            ("invalid_prompt_line", Some(1)),
            ("invalid_prompt_line", Some(2))
        ]
    );
    let second = read_path(&collection, "b.chatgpt.md");
    assert_eq!(second.records.len(), 2);
    assert_eq!(second.records[0].source_record_id, PROMPT_ID);
    assert_eq!(second.records[1].source_kind.as_deref(), Some("other"));
    assert_eq!(
        second.records[1].source_extra.as_ref().unwrap()["date"],
        "2026-02-30"
    );
    assert_eq!(second.records[1].evidence_level, EvidenceLevel::Inferred);
    assert_eq!(
        second
            .anomalies
            .iter()
            .map(|anomaly| (anomaly.code.as_str(), anomaly.line))
            .collect::<Vec<_>>(),
        [
            ("unknown_source_kind", Some(2)),
            ("invalid_prompt_date", Some(2))
        ]
    );
    let last = read_path(&collection, "c.chatgpt.md");
    assert!(last.records.is_empty());
    assert_eq!(
        last.anomalies
            .iter()
            .map(|anomaly| (anomaly.code.as_str(), anomaly.line))
            .collect::<Vec<_>>(),
        [
            ("duplicate_prompt_line", Some(1)),
            ("duplicate_prompt_line", Some(2))
        ]
    );
    assert!(
        !serde_json::to_string(&first.anomalies)
            .unwrap()
            .contains(PRIVATE)
    );
}

/// Per-entry metadata and explicit IDs pass through independently of envelope losses; no-ID identity changes with body bytes.
#[test]
fn saved_fields_and_no_id_fallbacks_remain_record_specific() {
    let data = json!({
        "memory": [
            {"id":"original-id","content":"Original body.","text":"Original body.","kind":"preference","type":"project",
             "created_at":"2026-01-02T03:04:05Z","updated_at":1767315845,
             "enabled":false,"extra":{"a/b":[true,2,null],"empty":{}}},
            {"content":"Synthetic no-ID body.","type":"tool","entry_only":{"flag":true}},
            {"content":"Synthetic changed body.","unknown":17},
            {"deleted":true,"content":"Deleted private body."},
            {"id":"conflict","content":"First conflicting body.","text":"Second conflicting body."}
        ],
        "global_only":PRIVATE
    });
    let text = data.to_string();
    let collection = source(&[("memory.json", &text)]);
    let output = read_path(&collection, "memory.json");
    assert_eq!(output.records.len(), 3);
    assert_eq!(output.deleted_count, 1);
    assert_eq!(
        output
            .anomalies
            .iter()
            .map(|anomaly| (anomaly.code.as_str(), anomaly.source_locator.as_str()))
            .collect::<Vec<_>>(),
        [
            ("uncarried_export_field", "memory.json"),
            ("conflicting_memory_content", "memory.json#/memory/4")
        ]
    );
    let original = &output.records[0];
    assert_eq!(original.source_record_id, "original-id");
    assert_eq!(original.canonical_id, "fdzepbuinq5tli6qqeumiqyo3ubtynvb");
    assert_eq!(original.content, "Original body.");
    assert_eq!(original.source_kind.as_deref(), Some("preference"));
    assert_eq!(original.dna_class, DnaClass::Dna);
    assert_eq!(original.created_at.as_deref(), Some("2026-01-02T03:04:05Z"));
    assert_eq!(original.updated_at.as_deref(), Some("2026-01-02T01:04:05Z"));
    assert_eq!(
        original.consent.as_ref().unwrap().memory_enabled,
        Some(false)
    );
    assert_eq!(
        original.source_extra,
        Some(
            serde_json::from_value(json!({
                "type":"project","extra":{"a/b":[true,2,null],"empty":{}}
            }))
            .unwrap()
        )
    );
    assert_eq!(output.source_records[0].fields, data["memory"][0]);
    for (path, canonical) in [
        ("/content", "/content"),
        ("/text", "/content"),
        ("/id", "/source_record_id"),
        ("/kind", "/source_kind"),
        ("/enabled", "/consent/memory_enabled"),
        ("/extra/a~1b", "/source_extra/extra/a~1b"),
        ("/extra/empty", "/source_extra/extra/empty"),
    ] {
        assert!(
            output.source_records[0]
                .field_map
                .iter()
                .any(|mapping| mapping.source_path == path && mapping.canonical_path == canonical)
        );
    }
    assert_eq!(
        output.records[1].source_record_id,
        "sha256:e1f493b562ad2a48a1051ece0e5079f634259a80698f5e05e42c05114939b40c"
    );
    assert_eq!(
        output.records[1].canonical_id,
        "thzn75o2utflhqkiu4ygavfbsgqxzxd3"
    );
    assert_eq!(output.records[1].source_kind.as_deref(), Some("tool"));
    assert_eq!(
        output.records[1].source_extra,
        Some(serde_json::from_value(json!({"entry_only":{"flag":true}})).unwrap())
    );
    assert_eq!(
        output.records[2].source_record_id,
        "sha256:d5d1bdfe68a7a963666f7a6556cae3ce60f942ddf30886700d0c4339f96a2c14"
    );
    assert_eq!(
        output.records[2].canonical_id,
        "bcvdwxclmlyme3ecaqmcipwjif7ax4uf"
    );
    assert_eq!(output.records[2].evidence_level, EvidenceLevel::Inferred);
    assert_eq!(
        output.records[2].source_extra,
        Some(serde_json::from_value(json!({"unknown":17})).unwrap())
    );
    assert!(output.records.iter().all(|record| {
        record
            .source_extra
            .as_ref()
            .unwrap()
            .get("global_only")
            .is_none()
    }));
}

/// Present nonstring kind values fail instead of falling through to a valid type; absent kind still uses explicit type.
#[test]
fn invalid_present_kind_is_not_hidden_by_type() {
    for value in [
        Value::Null,
        json!(17),
        json!(false),
        json!([]),
        json!({"private":PRIVATE}),
    ] {
        let text =
            json!([{"content":"Synthetic body.","kind":value,"type":"preference"}]).to_string();
        let collection = source(&[("memory.json", &text)]);
        let claim = ChatgptReader.claim(&collection.files).pop().unwrap();
        let error = ChatgptReader.read(&claim, &collection).err().unwrap();
        assert!(format!("{error:#}").contains("kind"));
        assert!(!format!("{error:#}").contains(PRIVATE));
        assert_plan_refused(&[("memory.json", &text)], "kind");
    }
    let collection = source(&[(
        "memory.json",
        r#"[{"content":"Synthetic body.","type":"preference"}]"#,
    )]);
    let output = read_path(&collection, "memory.json");
    assert_eq!(output.records[0].source_kind.as_deref(), Some("preference"));
    assert_eq!(output.records[0].dna_class, DnaClass::Dna);
    assert!(
        output.source_records[0]
            .field_map
            .iter()
            .any(|mapping| mapping.source_path == "/type"
                && mapping.canonical_path == "/source_kind")
    );
}
