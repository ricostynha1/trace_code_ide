---
id: REQ-DRT
title: Differential testing
refines: [ARCH-EFFECT-LAW, ARCH-DETERMINISM, ARCH-HONEST]
status: approved
decomposition: open
clauses:
  same_question: The system shall ask both sides of a binding identical cases, in identical order.
  error_is_an_answer: If one side fails on a case, then the system shall compare the failure against the other side's answer, and shall not treat the run as aborted.
  divergence_reported: When the sides disagree, the system shall report the case that produced the disagreement, reduced to a minimal form.
  falsification_only: If a run finds no disagreement, then the system shall record it as evidence at L3 and never as proof.
  case_count_stated: The system shall state in evidence its case count and its seed, which fix the cases it ran, and shall write the evidence only once the coverage those cases reached meets every floor its binding declares.
---

# Differential testing

The system never parses the implementation, only runs it, so `unsafe`,
generics, FFI and third-party libraries do not matter. Agreement falsifies; it
does not prove, which is why this evidence sits below a Lean proof. Both sides
rejecting an input is agreement, but one rejecting and one returning a value is
a divergence, often the most interesting one in the run.
