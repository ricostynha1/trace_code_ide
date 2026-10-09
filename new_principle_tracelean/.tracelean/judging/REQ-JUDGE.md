# Judging advice — REQ-JUDGE

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`Judge.lean`). Verdicts were
recorded with `--by "claude-review (simulated human)"`; under
`REQ-JUDGE.advice_is_not_evidence` a person should re-enter any verdict they
want to own. Clauses not listed were not part of this review.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-JUDGE.<clause>`.

| Clause | Verdict |
|---|---|
| `human_decides` | **drift** |
| `advice_is_not_evidence` | **drift** |
| `invalidated_by_change` | agrees (re-judged with spec) |

## `invalidated_by_change` — agrees

`stillApplies` compares both hashes. Spec: `SpecJudge.StillStands` says a
judgement applies exactly when the requirement hash and the model hash both
match what it was made against. The clause states only the "invalidated" half.
The spec also adds that an unchanged judgement still stands, which is the
reasonable converse. `invalidated_by_change_pinned` holds.

## `human_decides` — drift

`record` writes L2 evidence for any `judgedBy`, the empty string included, so a
judgement identifying nobody is recorded. Only the separate `reproducibility`
check in `Record.lean` would flag it; the model claiming this clause does not.

## `advice_is_not_evidence` — drift

The model claiming it is `prompt`, which only says in its text that the answer
is advice. It does not model how anything enters the record, so a system that
wrote the tool's reply straight into evidence would satisfy it equally. The
property lives in `record` taking only a `Judgement` — which is not annotated
for this clause.
