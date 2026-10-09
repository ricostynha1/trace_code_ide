# Judging advice — ARCH-DETERMINISM

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`); the verdicts were entered with `--judge …
--verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `replay_exact` | agrees, with a note |
| `stable_ordering` | drift |

### `replay_exact` — note

`replay` folds the same `apply` the recorder uses, from `base.canon`, and stops
at a refusal rather than inventing a state. That reaches the recorded state only
because recording goes through `apply` too (`REQ-CMD.single_path`); no theorem
here relates replay to a recorded state, and the differential test compares the
implementation's replay with the model's, not with what was recorded.

### `stable_ordering` — drift

The clause covers *every* collection written to disk or compared. The model
orders one: the lockfile link list (`orderedLinks` / `lockLinkLe`, which is
total over every field). Evidence records, the history log, touched-path lists
and the rest have no model under this clause. The one proof,
`ordering_is_idempotent_on_the_empty_list`, says `orderedLinks [] = []` — its
docstring claims idempotence, which it does not prove.
