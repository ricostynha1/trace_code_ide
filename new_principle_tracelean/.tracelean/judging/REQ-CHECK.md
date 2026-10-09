# Judging advice — REQ-CHECK

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `named_kinds` | drift |
| `progress_not_fault` | agrees, with a note |
| `qualifier_soundness` | drift |
| `severity_policy` | drift |
| `structural_is_not_exempt` | drift |

## `named_kinds`

*Drift.* `Kind` is a closed set with no generic error. `malformed`, though, is
"something wrong in a document or an annotation", and `check` reports every
index problem under it except `imprecise`. That covers unknown role, missing
id, unclosed region, stray end, and frontmatter that is not key/value. The
annotation grammar names each of these precisely as a `ProblemKind`, and the
finding throws that name away. Only the message text tells them apart.

## `progress_not_fault`

*Note.* The split is clean, and `progress_never_blocks` proves it. `unbound` is
classed as a fault (warn), not as progress. That is defensible given the prose,
though arguably it is also work not yet done.

## `qualifier_soundness`

*Drift: the expiry half is missing.* `qualifierKinds` ignores the third field
of `.exempt`. `exempt reason=r by=p until=2000-01-01` gives `[]`, where the
clause asks for a finding. No date is an input anywhere in the model.

## `severity_policy`

*Drift.* `blocksByDefault` is a fixed per-kind table with no policy input, so
the model does not express that which kinds block is *policy*. The default set
also has 7 of the 13 kinds, every error-severity kind. That is hard to read as
"small", although it does exclude every progress kind.

## `structural_is_not_exempt`

*Drift: two of the three parts are missing.* "Test required, model not" is
there. "Remains in the denominator" cannot be seen in this output: a tested
structural clause returns `[]`, exactly as an exempt one does. "Shall not reach
the DRT level" is modelled nowhere. No level function takes a structural flag.
