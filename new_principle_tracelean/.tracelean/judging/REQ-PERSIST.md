# Judging advice — REQ-PERSIST

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `replay_exact` | agrees, with a note |
| `checkpoint_equivalent` | agrees, with a note |
| `append_only` | drift |
| `truncated_is_reported` | agrees, with a note |

### `replay_exact` — note

Same as `ARCH-DETERMINISM.replay_exact`: replay is the fold of the recorder's
own `apply`, so equality with the recorded state rests on `REQ-CMD.single_path`,
not on anything proved here.

### `checkpoint_equivalent` — note

Holds by construction (`foldl` over `take k` then `drop k` is `foldl` over the
whole list, refusals included). The proof covers only the whole-log cut; every
other cut point rests on the differential test.

### `append_only` — drift

The bound model is `parseLog`, a reader. It constrains nothing about how the
log is written: an implementation that rewrote the file in place, or truncated
and re-wrote it, produces the same model behaviour. A writer model (new log =
old log ++ entries) would express the clause.

### `truncated_is_reported` — note

A half-written last line is reported (`truncatedAfter`) and replay stops before
it. Wider than the clause: a corrupt line in the *middle* is reported the same
way and every complete entry after it is discarded.
