# Judging advice — REQ-MIRROR

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `apply_reproduces` | drift |
| `binary_handled` | drift |
| `ordering_defined` | agrees, with a note |
| `protected_excluded` | agrees, with a note |

### `apply_reproduces` — drift

`mirrored before after` equals the observed tree only on mirrored paths.
Input `before = {.git/x: "a"}`, `after = {.git/x: "b"}` (or `target/x`) gives
`before` back, not the observed tree. That is what `protected_excluded` and
`REQ-SBX.passthrough_not_mirrored` demand, so the fix is in the clause ("the
observed tree on mirrored paths"), not the model. Separately, `mirrorApply`
swallows a refused command and carries on, so a refusal shows up only as a
wrong final state.

### `binary_handled` — drift

`changeAt path .opaque .opaque = []`. A binary file whose bytes changed between
snapshots is opaque on both sides and is reported as nothing. `FileState.opaque`
carries no hash, so the model cannot tell a changed binary from an unchanged
one. Text↔opaque and opaque↔absent are reported correctly, and
`an_opaque_file_is_never_mirrored` holds; the missing half is "reported as a
change".

### `ordering_defined` — note

Deletions, then creations (each with its fill), then content changes, over
sorted paths. The workspace is a flat path→content map, so the doc's own example
(a file inside a directory the same batch creates) cannot arise in the model;
the ordering is sound for what is represented.

### `protected_excluded` — note

Filtered through `isMirrored`, false for every protected path after `..` is
resolved. Protected means `.git`/`.tracelean` at the root only; a nested `.git`
(submodule) classifies as mirrored.
