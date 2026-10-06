---
adr: 6
title: Arguments are deserialized from the raw case slice, not from an owned value
status: accepted
supersedes: 4
affects: [REQ-DRT-RUST]
---

# ADR-0006 — Borrowed arguments

## Context

[ADR-0004](ADR-0004-rust-conformance-runner.md) specified `serde_json::from_value`
at the generated call site, with the type inferred from the parameter position.
It does not compile for a borrowed parameter: `from_value` requires
`DeserializeOwned`, and `&str` implements `Deserialize<'de>` only for a specific
lifetime. The first end-to-end test failed on exactly that, against a real
function taking `&str`.

Restricting implementations to owned parameters was the alternative, and it is
not acceptable: idiomatic Rust takes `&str` and `&[T]`, so the rule would have
excluded most functions worth checking, for a reason internal to the harness.

## Decision

The case line is parsed into fields of `&RawValue`, and each argument is
deserialized with `serde_json::from_str` from its raw slice. The slice borrows
from the line, which outlives the call, so a borrowed parameter is fine and an
owned one is unaffected.

Type inference is unchanged — `from_str` is generic in its return type the same
way `from_value` was — so `types_inferred` still holds and a binding still cannot
name a type.

## Consequences

The generated crate needs `serde_json`'s `raw_value` feature.

Errors improve: each argument is bound to a local named after the parameter, so a
shape mismatch reports `argument \`source\`: invalid type …` rather than naming
only a position.
