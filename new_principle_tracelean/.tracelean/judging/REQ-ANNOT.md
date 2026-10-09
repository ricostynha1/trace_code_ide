# Judging advice — REQ-ANNOT

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `qualifiers` | drift |
| `role_vocabulary` | agrees |
| `totality` | drift |

## `qualifiers`

*Drift: attachment is not modelled.* The vocabulary is right: `partial`,
`exempt`, `nondeterministic`, `structural`. The clause also says each one
attaches to the nearest preceding annotation unless it carries its own
identifier. `parseComment` emits `.qualified q none none line` for a bare
qualifier and attaches it to nothing. `ProblemKind.orphanQualifier` exists, but
no model produces it. Concrete case: a comment that is only `@partial reason=x`
gives a directive with no target and no problem.

## `totality`

*Drift: a candidate can vanish.* `parseLineStep` reads the word after `@` with
`isAsciiLower || '_'`. With `@Models REQ-X.a`, a role spelled with a capital,
that word is empty and the function returns `acc` unchanged. The result is no
directive and no problem. That is the silent drop the requirement's prose names
as the reason for this clause. The only proof is about a comment with no `@` at
all.
