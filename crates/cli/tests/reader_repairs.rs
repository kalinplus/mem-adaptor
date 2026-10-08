//! Regression tests for recognized-input refusal, shared Markdown parsing, and truthful field preservation.
//! All inputs and native targets are synthetic; snapshots inspect actual bytes independently of Writer read-back.
//! These tests do not choose a home-edit or satellite identity policy.

use std::{collections::BTreeMap, fs, path::Path, process::Command};

use mem_adaptor_core::{
    canonical::{DnaClass, EvidenceLevel},
    engine::Engine,
    governance::{GateAction, GatePolicy, PolicyOrigin},
    plugins::{Reader, ReaderOutput, Registry, SourceFs},
    reader as normalize,
};
use mem_adaptor_reader_chatgpt::ChatgptReader;
use mem_adaptor_reader_claude::ClaudeReader;
use mem_adaptor_reader_markdown::MarkdownReader;
use serde_json::{Value, json};
use tempfile::TempDir;

mod common;

/// Builds an isolated in-memory source, optionally including binary invalid UTF-8 input.
fn source(entries: &[(&str, &[u8])]) -> SourceFs {
    SourceFs {
        root: "/synthetic/private-root".into(),
        files: entries
            .iter()
            .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
            .collect(),
        satellite_id: None,
    }
}

/// Reads every claimed file without bypassing adapter recognition.
fn read(reader: &dyn Reader, source: &SourceFs) -> mem_adaptor_core::Result<Vec<ReaderOutput>> {
    reader
        .claim(&source.files)
        .iter()
        .map(|claim| reader.read(claim, source))
        .collect()
}

/// Recursively snapshots names and bytes to catch unexpected files as well as overwritten user files.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    /// Visits only the isolated test tree and records relative names.
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), result);
            } else {
                result.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    if root.exists() {
        visit(root, root, &mut result);
    }
    result
}

/// Verifies empty/comment-only closed metadata and unclosed openings preserve precisely the promised body bytes.
#[test]
fn shared_frontmatter_preserves_empty_comments_and_unclosed_bodies() {
    for reader in [&MarkdownReader as &dyn Reader, &ClaudeReader] {
        for (text, expected, unclosed) in [
            ("---\n---\nBody.\r\n", "Body.\r\n", false),
            (
                "---\r\n# comment\r\n\r\n---\r\nBody.\r\n",
                "Body.\r\n",
                false,
            ),
            (
                "---\ntype: profile\nBody.\r\n",
                "---\ntype: profile\nBody.\r\n",
                true,
            ),
        ] {
            let document = if reader.id() == "markdown" {
                text.to_string()
            } else {
                json!([{"account_uuid":"synthetic","memory_files":[{"path":"/note.md","content":text}]}]).to_string()
            };
            let path = if reader.id() == "markdown" {
                "note.md"
            } else {
                "memories.json"
            };
            let output = read(reader, &source(&[(path, document.as_bytes())])).unwrap();
            assert_eq!(output[0].records.len(), 1);
            // Claude's native memory-file content includes the complete frontmatter, unlike local Markdown.
            assert_eq!(
                output[0].records[0].content,
                if reader.id() == "claude" {
                    text
                } else {
                    expected
                }
            );
            assert_eq!(
                output[0]
                    .anomalies
                    .iter()
                    .filter(|a| a.code == "frontmatter_unclosed")
                    .count(),
                usize::from(unclosed)
            );
            assert_eq!(output[0].records[0].dna_class, DnaClass::Standard);
        }
    }
}

/// Rejects damaged extensions, closed YAML, nonobjects and UTF-8 through ordinary located errors, never panic.
#[test]
fn markdown_damage_returns_located_private_errors() {
    for (bytes, expected) in [
        (
            &b"---\ntype: Memory\nmem_adaptor_envelope: okf:0.2\nmem_adaptor: []\n---\nBody."[..],
            "OKF mem_adaptor must be an object",
        ),
        (
            &b"---\ntype: Memory\nmem_adaptor_envelope: okf:0.2\nmem_adaptor: null\n---\nBody."[..],
            "OKF mem_adaptor must be an object",
        ),
        (
            &b"---\ncustom: [private-value\n---\nBody."[..],
            "Invalid YAML frontmatter",
        ),
        (
            &b"---\n- private-value\n---\nBody."[..],
            "frontmatter must be an object",
        ),
        (&b"\xffprivate-value"[..], "Invalid UTF-8 source"),
        (
            &b"---\ntype: Other\nmem_adaptor_envelope: okf:0.2\nmem_adaptor: {}\n---\nBody."[..],
            "Unexpected OKF memory type",
        ),
        (
            &b"---\ntype: Memory\nmem_adaptor_envelope: okf:0.2\nmem_adaptor: {}\n---\nBody."[..],
            "Invalid OKF record fields",
        ),
    ] {
        let error = read(&MarkdownReader, &source(&[("nested/note.md", bytes)]))
            .err()
            .unwrap();
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{message}");
        assert!(message.contains("nested/note.md"), "{message}");
        assert!(!message.contains("private-value"));
    }
}

/// Rejects present damaged categories rather than selecting an alternate protected category or guessing from filenames.
#[test]
fn damaged_protection_categories_fail_but_unknown_strings_are_preserved() {
    for value in [Value::Null, json!(9), json!([]), json!({})] {
        let document = json!([{"account_uuid":"synthetic","memory_files":[{"path":"/profile.md","type":value,"content":"---\ntype: profile\n---\nBody."}]}]).to_string();
        let error = read(
            &ClaudeReader,
            &source(&[("memories.json", document.as_bytes())]),
        )
        .err()
        .unwrap();
        let message = format!("{error:#}");
        assert!(message.contains("Invalid protection category"));
        assert!(message.contains("memories.json#/0/memory_files/0"));
        assert!(message.contains("fix the category"));
    }
    let error = read(
        &MarkdownReader,
        &source(&[("profile.md", b"---\ntype: null\n---\nBody.")]),
    )
    .err()
    .unwrap();
    assert!(format!("{error:#}").contains("Invalid protection category"));
    let output = read(
        &MarkdownReader,
        &source(&[("profile.md", b"---\ntype: feedback\n---\nBody.")]),
    )
    .unwrap();
    assert_eq!(output[0].records[0].dna_class, DnaClass::Standard);
    assert_eq!(output[0].records[0].evidence_level, EvidenceLevel::Inferred);
    assert_eq!(
        output[0].records[0].source_kind.as_deref(),
        Some("feedback")
    );
    assert!(
        output[0].source_records[0]
            .unmapped
            .iter()
            .any(|f| f.source_path == "/frontmatter/type")
    );
}

/// Distinguishes absent and empty optional content from damaged present fields, preserving unused UUID types.
#[test]
fn claude_content_types_and_unused_uuid_have_truthful_dispositions() {
    for value in [Value::Null, json!(7), json!({}), json!([])] {
        for (path, document, category) in [
            (
                "projects.json",
                json!([{"uuid":"p","docs":[{"uuid":"d","content":"Body."}],"prompt_template":value}]),
                "prompt_template",
            ),
            (
                "memories.json",
                json!([{"account_uuid":"a","conversations_memory":value,"project_memories":{"p":"Body."}}]),
                "conversations_memory",
            ),
        ] {
            let text = document.to_string();
            let error = read(&ClaudeReader, &source(&[(path, text.as_bytes())]))
                .err()
                .unwrap();
            let message = format!("{error:#}");
            assert!(message.contains(category), "{message}");
            assert!(message.contains(path));
            assert!(message.contains("must be a string"));
            assert!(message.contains("/0/"));
        }
    }
    for field in [None, Some("")] {
        let mut project = json!({"name":"Synthetic project","uuid":42,"docs":[{"uuid":"doc-id","content":"Doc."}]});
        if let Some(prompt) = field {
            project["prompt_template"] = json!(prompt);
        }
        let text = json!([project]).to_string();
        let output = read(
            &ClaudeReader,
            &source(&[("projects.json", text.as_bytes())]),
        )
        .unwrap();
        assert_eq!(output[0].records.len(), 1);
        assert_eq!(
            output[0].records[0].canonical_id,
            "kpjnewi7lsogcvyfzl2hifl7mwewmzt5"
        );
        assert_eq!(
            output[0].records[0].source_extra.as_ref().unwrap()["project"]["uuid"],
            42
        );
    }
    let text = r#"[{"uuid":42,"name":"Synthetic project","prompt_template":"Instructions."}]"#;
    let output = read(
        &ClaudeReader,
        &source(&[("projects.json", text.as_bytes())]),
    )
    .unwrap();
    assert_eq!(
        output[0].records[0].scope_qualifier.as_deref(),
        Some("sha256:052f25899c35ad3bf8f9f72d781a92077c5db95339cf8ec87d3bbe1ed89f2365")
    );
    assert_eq!(
        output[0].records[0].source_extra.as_ref().unwrap()["uuid"],
        42
    );
    assert!(
        !output[0].source_records[0]
            .field_map
            .iter()
            .any(|f| f.source_path == "/uuid" && f.canonical_path == "/scope_qualifier")
    );
    for field in [None, Some("")] {
        let mut account = json!({"account_uuid":"a","project_memories":{"p":"Body."}});
        if let Some(content) = field {
            account["conversations_memory"] = json!(content);
        }
        let text = json!([account]).to_string();
        let output = read(
            &ClaudeReader,
            &source(&[("memories.json", text.as_bytes())]),
        )
        .unwrap();
        assert_eq!(output[0].records.len(), 1);
        assert_eq!(output[0].records[0].content, "Body.");
    }
}

/// Uses independently calculated hashes for no-ID document fallbacks and precisely escapes legacy diagnostic keys.
#[test]
fn claude_fallback_id_and_legacy_diagnostics_are_specific() {
    let text = r#"[{"name":"Synthetic project","docs":[{"filename":"note.md","content":"Body without ID."},{"content":"Body without ID."}]}]"#;
    let output = read(
        &ClaudeReader,
        &source(&[("projects.json", text.as_bytes())]),
    )
    .unwrap();
    assert_eq!(
        output[0].records[0].source_record_id,
        "sha256:052f25899c35ad3bf8f9f72d781a92077c5db95339cf8ec87d3bbe1ed89f2365/note.md"
    );
    assert_eq!(
        output[0].records[1].source_record_id,
        "sha256:052f25899c35ad3bf8f9f72d781a92077c5db95339cf8ec87d3bbe1ed89f2365/sha256:f66eee763e92d0f2a6c751442922b0177ec95365374a8e2671d1a0e3ac7c3b62"
    );
    let text = r#"[{"project_memories":{"a/b":null,"c~d":7}}]"#;
    let output = read(
        &ClaudeReader,
        &source(&[("memories.json", text.as_bytes())]),
    )
    .unwrap();
    assert!(output[0].records.is_empty());
    assert_eq!(
        output[0]
            .anomalies
            .iter()
            .map(|a| a.source_locator.as_str())
            .collect::<Vec<_>>(),
        [
            "memories.json#/0/project_memories/a~1b",
            "memories.json#/0/project_memories/c~0d"
        ]
    );
}

/// Registers actual links, not checkboxes; unsupported or old private index metadata keeps root files ordinary.
#[test]
fn index_recognition_requires_native_version_only_and_registration_counts_links_only() {
    let output = read(
        &MarkdownReader,
        &source(&[(
            "MEMORY.md",
            b"- [A](a.md)\n- [ ] task\n- [x] done\n- [B](b.md)\n",
        )]),
    )
    .unwrap();
    assert_eq!(output[0].registered_count, 2);
    assert!(output[0].records.is_empty());
    for text in [
        "---\ntype: Index\n---\nBody.",
        "---\ntype: Index\nokf_version: '0.2'\n---\nBody.",
        "---\nokf_version: '0.3'\n---\nBody.",
    ] {
        let source = source(&[("index.md", text.as_bytes()), ("log.md", b"Log.")]);
        let claims = MarkdownReader.claim(&source.files);
        assert_eq!(
            claims.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
            ["index.md", "log.md"]
        );
        assert!(claims.iter().all(|c| !c.registered_only));
        assert_eq!(
            read(&MarkdownReader, &source)
                .unwrap()
                .iter()
                .flat_map(|o| &o.records)
                .count(),
            2
        );
    }
}

/// Keeps project scope stable across relocated roots, explicitly reporting relative fallback instead of leaking roots.
#[test]
fn claude_code_scope_uses_project_slug_or_warned_relative_parent() {
    for (path, expected, warning) in [
        ("projects/slug/memory/note.md", "slug", false),
        ("project-a/note.md", "project-a", true),
    ] {
        let mut source = source(&[(
            path,
            b"---\nmetadata:\n  node_type: memory\n  type: project\n---\nBody.",
        )]);
        let first = read(&MarkdownReader, &source).unwrap();
        source.root = "/different/relocated/root".into();
        let second = read(&MarkdownReader, &source).unwrap();
        assert_eq!(
            first[0].records[0].scope_qualifier.as_deref(),
            Some(expected)
        );
        assert_eq!(
            second[0].records[0].scope_qualifier,
            first[0].records[0].scope_qualifier
        );
        assert_eq!(
            first[0]
                .anomalies
                .iter()
                .any(|a| a.code == "project_scope_relative_fallback"),
            warning
        );
        assert!(first[0].records[0].updated_at.is_none());
        assert!(first[0].records[0].observed_at.is_none());
        assert!(first[0].records[0].valid_from.is_none());
    }
}

/// Converts signed decimal and exponent seconds at nanosecond precision without the original f64 drift.
#[test]
fn decimal_unix_seconds_have_exact_fractional_timestamps() {
    for (value, expected) in [
        (json!(1700000000.001), "2023-11-14T22:13:20.001Z"),
        (json!(-0.001), "1969-12-31T23:59:59.999Z"),
        (json!(1e-9), "1970-01-01T00:00:00.000000001Z"),
        (json!(1700000000), "2023-11-14T22:13:20Z"),
    ] {
        assert_eq!(normalize::time(&value).as_deref(), Some(expected));
    }
    assert!(normalize::time(&json!("not a timestamp")).is_none());
    assert!(normalize::time(&json!(1e100)).is_none());
}

/// Maps valid local tags/time only; missing kinds and invalid timestamps remain inferred and unpromoted.
#[test]
fn ordinary_markdown_optional_fields_are_mapped_or_preserved_without_guessing() {
    for (yaml, time, tags) in [
        (
            "updated_at: 2026-01-02T03:04:05Z\ntags: single",
            Some("2026-01-02T03:04:05Z"),
            Some(vec!["single".to_string()]),
        ),
        ("updated_at: invalid-time\ntags: [1]", None, None),
        ("custom: retained", None, None),
    ] {
        let text = format!("---\n{yaml}\n---\nBody.");
        let output = read(&MarkdownReader, &source(&[("profile.md", text.as_bytes())])).unwrap();
        let record = &output[0].records[0];
        assert_eq!(record.content, "Body.");
        assert_eq!(record.dna_class, DnaClass::Standard);
        assert_eq!(record.evidence_level, EvidenceLevel::Inferred);
        assert!(record.source_kind.is_none());
        assert_eq!(record.updated_at.as_deref(), time);
        assert_eq!(record.tags, tags);
        assert!(record.observed_at.is_none());
        assert!(record.valid_from.is_none());
        if yaml.contains("invalid-time") {
            let extra = record.source_extra.as_ref().unwrap();
            assert_eq!(extra["frontmatter"]["updated_at"], "invalid-time");
            assert_eq!(extra["frontmatter"]["tags"], json!([1]));
            assert!(
                output[0].source_records[0]
                    .unmapped
                    .iter()
                    .any(|f| f.source_path == "/frontmatter/updated_at")
            );
        }
    }
}

/// Refuses mixed malformed sources before success reports or target writes, checking every observable output channel.
#[test]
fn mixed_bad_inputs_fail_cli_without_partial_plan_or_target_changes() {
    for (name, bytes, marker, expected) in [
        (
            "memories.json",
            &b"{private-value"[..],
            Some(("users.json", "[]")),
            "Invalid Claude JSON",
        ),
        (
            "memories.json",
            &b"{\"account_uuid\":\"a\"}"[..],
            None,
            "Claude export must be a JSON array",
        ),
        (
            "conversations.json",
            &b"{}"[..],
            Some(("projects.json", "[]")),
            "Claude export must be a JSON array",
        ),
        (
            "conversations.json",
            &b"[{\"wrong\":\"private-value\"}]"[..],
            Some(("users.json", "[]")),
            "conversation entries must contain messages",
        ),
        (
            "projects.json",
            &b"[{\"uuid\":9007199254740992,\"prompt_template\":\"Body.\"}]"[..],
            None,
            "safe",
        ),
        (
            "note.md",
            &b"---\ntype: Memory\nmem_adaptor_envelope: okf:0.2\nmem_adaptor: private-value\n---\nBody."[..],
            None,
            "OKF mem_adaptor must be an object",
        ),
        (
            "note.md",
            &b"---\ncustom: [private-value\n---\nBody."[..],
            None,
            "Invalid YAML frontmatter",
        ),
    ] {
        let directory = TempDir::new().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir_all(root.join("target/nested")).unwrap();
        fs::write(root.join("target/nested/sentinel"), b"User bytes.\0").unwrap();
        fs::write(root.join("source/good.md"), "Good body.").unwrap();
        fs::write(root.join("source").join(name), bytes).unwrap();
        if let Some((path, text)) = marker {
            fs::write(root.join("source").join(path), text).unwrap();
        }
        let before = snapshot(&root.join("target"));
        let result = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
            .args([
                "plan",
                root.join("source").to_str().unwrap(),
                "--to",
                &format!("okf:{}", root.join("target").display()),
                "--report",
                root.join("plan.json").to_str().unwrap(),
            ])
            .env("XDG_CONFIG_HOME", common::config_home())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(!result.status.success(), "{name}");
        assert!(stderr.contains(expected), "{stderr}");
        if !expected.eq("safe") {
            assert!(stderr.contains(name), "{stderr}");
        }
        assert!(stderr.contains("Planning failed before target writes; target unchanged"));
        assert!(stderr.contains("Check the source"));
        assert_eq!(snapshot(&root.join("target")), before);
        assert!(!root.join("plan.json").exists());
        assert!(!root.join("receipt.json").exists());
        for stream in [&result.stdout, &result.stderr] {
            let text = String::from_utf8_lossy(stream);
            assert!(!text.contains("private-value"));
            assert!(!text.contains("panicked"));
        }
    }
}

/// Confirms unsafe unused UUID survives to core validation, rather than being stripped by a false mapping.
#[test]
fn unsafe_unused_uuid_reaches_schema_validation() {
    let directory = TempDir::new().unwrap();
    fs::write(
        directory.path().join("projects.json"),
        r#"[{"uuid":9007199254740992,"prompt_template":"Body."}]"#,
    )
    .unwrap();
    let mut registry = Registry::default();
    registry.register_reader(ClaudeReader).unwrap();
    let error = Engine { registry }
        .plan(
            directory.path(),
            GatePolicy {
                secrets: GateAction::Pass,
                high_risk_pii: GateAction::Pass,
                rule_allowlist: vec![],
                origin: PolicyOrigin::Default,
                user_selected: false,
            },
        )
        .err()
        .unwrap();
    assert!(format!("{error:#}").contains("safe"), "{error:#}");
}

/// Masks every nested Reader error context, not just the outer filename or CLI's final presentation.
#[test]
fn claude_damaged_yaml_masks_inner_and_outer_source_locations() {
    for (secret, prefix, outer, inner) in [
        (
            "synthetic-private-path-value".to_string(),
            "password=",
            "password=[secret]",
            "password=[secret]",
        ),
        (
            format!("ghp_TEST{}", "Q".repeat(32)),
            "",
            "[secret]/memories.json",
            "[secret]/memories.json#/0/memory_files/0",
        ),
    ] {
        let path = format!("{prefix}{secret}/memories.json");
        let input = source(&[(
            &path,
            br#"[{"memory_files":[{"path":"/note.md","content":"---\nbad: [\n---\nBody."}]}]"#,
        )]);
        let error = read(&ClaudeReader, &input).err().unwrap();
        let message = format!("{error:#}");
        assert!(message.contains("Invalid YAML frontmatter"));
        assert!(message.contains(&format!("Claude source at {outer}")));
        assert!(message.contains(&format!("Claude memory at {inner}")));
        assert!(!message.contains(&secret));
    }
}

/// Recognizes project-only Claude markers as siblings, but never treats descendants as parent source evidence.
#[test]
fn claude_project_only_marker_and_sibling_scope_are_recognized() {
    let siblings = source(&[
        ("memories.json", br#"[{"project_memories":{}}]"#),
        ("conversations.json", b"[]"),
    ]);
    assert_eq!(
        ClaudeReader
            .claim(&siblings.files)
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        ["conversations.json", "memories.json"]
    );
    let descendants = source(&[
        ("sub/memories.json", br#"[{"project_memories":{}}]"#),
        ("conversations.json", b"[]"),
    ]);
    assert_eq!(
        ClaudeReader
            .claim(&descendants.files)
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        ["sub/memories.json"]
    );
}

/// Empty shared memory arrays use sibling source evidence without creating a competing Claude claim.
#[test]
fn empty_chatgpt_generic_memories_and_transcripts_do_not_get_claimed_as_claude() {
    for marker in [
        "user.json",
        "memory.json",
        "saved_memories.json",
        "memories.chatgpt.json",
        "extract.chatgpt.md",
    ] {
        let marked = source(&[
            ("memories.json", b"[]"),
            ("conversations.json", b"[]"),
            (marker, if marker.ends_with(".md") { b"" } else { b"{}" }),
        ]);
        assert!(ClaudeReader.claim(&marked.files).is_empty(), "{marker}");
        let claims = ChatgptReader.claim(&marked.files);
        assert!(claims.iter().any(|c| c.path == "memories.json"), "{marker}");
        assert!(
            claims.iter().any(|c| c.path == "conversations.json"),
            "{marker}"
        );
    }
}

/// Independently inspects the complete native fixture set and exact body bytes, including the intentional empty document.
#[test]
fn website_fixtures_write_exact_native_sets_and_exclude_disabled_deleted_legacy_bodies() {
    for (name, expected) in [
        (
            "chatgpt",
            vec![
                (
                    "2bsrwbr7hr2nbtsglpi225eabpbigbhm",
                    "Prefer concise answers.",
                ),
                (
                    "qsrxnpzmv5ig6ese5cl3fe4dqiumgj3k",
                    "Synthetic occupation: test engineer.",
                ),
                (
                    "kuzbyrywvjhtcqe4sxysd5knsikvtl6d",
                    "Preserve my original words.",
                ),
                (
                    "li4pe5tr6ox2jedsc5b346krfv3cmbtg",
                    "Synthetic date must not become midnight.",
                ),
                (
                    "gczvrx44fd7oiblua36iuwl3gqffd4e6",
                    "Keep this unknown category.",
                ),
            ],
        ),
        (
            "claude",
            vec![
                (
                    "v3tnngptwkwcqiqf5r44u7bakp7jp2qi",
                    "---\ntype: profile\nname: Synthetic identity\n---\nKeep the entire synthetic file.\r\n",
                ),
                (
                    "j4rhy3vfmcydtdol4d7lpeosohpvrsa5",
                    "Plain synthetic project context.",
                ),
                (
                    "4j7k72aopijct757zovgilfyc6ihekpd",
                    "Synthetic knowledge document.\n",
                ),
                ("6nxnndc4s6g3n77lxajyzjuschtdbbj6", ""),
                (
                    "wkoezyvlsbpwi2dcshbshybidzph3jki",
                    "Follow these synthetic project instructions.\n",
                ),
            ],
        ),
    ] {
        let directory = TempDir::new().unwrap();
        let root = directory.path();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/m4")
            .join(name);
        let plan = root.join("plan.json");
        let receipt = root.join("receipt.json");
        let target = root.join("target");
        let planned = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
            .args([
                "plan",
                source.to_str().unwrap(),
                "--to",
                &format!("okf:{}", target.display()),
                "--report",
                plan.to_str().unwrap(),
            ])
            .env("XDG_CONFIG_HOME", common::config_home())
            .output()
            .unwrap();
        assert!(
            planned.status.success(),
            "{}",
            String::from_utf8_lossy(&planned.stderr)
        );
        assert!(!target.exists());
        let applied = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
            .args([
                "apply",
                plan.to_str().unwrap(),
                "--receipt",
                receipt.to_str().unwrap(),
                "--yes",
            ])
            .env("XDG_CONFIG_HOME", common::config_home())
            .output()
            .unwrap();
        assert!(
            applied.status.success(),
            "{}",
            String::from_utf8_lossy(&applied.stderr)
        );
        let actual = snapshot(&target);
        let mut paths = expected
            .iter()
            .map(|(id, _)| format!("memories/{id}.md"))
            .collect::<Vec<_>>();
        paths.extend(["index.md".into(), "log.md".into()]);
        paths.sort();
        assert_eq!(actual.keys().cloned().collect::<Vec<_>>(), paths);
        for (id, body) in &expected {
            let text = std::str::from_utf8(&actual[&format!("memories/{id}.md")]).unwrap();
            let (_, native_body) = text
                .strip_prefix("---\n")
                .unwrap()
                .split_once("\n---\n")
                .unwrap();
            assert_eq!(native_body, *body, "{id}");
        }
        let receipt: Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
        assert_eq!(
            receipt["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["verification"]["status"] == "verified")
                .count(),
            5
        );
        if name == "chatgpt" {
            let disabled = receipt["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["source_record_id"] == "synthetic-disabled")
                .unwrap();
            assert_eq!(disabled["disposition"]["reason"]["code"], "memory_disabled");
            assert!(disabled.get("target_id").is_none());
            assert!(disabled.get("verification").is_none());
        }
    }
}

/// Checks website metadata secrets across plan/receipt/stdout/stderr while independently confirming policy-dependent native values.
#[test]
fn website_metadata_secret_policies_are_reported_privately_and_enforced_in_native_files() {
    for name in ["chatgpt", "claude"] {
        for policy in ["pass", "block", "allowlist"] {
            let directory = TempDir::new().unwrap();
            let root = directory.path();
            let source = root.join("source");
            let target = root.join("target");
            let plan = root.join("plan.json");
            let receipt = root.join("receipt.json");
            fs::create_dir(&source).unwrap();
            let secret = format!("ghp_TEST{}", "Q".repeat(32));
            let private_metadata = "private-metadata-value-not-for-reports";
            let (file, data) = if name == "chatgpt" {
                (
                    "memory.json",
                    json!([{"id":"s","content":"Body.","kind":"tool","custom":{"credential":secret,"private":private_metadata}}]),
                )
            } else {
                (
                    "projects.json",
                    json!([{"uuid":"p","docs":[{"uuid":"s","content":"Body.","custom":{"credential":secret,"private":private_metadata}}]}]),
                )
            };
            fs::write(source.join(file), data.to_string()).unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"));
            command.env("XDG_CONFIG_HOME", common::config_home()).args([
                "plan",
                source.to_str().unwrap(),
                "--to",
                &format!("okf:{}", target.display()),
                "--report",
                plan.to_str().unwrap(),
                "--secret-policy",
                if policy == "block" || policy == "allowlist" {
                    "block"
                } else {
                    "pass"
                },
            ]);
            if policy == "allowlist" {
                command.args(["--allow-rule", "github-pat"]);
            }
            let planned = command.output().unwrap();
            assert!(
                planned.status.success(),
                "{}",
                String::from_utf8_lossy(&planned.stderr)
            );
            let applied = Command::new(env!("CARGO_BIN_EXE_mem-adaptor"))
                .args([
                    "apply",
                    plan.to_str().unwrap(),
                    "--receipt",
                    receipt.to_str().unwrap(),
                    "--yes",
                ])
                .env("XDG_CONFIG_HOME", common::config_home())
                .output()
                .unwrap();
            assert!(
                applied.status.success(),
                "{}",
                String::from_utf8_lossy(&applied.stderr)
            );
            for bytes in [
                fs::read(&plan).unwrap(),
                fs::read(&receipt).unwrap(),
                planned.stdout,
                planned.stderr,
                applied.stdout,
                applied.stderr,
            ] {
                let text = String::from_utf8(bytes).unwrap();
                assert!(!text.contains(&secret));
                assert!(!text.contains(private_metadata));
            }
            let plan: Value = serde_json::from_slice(&fs::read(plan).unwrap()).unwrap();
            let receipt: Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
            assert!(
                plan["entries"][0]["sensitive_findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["rule_id"] == "github-pat")
            );
            let actual = snapshot(&target);
            if policy == "block" {
                assert_eq!(plan["entries"][0]["disposition"]["status"], "rejected");
                assert_eq!(receipt["entries"][0]["disposition"]["status"], "rejected");
                assert!(actual.is_empty());
                assert!(!target.exists());
                assert!(receipt["entries"][0].get("verification").is_none());
            } else {
                assert_eq!(receipt["entries"][0]["verification"]["status"], "verified");
                assert_eq!(actual.len(), 3);
                let memory = actual
                    .iter()
                    .find(|(path, _)| path.starts_with("memories/"))
                    .unwrap()
                    .1;
                let native = std::str::from_utf8(memory).unwrap();
                assert!(native.contains(&secret));
                assert!(native.contains(private_metadata));
                let frontmatter = native
                    .strip_prefix("---\n")
                    .unwrap()
                    .split_once("\n---\n")
                    .unwrap()
                    .0;
                let parsed: Value = serde_saphyr::from_str(frontmatter).unwrap();
                let preserved = if name == "chatgpt" {
                    &parsed["mem_adaptor"]["source_extra"]["custom"]
                } else {
                    &parsed["mem_adaptor"]["source_extra"]["doc"]["custom"]
                };
                assert_eq!(preserved["credential"], secret);
                assert_eq!(preserved["private"], private_metadata);
            }
        }
    }
}
