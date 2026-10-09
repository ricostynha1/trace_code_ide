---
id: REQ-ROLLUP
title: Roll-up and coverage reporting
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  min_not_mean:
    text: An aggregate over children shall take the minimum assurance, never the mean.
    empty: With no parts to aggregate, the aggregate shall be L1.
  open_is_lower_bound: A roll-up over an unclaimed decomposition shall be marked provisional, because a clause not yet written can only lower it.
  never_complete_when_open: A requirement with an unclaimed decomposition shall never render as finished.
  exempt_leaves_denominator: An exempted clause shall leave the coverage denominator and the assurance.
  partial_capped:
    text: A clause marked partial shall remain in the coverage denominator with its contribution capped below the level at which anything ran.
    cap: The cap shall be L2, so a partial clause never meets a floor of L3 or above.
  untraced_counted: A file that no annotation claims shall appear in the denominator of every per-file coverage figure that covers it.
  deterministic_order: Roll-up shall not depend on the order in which requirements are declared or children are visited.
  counted_once:
    text: A requirement's figures shall be computed over the set of requirements reachable from it by refinement, each counted once however many paths reach it.
    cycle: A requirement reached again through a cycle shall not be counted again, and shall not be shown again beneath itself.
---

# Roll-up and coverage reporting

Where a wrong number does the most damage, because a percentage is the output
people quote without reading what produced it.

`open_is_lower_bound` and `never_complete_when_open` are one honesty rule at two
levels: if nobody claimed the clauses exhaust the requirement, any figure over
them is provisional — an unwritten clause has no evidence, so writing it can
only bring the figure down — and rendering it exactly converts an unknown
denominator into a confident number. (The key keeps its old name; the figure
bounds the truth from above.)

`counted_once` is why the roll-up is over a set, not a walk: in a diamond (B
and C refine A, D refines both) a walk sharing one visited set counts D under B
only, and C reads higher than its weakest part. `untraced_counted` is per file;
unclaimed code inside a claimed file is `REQ-LINECOV`'s question.

`exempt_leaves_denominator` is why exemption needs a reason, an approver and an
expiry — removing a clause improves every percentage above it, making it the most
attractive available lie.
