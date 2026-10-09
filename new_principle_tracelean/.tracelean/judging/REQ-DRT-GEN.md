# Judging advice — REQ-DRT-GEN

Produced by claude-review simulating the human approver, reading each clause
against every `@models` declaration for it. Under
`REQ-JUDGE.advice_is_not_evidence` this file is advice; the verdicts were
entered with `--judge … --verdict … --by "claude-review (simulated human)"`.

| Clause | Verdict |
|---|---|
| `seed_reproduces` | drift |

## `seed_reproduces`

*Drift: missing cases.* The clause covers every schema. The models cover two of
them: `natStream` and `strStream`. Generation for `Struct`, `List`, `Option`,
`Bool`, `Int` and enum schemas, which every differential run uses, has no
model. So the DRT cannot pin the Rust generator's stream for them. The proof,
`rawStream s c = rawStream s c := rfl`, is true of any Lean function and checks
nothing.
