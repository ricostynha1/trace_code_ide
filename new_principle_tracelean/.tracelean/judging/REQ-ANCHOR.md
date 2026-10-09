# Judging advice — REQ-ANCHOR

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

Material assembled by `tracelean-trace . --judge` and `--context --parts requirement,models`.

| Clause | Verdict |
|---|---|
| `hash_tracks_body` | agrees, with a note |
| `imprecise_capped` | drift |
| `stable_under_move` | drift |

## `hash_tracks_body`

*Note.* `bodyHash = digest ∘ normalize`, so the hash is a function of the
normalised body. That gives "only if" exactly. "If" holds up to 64-bit FNV-1a
collisions, which is acceptable. The one proof,
`reindentation_does_not_move_the_hash`, covers a single reindent example and
neither direction of the biconditional in general.

## `imprecise_capped`

*Drift: half the clause is missing.* `anchorCeiling false _ = L1` is the cap,
and L1 is the bottom of `Level`. Nothing bound to this clause says a file with
no grammar anchors to the whole file. `precise` is a free Bool, and no model
connects it to whether a grammar is available.

## `stable_under_move`

*Drift.* `anchorIdent` for a region is `file@start..end` in byte offsets.
Insert one line above an unchanged `begin`…`end` region and its identity
changes. A declaration's identity is its symbol path, so moving an unchanged
function into an `impl` or a `mod` also changes it. The doc comment says
identity "survives edits within the file", but that is true only of a `decl`
whose path stays the same.
