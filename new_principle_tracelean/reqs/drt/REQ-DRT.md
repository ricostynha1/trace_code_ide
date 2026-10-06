---
id: REQ-DRT
title: Differential testing
refines: [ARCH-EFFECT-LAW, ARCH-DETERMINISM, ARCH-HONEST]
status: approved
decomposition: open
clauses:
  same_question: Both sides of a binding shall be asked identical cases, in identical order.
  error_is_an_answer: A failure on one side shall be compared against the other side's answer, not treated as an aborted run.
  divergence_reported: A disagreement shall be reported with the case that produced it, reduced to a minimal form.
  falsification_only: A run finding no disagreement shall be recorded as evidence at L3 and never as proof.
  case_count_stated: Evidence shall state its case count, its seed, and the coverage it reached.
---

# Differential testing

The bond between model and implementation, and what makes the design
language-independent: TraceLean never parses the implementation, only runs it, so
`unsafe`, generics, FFI and third-party libraries are irrelevant.

It buys falsification, not proof, which is why the evidence sits below a Lean
proof and why case count and coverage are recorded beside the result.

`error_is_an_answer` is the clause easily got wrong. Both sides rejecting an input
is agreement; one rejecting and one returning a value is a divergence, often the
most interesting in the run. Treating a failure as an aborted case hides exactly
those.
