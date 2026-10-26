---
id: REQ-DRT-RUST
title: Rust conformance runner
refines: [REQ-DRT-PROTO, ARCH-CORE-SHELL, ARCH-SELFHOST]
status: approved
decomposition: complete
clauses:
  generated: The system shall generate the runner as a crate and compile it, and shall not ship it as a script.
  project_untouched: When generating the runner, the system shall write only under the cache directory and shall never modify a file the project owns.
  path_dependency: The system shall have the generated crate reach the implementation as a library path dependency.
  params_from_source: The system shall take argument order from the implementation's own signature.
  types_inferred: The system shall infer argument and return types at the generated call site, and shall not let a binding name a type.
  build_error_explained: If a generated crate fails to compile, then the system shall report it as a binding that does not typecheck, naming what did not match.
  rewritten_when_stale: When the generated crate differs from what the generator would now produce, the system shall rewrite it.
---

# Rust conformance runner

Rust has no runtime reflection, so the runner is generated and compiled like
the Lean side. Parameter order is read from the implementation's `fn`
signature, and parameter types are inferred at the call site, because
`serde_json::from_value` is generic in its return type. A binding that cannot
name a type cannot name a wrong one. See
[ADR-0004](../../docs/decisions/ADR-0004-rust-conformance-runner.md).
