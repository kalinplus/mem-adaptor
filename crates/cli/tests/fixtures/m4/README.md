# Synthetic M4 fixtures

Every input is synthetic. No fixture was copied from a personal memory directory or a real export.

| Directory | Expected result |
|---|---|
| `markdown` | One preference record, one registered index with two links; typed metadata is retained. |
| `chatgpt` | Six records, including one disabled/unresolved record; one deleted record is counted, four anomalies are reported, chats and user metadata are registered only. |
| `claude` | Five records; new memory files take precedence, project documents and instructions remain separate, two anomalies are reported, chats and users are registered only. |

`snapshots/*.plan.json` are complete masked plan-report examples. Runtime ID, time, and source/target
paths are normalized, then the plan digest is recalculated. They are review snapshots, not executable approvals.
Tests compare parsed JSON values, so object formatting does not affect acceptance.
These are implementation regression tests, not the independent M7 conformance runner.

Native-rule repair changes only `target_map` and its digest-bound predictions, then recalculates
the digest: every record maps its actual source ID, none of these scan/import records claims
`generated.at`, Markdown maps its tags, and the Claude record with a modification time maps
`sources.last_modified`. Source inventory, body/record hashes, dispositions, anomalies and all
other values remain unchanged. The three digests are also recomputed with independent Python
sorted-key JSON over these number-free digest inputs, not copied from engine results.

Run `cargo test -p mem-adaptor-cli --test readers` from the repository root.
