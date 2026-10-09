# Judging advice — REQ-PROV

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `position_question` | agrees |
| `base_is_honest` | drift |

### `base_is_honest` — drift

The `@models` sits in the docstring of the `Origin.base` *constructor*, and the
scanner binds it to the next declaration, the `BackStep` inductive — which is
what `--judge` prints and what the hash covers. `BackStep` has no base case and
says nothing about attribution. The behaviour the clause wants is in `origin`
(annotated only for `position_question`), and it also answers `.base` when a
node id on the ancestry is missing from the tree — a corrupt tree reported as
"arrived with the file". The proof `nothing_recorded_means_nothing_written`
shows `origins base [] … = []`, an empty list, not a `.base` answer. Fix: move
the annotation onto `origin` (or a dedicated function), and prove a base
attribution for an unedited position.
