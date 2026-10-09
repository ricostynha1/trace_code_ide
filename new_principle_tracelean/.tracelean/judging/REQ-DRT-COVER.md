# Judging advice — REQ-DRT-COVER

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `law_coverage` | drift |
| `vacuous_named` | drift |

## `law_coverage`

*Drift.* The requirement's own prose says per-law coverage "is not yet
designed", and the model matches that prose rather than the clause.
`coverageVerdict` judges per-binding floors keyed by a situation string and
returns only the *first* failing floor. It has no law, no precondition, and no
per-law record of whether cases reached it.

## `vacuous_named`

*Drift.* `vacuous` fires only for a declared floor with `atLeast > 0` and a
count of 0:
- a law with no floor at all gives `undeclared`;
- `[{situation := "x", atLeast := 0}]` with nothing observed gives `met`;
- a vacuous floor listed after a short one is hidden behind `short`.

None of these passes as L3 (`coverageLevel`), but a law satisfied only because
nothing reached its precondition is not reliably *named* vacuous.
