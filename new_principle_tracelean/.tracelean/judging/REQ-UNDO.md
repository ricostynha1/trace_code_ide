# Judging advice — REQ-UNDO

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `no_loss_on_branch` | agrees |
| `jump_equivalence` | agrees, with a note |
| `path_via_ancestor` | agrees |
| `preview_is_pure` | agrees, with a note |
| `reachable` | agrees |
| `tree_is_drawn` | agrees, with a note |
| `filtered_view` | drift |

### `jump_equivalence` — note

`runScript` computes both `travelled` (via `jumpTo`) and `replayed` (via
`stateAt`), and they agree given the `REQ-CMD` round-trip law. Nothing checks
`travelled = replayed` beyond the empty history: the differential test compares
each list with the implementation's, so a shared mistake on both sides would
pass.

### `preview_is_pure` — note

True by construction in a value-typed model; `currentAfterPreviews := current`
is a constant. It does its job only as the expected value the implementation is
compared against.

### `tree_is_drawn` — note

Checked only by the differential test; no property is proved.

### `filtered_view` — drift

A `Point` carries one `file : Option String`. The File filter keeps a node only
when that equals the file shown. A batch editing `a.rs` and `b.rs` gets
`file = none` (editor's `file_of`) and appears in neither file's view; a rename
`a.rs → b.rs` appears only under `b.rs`. The clause says "the nodes touching one
file". Nearest-kept-ancestor parenting and the marking agree.
