---
adr: 4
title: The Rust runner is a generated crate that infers its own types
status: accepted
affects: [REQ-DRT-RUST, REQ-DRT-PROTO]
---

# ADR-0004 — The Rust conformance runner

## Context

Differential testing needs both sides to answer JSON cases on stdin. Stage 0
ships a Python runner and *generates* the Lean one. Rust has neither, which is
why no Rust implementation can currently reach L3 — and this project is Rust.

Python's runner works by reflection: `inspect.signature` gives parameter names at
runtime, and arguments are bound by name. Rust has no runtime reflection, so the
same trick is unavailable.

## Decision

The Rust side is **generated and built**, exactly as the Lean side already is: a
small crate written into `.tracelean/drt-rust/`, depending on the project's crate
by path, with a `main` that reads cases and dispatches on `op`.

Two mechanisms make it work without type annotations in the binding.

**Parameter order comes from the source.** The generator parses the
implementation's `fn` signature for parameter names in order — the same parse
stage-0 `drt/bind.rs::implementation_parameters` already performs for Rust. The
binding's `params` map renames model fields onto those parameters.

**Types come from inference.** The generated call site is

```rust
let out = tracelean_core::evidence::assurance(
    serde_json::from_value(field(&input, "records")?)?,
    serde_json::from_value(field(&input, "bond")?)?,
);
serde_json::to_value(out)
```

`from_value` is generic in its return type, and Rust infers each one from the
parameter position it is passed into. The binding therefore never names a type,
and cannot name a wrong one. The requirement this places on an implementation is
the honest one: parameters `DeserializeOwned`, return value `Serialize`.

## Consequences

An implementation must be reachable as a library: a `pub` function in a crate
with a `[lib]` target. That is a real constraint and it is the right one — a
function nothing can call is a function nothing can check.

The generated crate lives under `.tracelean/`, a cache directory, and is rewritten
whenever it differs from what the generator would produce. TraceLean never edits a
file the project owns, which is the same rule the Lean generator follows.

A build failure in the generated crate is reported as *"the binding does not
typecheck"* and names the mismatched parameter, since that is what it almost
always is. This is a genuine advantage over the Python runner, where the same
mistake surfaces as a `TypeError` on the first case.

## Rejected

*Ship a `build.rs`-style harness inside the project.* Requires editing the
project's own `Cargo.toml`, which the Lean side deliberately avoids.

*Require type names in the binding.* Restores the annotation burden inference
removes, and creates a way for the binding to be wrong that the compiler would
otherwise have made impossible.

*Serialize across a C ABI or use a dynamic plugin.* Enormously more machinery for
a process that only has to read lines and write lines.
