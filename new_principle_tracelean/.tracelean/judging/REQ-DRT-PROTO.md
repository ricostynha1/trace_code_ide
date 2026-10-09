# Judging advice — REQ-DRT-PROTO

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `failure_named` | drift |
| `line_delimited` | drift |
| `op_dispatch` | drift |
| `reply_exclusive` | agrees (re-judged with spec; see below) |
| `runner_shared` | drift |

## `failure_named`

*Drift: three of four failures are missing.* `hear` tells a non-reply line
apart from an answer. A runner that cannot start, times out or dies is a
process-level outcome, and no model has a constructor for it.

## `line_delimited`

*Drift: the stdin half is missing.* `hear` reads one reply line. Nothing models
a case being written as one JSON object on one line of stdin.

## `op_dispatch`

*Drift: the first half is missing.* `duplicateOps` covers "an op shall be
unique". Nothing models a case naming its entry point: there is no case record
with an `op` field and no dispatch on it.

## `reply_exclusive`

*Agrees.* Spec: `SpecTrace.ReplyExclusive` spells out "output and no error, or
error and no output" as a disjunction, not as the `!=` of `isExclusive`. It
pins `isExclusive`, and `hear` refuses a reply that fails it. An
`"output": null` counts as an output, on both sides.

## `runner_shared`

*Drift.* `duplicateOps` checks a consequence of sharing (ops must not collide).
It does not express one process per language per project. A plan that started
one process per binding would give the same model output.
