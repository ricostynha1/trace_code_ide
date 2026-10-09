# Judging advice — REQ-CMD

Produced by claude-review, simulating the human approver. Under
`REQ-JUDGE.advice_is_not_evidence` this is **advice and not a record**. The
verdict below was entered with `--judge … --verdict … --by "claude-review
(simulated human)"`.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-CMD.<clause>`.
Reviewed 2026-10-09.

| Clause | Verdict |
|---|---|
| `single_path` | unmodelable |

The other clauses were not in this review.

## `single_path`: unmodelable

"Every mutation … shall be expressed as a command, and no state shall change by
another route." `apply` is the interpreter of commands, and it is a good model
of what a command does. The clause, though, is about exclusivity: no other
route exists. That depends on call sites. `Workspace.set` and `Workspace.remove`
are public, and a caller that used them directly would break the clause while
`apply` stayed as it is. No argument to `apply` can show that it is the only
way in. This wants an architectural check that no code outside the command
interpreter writes workspace state. One place to look is the editor's
`accept`. It pushes commands, which is fine, and then calls
`workcopy::sync(&self.root, …)` to write the real tree. That is persistence of
an already-commanded state rather than a second route, but a check should make
the distinction explicit.
