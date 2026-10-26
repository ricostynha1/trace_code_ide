---
id: REQ-DRT-COVER
title: Coverage qualifies a passing run
refines: [REQ-DRT, ARCH-HONEST]
status: approved
decomposition: open
clauses:
  floor_stated: The system shall have every binding state the coverage its runs must reach.
  floor_unmet_is_not_pass: If a run is below its floor, then the system shall not produce L3 evidence from it.
  law_coverage: The system shall record, for each law, whether generated cases reached the situation it constrains.
  vacuous_named: If a law holds only because no case reached its precondition, then the system shall report it as vacuous, not as passing.
  classes_reached: The system shall list the argument classes a run's cases reach from the binding's input shape, so that a class no case reached is a named gap.
  lines_run: The system shall record the implementing lines a run executed, so that a line no case ran is a named gap.
  waiver_reasoned: The system shall require a coverage waiver to carry a reason, and shall not count a waiver without one.
  waiver_unused_reported: If a waiver covers a situation the run did reach, then the system shall report the waiver as unused.
---

# Coverage qualifies a passing run

A run that found nothing must have had a chance of finding something. The
target is complete coverage, every argument class reached and every
implementing line run, and anything less is waived with a reason. A law such
as "a protected path is never written" holds in a run where no case named a
protected path, so a floor is also stated per law, over its precondition.
