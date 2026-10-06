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

Run `cargo test -p mem-adaptor-cli --test readers` from the repository root.
