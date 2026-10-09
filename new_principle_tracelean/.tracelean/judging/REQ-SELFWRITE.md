# Judging advice — REQ-SELFWRITE

## Independent review — 2026-10-09

Produced by claude-review simulating the human approver. Advice, not a record
(`REQ-JUDGE.advice_is_not_evidence`).

| Clause | Verdict |
|---|---|
| `suppression_is_consumed` | drift |

### `suppression_is_consumed` — drift

`remaining` serves as both the expiry counter and a count of suppressions.
`suppress` decrements it on a match and keeps the write pending while it is
above zero. Input `pending = [{p, "x", remaining := 2}]`, observe `(p, "x")`
twice: both observations are suppressed. The clause says one. The model agrees
only when every write starts at `remaining := 1`, which nothing in it enforces.
