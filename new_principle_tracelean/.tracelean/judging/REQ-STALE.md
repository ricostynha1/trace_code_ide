# Judging advice — REQ-STALE

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`Staleness.lean`,
`Record.lean`). Verdicts were recorded with `--by "claude-review (simulated
human)"`; under `REQ-JUDGE.advice_is_not_evidence` a person should re-enter any
verdict they want to own. Clauses not listed were not part of this review.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-STALE.<clause>`.

| Clause | Verdict |
|---|---|
| `inputs_identified` | **drift** |
| `change_invalidates` | agrees |
| `retarget_invalidates` | agrees, with a note |
| `scanner_never_writes` | agrees |
| `stale_is_visible` | agrees |
| `no_silent_revalidation` | **drift** |

## `inputs_identified` — drift

`requiredInputs` asks a proof record for `model` and `toolchain` only, and a
differential record for `model` and `implementation`. A proof record that does
not identify the theorem it rests on is accepted as complete — weaken the
theorem's statement and the record stays valid (the link hash is identity, not
body). A differential record need not name the test or the requirement text
(which `requirement_reopens_all` says every record carries).

## `no_silent_revalidation` — drift

`revalidated` returns any produced record whose `linkHash` matches a stale one.
There is no notion of the owning backend, and no check that the new record is
itself fresh: `revalidated [r] [r] = [r]`, so resubmitting the stale record
revives it.

## `retarget_invalidates` — agrees, with a note

The model is right — a link hash not among the live ones is `linkRetargeted`
before any input is looked at. The proof `retargeted_is_stale` only covers the
empty live list, which is weaker than it reads.
