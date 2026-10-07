# Synthetic M5 Writer fixtures

`source/note.md` is synthetic. `snapshots/` contains the generated OKF memory, scope index,
migration log, and UMP JSON array for that same source.

Only runtime migration timestamps are normalized. The original body and typed source metadata are not rewritten.
The UMP creation marker is `target_migration`; the canonical source still has no `created_at`.
No fixture implies an embedding call, human fact verification, or independent M7 conformance.

Native-rule repair changes only index/log goldens: the root index keeps version-only frontmatter
and moves ownership to a body comment; the log uses ISO date headings, newest-first entries and
`+added ~updated` counts. Memory bytes and UMP JSON are unchanged. The log normalization replaces
only its migration date/time, never ownership, entry text, counts or ordering.
