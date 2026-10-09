# Judging advice — REQ-DRT-RUST

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `params_from_source` | agrees, with a note |
| `types_inferred` | drift |

## `params_from_source`

*Note: the prompt shows the wrong declaration.* The doc comment carrying
`@models` sits above `private def parameterStep`, not above `parameters`, so
`--judge` prints only the per-parameter step. The DRT binds
`TraceLean.Signature.parameters`. Judged on `parameters`: names come in
declaration order, `self` is dropped, and patterns are refused.

`parameters` takes the *first* textual `fn name` match. With
`// fn f(b, a)` above `fn f(a, b)` the model answers `[b, a]`. Only the
unmodelled `declarations > 1` guard in `rust_runner::resolve` prevents that.

## `types_inferred`

*Drift.* The model bound here is the parameter-name parser. It says nothing
about the binding having no type field, or about types being inferred at the
generated call site. A generator that wrote `from_value::<u64>` explicitly
would leave every model output unchanged.
