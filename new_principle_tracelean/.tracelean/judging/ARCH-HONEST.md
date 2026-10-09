# Judging advice — ARCH-HONEST

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it. Verdicts were recorded with
`--by "claude-review (simulated human)"`; under `REQ-JUDGE.advice_is_not_evidence`
a person should re-enter any verdict they want to own.

Prompt version 1. Material assembled by `tracelean-trace . --judge ARCH-HONEST.<clause>`
and `--context … --parts requirement,models`.

| Clause | Verdict |
|---|---|
| `weakest_link` | **drift** |
| `lower_bound_marked` | **drift** |

## `weakest_link` — drift

`combine` is a minimum, but `rollUp` threads one visited set across siblings. In
a diamond — B and C refine A, D refines both B and C — D is walked under B only.
With D at L1 and C's own clauses at L4, C reports L4 though one of its parts is
L1. The root is still right; the intermediate node is not "the minimum over its
parts".

## `lower_bound_marked` — drift

`Figure` carries `exact := false` for an open decomposition, and nothing renders
it, so "rendered as a lower bound" is not modelled. More importantly, the figure
is not a lower bound: two written clauses, both met, give 2/2; an unwritten
unmet clause makes the truth 2/3. Only `met` bounds the truth from below; the
fraction (and the min-assurance) bounds it from above. The marking is honest
("not exact"); the word "lower bound" is not what the model computes.
