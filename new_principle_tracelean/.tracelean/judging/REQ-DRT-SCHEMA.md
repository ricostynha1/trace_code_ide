# Judging advice — REQ-DRT-SCHEMA

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `derived_from_both` | drift |

## `derived_from_both`

*Drift: the by-position half is missing.* `pair` judges a single Lean type
against a single Rust type, and it is sound: Float, mismatches and unpaired
structures all give `err`. Pairing the two argument lists *by position*, and
reporting different arities (a Lean model with 2 arguments against a Rust
function with 3), happens in `derive.rs::derive`. That function has no model.
Neither do the text-to-`Ty` readers, so whether `&str` is refused, as the prose
promises, is not modelled either.
