---
id: REQ-DRT-LEAN
title: Lean model runner
refines: [REQ-DRT-PROTO, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  generated: The model side shall be generated as a Lean package and built, not hand-written by the project.
  project_untouched: Generation shall not modify the project's own Lean package or its build configuration.
  toolchain_explained: A missing Lean toolchain shall be reported as a distinct, explained state rather than as a failure of the model.
  compiled_not_interpreted: The model shall be compiled, so that a case costs microseconds and large runs are affordable.
  encoding_declared: The JSON encoding of the model's inputs and outputs shall follow one declared convention that the schema grammar describes.
---

# Lean model runner

The model is ordinary compiled Lean, so a case costs microseconds and millions
fit in a run. Only the thin `main` that decodes a case, calls the model and
encodes the reply has to be generated.

`project_untouched` is why the generated package is separate: a project's build
files are something a person maintains, and a tool that rewrites them creates
merge conflicts nobody signed up for.

`toolchain_explained` is the difference between "this is broken" and "this is not
set up yet".
