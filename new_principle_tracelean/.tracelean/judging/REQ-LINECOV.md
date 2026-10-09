# Judging advice — REQ-LINECOV

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`Lines.lean`). Verdicts were
recorded with `--by "claude-review (simulated human)"`; under
`REQ-JUDGE.advice_is_not_evidence` a person should re-enter any verdict they
want to own. Clauses not listed were not part of this review.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-LINECOV.<clause>`.

| Clause | Verdict |
|---|---|
| `per_test` | agrees |
| `clause_summary` | **drift** |

## `clause_summary` — drift

`spanCoverage` summarises one line span. The clause is about *all* implementing
items of a clause; summing `run`/`all` over several items, without
double-counting overlapping spans, and counting the union of tests, is done in
`crates/editor/src/lib.rs::clause_lines` and is not modelled. With one
implementing item the model is exact.
