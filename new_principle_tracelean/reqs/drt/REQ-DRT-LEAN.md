---
id: REQ-DRT-LEAN
title: Lean model runner
refines: [REQ-DRT-PROTO, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  generated: The system shall generate and build the model side as a Lean package, and shall not take one written by the project.
  project_untouched: When generating the model side, the system shall not modify the project's own Lean package or its build configuration.
  toolchain_explained: If the Lean toolchain is missing, then the system shall report it as a distinct, explained state rather than as a failure of the model.
  compiled_not_interpreted: The system shall compile the model, so that a case costs microseconds and large runs are affordable.
  encoding_declared: The system shall encode the model's inputs and outputs as JSON by one declared convention that the schema grammar describes.
---

# Lean model runner

The model is ordinary compiled Lean, and only the thin `main` that decodes a
case, calls the model and encodes the reply is generated. A project's build
files are maintained by a person, and a tool that rewrote them would make merge
conflicts nobody asked for. A missing toolchain means not set up yet, not
broken.
