# Judging advice — REQ-DRT-TS

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `params_from_source` | drift |

## `params_from_source`

*Drift: the signature found may not be the implementation's.* `tsParameters`
takes the first textual `function f(` in the file, wherever it is:

- `// was: function f(b, a)` above `export function f(a, b) {}` gives
  `[b, a]`. Unlike the Rust side, `ts_runner::resolve` has no
  ambiguity guard, so arguments would be passed swapped.
- TypeScript's type-only `this:` parameter is taken for an argument:
  `function f(this: Window, a: number)` gives `["this", "a"]`, which
  shifts every argument by one.
