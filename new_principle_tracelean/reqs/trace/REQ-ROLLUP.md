---
id: REQ-ROLLUP
title: Roll-up and coverage reporting
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  every_part_holds:
    text: The system shall let an aggregate over children hold a mark only when every counted part holds it, never by mean or majority.
    empty: Where there are no parts to aggregate, the system shall let the aggregate hold no mark.
  open_is_lower_bound: The system shall mark a roll-up over an unclaimed decomposition provisional, because a clause not yet written can only lower it.
  never_complete_when_open: The system shall never render a requirement with an unclaimed decomposition as finished.
  exempt_leaves_denominator: The system shall remove an exempted clause from the coverage denominator and the aggregate marks.
  partial_capped:
    text: The system shall keep a clause marked partial in the coverage denominator with its contribution capped below anything that ran.
    cap: The system shall never count a partial clause as tested or proved, so that it never meets a floor asking for either.
  untraced_counted: The system shall count a file that no annotation claims in the denominator of every per-file coverage figure that covers it.
  deterministic_order: The system shall compute a roll-up independently of the order in which requirements are declared or children are visited.
  counted_once:
    text: The system shall compute a requirement's figures over the set of requirements reachable from it by refinement, each counted once however many paths reach it.
    cycle: If a requirement is reached again through a cycle, then the system shall not count it again and shall not show it again beneath itself.
---

# Roll-up and coverage reporting

A percentage is what people quote without reading what produced it. If nobody
claimed the clauses exhaust a requirement, an unwritten clause has no evidence
and writing it can only bring the figure down, so the figure bounds the truth
from above and is shown as provisional. (The key keeps its old name.)

The roll-up is over a set, not a walk: in a diamond (B and C refine A, D refines
both) a walk sharing one visited set counts D under B only, and C reads higher
than its weakest part. Unclaimed code inside a claimed file is `REQ-LINECOV`'s
question. Removing a clause improves every percentage above it, which is why
exemption needs a reason, an approver and an expiry.
