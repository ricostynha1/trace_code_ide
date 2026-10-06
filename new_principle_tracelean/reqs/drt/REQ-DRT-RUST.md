---
id: REQ-DRT-RUST
title: Rust conformance runner
refines: [REQ-DRT-PROTO, ARCH-CORE-SHELL, ARCH-SELFHOST]
status: approved
decomposition: complete
clauses:
  generated: The runner shall be generated as a crate and compiled, not shipped as a script.
  project_untouched: Generation shall write only under the cache directory and shall never modify a file the project owns.
  path_dependency: The generated crate shall reach the implementation as a library path dependency.
  params_from_source: Argument order shall be taken from the implementation's own signature.
  types_inferred: The binding shall not name a type; argument and return types shall be inferred at the generated call site.
  build_error_explained: A generated crate that fails to compile shall be reported as a binding that does not typecheck, naming what did not match.
  rewritten_when_stale: The generated crate shall be rewritten whenever it differs from what the generator would now produce.
---

# Rust conformance runner

Stage-0 TraceLean refuses any implementation language but Python, so nothing in
this Rust project can reach L3 until that is lifted. It is therefore built before
anything it exists to check.

Python binds arguments by runtime reflection; Rust has none. So the runner is
generated and compiled like the Lean side, and two mechanisms replace reflection:
parameter *order* is parsed from the implementation's `fn` signature, and
parameter *types* are inferred at the call site, because `serde_json::from_value`
is generic in its return type and each call resolves by the position it is passed
into.

`types_inferred` earns the design: a binding that cannot name a type cannot name
a wrong one.

See [ADR-0004](../../docs/decisions/ADR-0004-rust-conformance-runner.md).
