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
---

# Coverage qualifies a passing run

A run that found nothing must have had a chance of finding something — the same
qualification a proof needs, and why `REQ-STRENGTH` exists for L4 and this for
L3.

`law_coverage` and `vacuous_named` are open work, and the reason this is `draft`.
Once effects are checked by their laws, a per-binding floor is insufficient: "a
protected path is never written" is satisfied perfectly by a run where no case
named a protected path. The floor must be stated per law, over that law's
precondition, and that is not yet designed.
