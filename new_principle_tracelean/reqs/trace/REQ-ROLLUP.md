---
id: REQ-ROLLUP
title: Roll-up and coverage reporting
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  min_not_mean: An aggregate over children shall take the minimum assurance, never the mean.
  open_is_lower_bound: A roll-up over an unclaimed decomposition shall be a lower bound on the true value.
  never_complete_when_open: A requirement with an unclaimed decomposition shall never render as finished.
  exempt_leaves_denominator: An exempted clause shall leave the coverage denominator, and a partial clause shall remain in it with a capped contribution.
  untraced_counted: Code that no annotation claims shall appear in the denominator of any coverage figure that covers its file.
  deterministic_order: Roll-up shall not depend on the order in which children are visited.
---

# Roll-up and coverage reporting

Where a wrong number does the most damage, because a percentage is the output
people quote without reading what produced it.

`open_is_lower_bound` and `never_complete_when_open` are one honesty rule at two
levels: if nobody claimed the clauses exhaust the requirement, any figure over
them is a lower bound, and rendering it exactly converts an unknown denominator
into a confident number.

`exempt_leaves_denominator` is why exemption needs a reason, an approver and an
expiry — removing a clause improves every percentage above it, making it the most
attractive available lie.
