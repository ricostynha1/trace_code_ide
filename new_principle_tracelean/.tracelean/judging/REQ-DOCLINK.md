# Judging advice — REQ-DOCLINK

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`DocLink.lean`). Verdicts
were recorded with `--by "claude-review (simulated human)"`; under
`REQ-JUDGE.advice_is_not_evidence` a person should re-enter any verdict they
want to own. Clauses not listed were not part of this review.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-DOCLINK.<clause>`.

| Clause | Verdict |
|---|---|
| `confirmation_is_human` | agrees, with a note |
| `decisions_exempt` | **drift** |
| `dangling_reported` | agrees (re-judged with spec) |
| `hash_moves_review` | agrees (re-judged with spec) |

## `dangling_reported`, `hash_moves_review` — agrees

`docState` dangles a link whose target has no entry, is current on an equal
hash, and otherwise is in review. Spec: `SpecTrace.DocStanding` says the same
three cases without calling `docState`. Its one added convention is that the
first entry for a repeated target wins (`HashedNow`), which is reasonable.
`doc_standing_pinned` holds.

## `confirmation_is_human` — agrees, with a note

`recordedFrontmatter` only produces text, and `docState` returns to `current`
only when the recorded hash equals the live one, so nothing in the model moves a
document back. That a *person* writes it is outside any function; the model
covers the "never automatically" half.

## `decisions_exempt` — drift

`declaredIn` returns `[]` for any frontmatter without `describes`. A decision
record with no `affects` key is treated exactly like one that has it, so "shall
declare the requirements it affects" is never read or required; the model only
shows a decision record gets no hashed target.
