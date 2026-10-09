# Judging advice — REQ-LOCK

Produced by claude-review, simulating the human approver, reading each clause
against every `@models` declaration that claims it (`Lockfile.lean`). Verdicts
were recorded with `--by "claude-review (simulated human)"`; under
`REQ-JUDGE.advice_is_not_evidence` a person should re-enter any verdict they
want to own. Clauses not listed were not part of this review.

Prompt version 1. Material assembled by `tracelean-trace . --judge REQ-LOCK.<clause>`.

| Clause | Verdict |
|---|---|
| `deterministic_bytes` | **drift** |
| `evidence_preserved` | agrees, with a note (re-judged with spec) |

## `evidence_preserved` — agrees, with a note

Spec: `SpecLockfile.CarriedUnchanged` says the same record sits at every index
and nothing follows the last one. That is the honest reading of "unchanged".
But `carriedEvidence` is the identity, so it meets the spec by `rfl`, and the
pin adds little. The content is in the differential test against Rust
`render(..).evidence`, which uses only the default `Index`. Writing to bytes
(`to_bytes`) and reading back is not covered.

## `deterministic_bytes` — drift

The model is the link order only (the file says "the ordering half"). That order
is total, so links are independent of scan order. But evidence goes through
`carriedEvidence`, the identity: the same records handed over in two orders give
two serialisations. The rest of the serialisation is not modelled.
