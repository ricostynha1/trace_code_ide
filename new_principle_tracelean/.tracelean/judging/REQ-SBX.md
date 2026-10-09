# Judging advice — REQ-SBX

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `capability_reported` | agrees, with a note |
| `escape_is_not_silent` | agrees, with a note |
| `passthrough_not_mirrored` | drift |
| `real_tree_untouched` | agrees, with a note |

### `capability_reported` — note

`capabilityViolations` checks only that the report names a mechanism or a
reason. Whether the name is true, and whether it reaches the user, is the axiom
`probe_reports_something` and the surface, not this model.

### `escape_is_not_silent` — note

Every observed write whose path string classifies `outside` is reported. A write
through a symlink inside the tree is not visible from the string, so the clause
depends on the observer recording resolved paths. The axiom `escapes_are_named`
restates `escapeViolations`' own definition, so it assumes nothing.

### `passthrough_not_mirrored` — drift

`classify` makes a path pass-through if *any* segment is in `passThroughDirs`.
`src/build/mod.rs` (source someone wrote, not regenerable) is therefore never
mirrored back. The other way, real caches outside the list (`.cache`,
`.mypy_cache`, `.gradle`) are mirrored. The clause is about regenerable
directories; a name match at any depth is neither.

### `real_tree_untouched` — note

Checked as a before/after snapshot over every path. A rewrite with identical
bytes, or a change reverted before exit, is a write the snapshot cannot see.
