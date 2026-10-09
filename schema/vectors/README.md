# Schema v0 vectors

All values are synthetic. These vectors validate structure, not migration semantics.
The illustrative hashes and IDs are placeholders, not hashes calculated from the example content.

Valid files wrap an actual `document` with its `schema` name and `expected_valid: true`.
The invalid cases declare `expected_valid: false`, reference a valid `base` relative to this directory,
and list the violated `constraint`. Each `patch` uses the RFC 6902 JSON Patch `add` or `remove`
operation targeting an object member. `add` replaces an existing member when present.
Paths use JSON Pointer, including array indices when navigating to the parent object.
Apply the patch to `base.document`, then validate against `base.schema`.

Register all five root schemas by their `$id` before validating. The URN identifiers are local registry
keys, not download endpoints. Enable `format` assertions, including RFC 3339 timestamps.
The schemas and vectors are language-neutral and do not require importing the Rust implementation.

Schema validation alone does not verify content hashes, canonical ID derivation, sorted digest inputs,
plan digest recomputation, privacy masking, field coverage, byte span ordering, or vector length vs dimension.
These checks belong to the engine and the independent conformance runner in later milestones.

M3 adds required digest `record_hash`, optional plan `previous_receipt_ref`, and historical `prior_write`.
Written receipt entries require `prior_write`; skipped entries can carry it without claiming current verification.
The 44 invalid cases include metadata binding and historical-state constraints.

M4 adds optional nonempty object `source_extra`, optional field-mapping `rule`, and inventory `deleted_count`.
The 50 invalid cases also reject null/array/empty source metadata, inline metadata in reports, and negative deletion counts.
Only the arbitrary values nested inside `source_extra` may retain source nulls.

M5 requires `TargetSpec.artifacts`, per-entry and predicted `target_map`, and historical `target_hash`.
Artifact snapshots omit both hash/bytes when missing and require both when present.
The 60 invalid cases also cover native-hash binding, paired snapshot fields, required mapping rules,
valid JSON Pointers, and rejection of inline native payload or mapped values in reports.
Plan artifacts capture approval-time state. Receipt shared artifacts retain the historical written
snapshot when a run has no verified write, rather than accepting user changes as a new overwrite baseline.

M0–M5 review fixes add optional `Finding.key_hash` (parent-object path and key-relative byte span),
and separate `duplicate_write` representative evidence from a record's own `prior_write`.
Recursive `jcs_value` validation rejects all integers outside ±(2^53−1), including nested metadata
and integral scientific notation. The 67 invalid cases include these constraints.
Writer output proofs are checked against observed files; observing a changed file does not create
a new successful historical overwrite baseline. Existing golden reports and native outputs remain unchanged.

M6 adds optional `source.satellite_id` (8 lowercase base32 characters, `^[a-z2-7]{8}$`) to the canonical
source identity, and the optional report `source.satellite` object (`id` required, display-only `label`
optional) shared by plan and receipt reports through one definition. Both fields stay absent in direct
migration; the engine, not these schemas, derives canonical IDs and binds receipts to satellite identity.
The 72 invalid cases also reject wrong-alphabet or wrong-length satellite IDs, a satellite without its
registered ID, and unknown satellite members such as a source path.

M6 `init` adds the optional top-level config `satellites` registry: each entry carries the issued `id`,
a display-only `label`, the detected `system`, an RFC 3339 `created_at`, and an optional `path` that
export-bundle satellites omit. The registry is append-only after an approved apply; direct-mode user
configuration stays valid without it (`valid/config-direct.json`).
The 83 invalid cases also reject wrong-alphabet or wrong-length registry IDs, unknown registry members,
missing system/issuance fields, non-RFC 3339 issuance times, empty labels, empty registered paths, and
payload keys on the payload-free `target_unmanaged` refusal.

M6 home-edit rules (DEC-21 B/C) add the four-rule conflict carriers and the home-only omission:
`valid/plan.json` carries an `omitted home_modified` entry with its `home_changed_fields`, a top-level
`conflict_clusters` list with satellite and home candidates, and the same list bound inside
`digest_inputs`; `valid/receipt.json` carries a `home_modified` entry and a `keep` verdict selecting a
candidate through `bases`; `valid/canonical-full.json` extends `conflict_candidates` with `origin`,
`content_hash`, and `record_hash`. `prior_write` gains no field.
The 92 invalid cases also reject a `home_modified` omission without (or with an empty)
`home_changed_fields`, plans and digest inputs without their `conflict_clusters`, conflict candidates
without an origin or record hash, a non-hash cluster id, an empty verdict `bases` list, and canonical
conflict candidates without an origin.
