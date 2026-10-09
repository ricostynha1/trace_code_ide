---
id: REQ-DRT-COVER
title: Coverage qualifies a passing run
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: open
clauses:
  floor_stated: A binding shall state the coverage its runs must reach.
  floor_unmet_is_not_pass: A run below its floor shall not produce L3 evidence.
  law_coverage: A law shall record whether generated cases reached the situation it constrains.
  vacuous_named: A law satisfied only because no case reached its precondition shall be reported as vacuous, not as passing.
  classes_reached: The argument classes a run's cases reach shall be listed from the binding's input shape, so a class no case reached is a named gap.
  lines_run: The implementing lines a run executed shall be recorded, so a line no case ran is a named gap.
  waiver_reasoned: A coverage waiver shall carry a reason, and a waiver without one shall not count.
  waiver_unused_reported: A waiver covering a situation the run did reach shall be reported as unused.
---

# Coverage qualifies a passing run

A run that found nothing must have had a chance of finding something — the same
qualification a proof needs, and why `REQ-STRENGTH` exists for L4 and this for
L3.

The target is complete coverage: every argument class reached and every
implementing line run. Anything less is waived with a reason (ADR-0016).
A per-binding floor alone is insufficient for laws: "a protected path is never
written" holds perfectly in a run where no case named a protected path, so the
floor is also stated per law, over its precondition.
