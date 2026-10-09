# Judging advice — REQ-STRENGTH

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`Strength.lean`,
`Pinning.lean`). Verdicts were recorded with `--by "claude-review (simulated
human)"`; under `REQ-JUDGE.advice_is_not_evidence` a person should re-enter any
verdict they want to own.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-STRENGTH.<clause>`.

| Clause | Verdict |
|---|---|
| `obligation_generated` | **drift** |
| `not_proved_by_us` | agrees |
| `open_is_the_default` | agrees |
| `attempted_distinguished` | agrees |
| `nondeterministic_declared` | **drift** |
| `qualifies_proof` | agrees, with a note |
| `per_input` | agrees |
| `kernel_decides` | **drift** |
| `verdict_kept` | agrees (re-judged with spec; see below) |

## `obligation_generated` — drift

`obligationSource` puts each proved theorem into the specification only as a
comment followed by `True`. The generated obligation is therefore always
`∀ f g, True → True → f = g`, whatever was proved: it never says that the
proved properties determine the model. (`per_input`'s `statement` does state a
real obligation; this one does not.)

## `nondeterministic_declared` — drift

A nondeterministic model is not excluded: `Strength.isSettled` — "whether this
may be presented as a pass" — returns `true` for it exactly as for `pinned`.
Excluded and passing are different; counting it as settled inflates. An empty
reason is also accepted.

## `kernel_decides` — drift

`accepted` reads only Lean's output, so "stated as the obligation" lives in the
unmodelled `check_source` (and, without a specification, is not checked at all).
And it inspects the axioms line by line: when Lean wraps a long axiom list, the
first line is `'T' depends on axioms: [propext,` and `sorryAx` is on a later
line, so a theorem depending on `sorry` is accepted.

## `qualifies_proof` — agrees, with a note

`obligationsFrom` pairs each declaration's proved theorems with its strength,
which is what a presentation needs. The presentation itself is not modelled, and
`obligationsFrom` never yields `pinned` — that comes from `standing`.

## `verdict_kept` — agrees

Spec: `SpecTrace.VerdictStanding` says a clause is pinned only when the kept
record passed for this theorem name and this declaration key. Any other record,
or none, leaves it attempted. With no theorem it is open, which comes from the
sibling clauses `open_is_the_default` and `attempted_distinguished`. That is
consistent and not an overreach. `verdict_kept_pinned` holds.
