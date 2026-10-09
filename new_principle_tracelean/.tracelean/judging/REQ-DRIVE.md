# Judging advice — REQ-DRIVE

Produced by claude-review, simulating the human approver. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**. The
verdict below was entered with `--judge … --verdict … --by "claude-review
(simulated human)"`.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-DRIVE.<clause>`.
Reviewed 2026-10-09.

| Clause | Verdict |
|---|---|
| `walk_follows_the_machine` | agrees, with a note |
| `session_is_a_value` | agrees (re-judged with spec) |

## `session_is_a_value`: agrees

`drive` takes a keymap, a mode and keys, and gives one step per key: the mode
it landed in and the bar there. Spec: `SpecSurface.SessionOf` says this
independently. Step *n* is key *n*, the mode from folding `nextMode` over the
first *n+1* keys, and `whichKey` of that mode, with nothing past the last key.
It adds no convention. `session_pinned` holds.

The other clauses were not in this review.

## `walk_follows_the_machine`: agrees, with a note

`drive` folds `nextMode` over the keys, and applies each key to the mode the
previous key left. `modesVisited` is a projection of those steps, not a second
walk. That is the clause.

The note concerns the theorems. The two that carry `@proves`
(`the_first_step_is_one_step_of_the_machine` and
`a_walk_is_as_long_as_the_session`) say less than the clause. The first covers
only the head step, and the second covers only length. No theorem says that
step *i*'s mode is `nextMode` applied to step *i-1*'s mode. That holds by the
definition of `drive`, so the model agrees, but a reader should not take the
proofs as the whole law. Compare `every_step_shows_the_bar_of_the_mode_it_reached`,
which is stated for every step.
