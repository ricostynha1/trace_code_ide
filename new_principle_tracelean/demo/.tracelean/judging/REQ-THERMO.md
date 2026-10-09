# Judging advice — REQ-THERMO

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `to_fahrenheit` | agrees |
| `to_celsius` | agrees |

No drift. `ToFahrenheit` pins `f = ⌊9c/5⌋ + 32` and `ToCelsius` pins
`c = ⌈5(f−32)/9⌉` ("round up" read as toward +∞, so −2.2 gives −2); the
`@pins` theorems prove each function meets its predicate and that no input has
two right answers, for every integer.
