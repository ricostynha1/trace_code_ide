# Judging advice — REQ-ROLLUP

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`Rollup.lean`). Verdicts
were recorded with `--by "claude-review (simulated human)"`; under
`REQ-JUDGE.advice_is_not_evidence` a person should re-enter any verdict they
want to own.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-ROLLUP.<clause>`.

| Clause | Verdict |
|---|---|
| `min_not_mean` | **drift** |
| `open_is_lower_bound` | **drift** |
| `never_complete_when_open` | agrees |
| `exempt_leaves_denominator` | **drift** |
| `untraced_counted` | **drift** |
| `deterministic_order` | **drift** |

Three of these come from one root cause: `rollUpWalk` shares its visited set
across siblings.

## `min_not_mean` — drift

`combine` takes the minimum. But in a diamond (B and C refine A; D refines B and
C) D is a child of B only, so C's aggregate over its children leaves D out. With
D at L1 and C at L4, C reads L4.

## `open_is_lower_bound` — drift

The model marks an open figure `exact := false`, but what it computes is an
upper bound: 2 written clauses both met give 2/2 and assurance L4; an unwritten
unmet clause makes the true value 2/3 and L1. Only the `met` count is a lower
bound.

## `exempt_leaves_denominator` — drift

The first half is modelled: `figureFor` filters exempt clauses out. The second
is not: `Node` has no partial clause and nothing caps a contribution, so "a
partial clause shall remain in it with a capped contribution" has no model.
(Separately, exempt clauses still feed the assurance via `own`, at L1 when they
have no level.)

## `untraced_counted` — drift

`fileCoverage` counts files, not code. A file with one annotated function and
ninety-nine unannotated ones counts as 1/1, so unclaimed code inside a claimed
file is never in the denominator.

## `deterministic_order` — drift

Children are visited in id order. In the diamond above D lands under B; were C
visited first, D and its level would move to C. The model makes the order fixed,
not irrelevant: per-node assurance and children depend on it. `canonNodes` and
`canonLevels` also keep whichever duplicate came last in the input.

## `never_complete_when_open` — agrees

`figureFor` sets `exact` from `complete`, and `Figure.isComplete` needs `exact`;
`an_unclaimed_decomposition_is_never_complete` proves it for every figure.
