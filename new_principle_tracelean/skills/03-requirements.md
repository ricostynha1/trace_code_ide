# Requirements

```markdown
---
id: REQ-CONVERT
title: Converting between temperature scales
refines: [ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  scales_round_trip: Converting a temperature to the other scale and back shall give what went in.
---

# Converting between temperature scales

Why these clauses; the failure each prevents; what is not claimed.
```

- **`id`** is the identity, never the path.
- **`refines`** builds a graph; cycles and unknown ids are reported.
- **`decomposition`**: `complete` (clauses exhaust it) or `open` (coverage is
  a lower bound; usual for architecture).

## A good clause

- One sentence with **"shall"**; **one claim** (an "and" of two obligations is
  two clauses).
- **What**, not how.
- Prefer one you could write a function for (gets a model and L3).
- Key named for the failure it prevents: `escape_pops_one`, not
  `escape_behaviour`.

## Changing requirements

- **Adding**: check the index first — usually you want a clause, not a
  document. Each new clause is a new `Unmodeled`: work you signed up for.
- **Changing** a clause makes its evidence stale, and documents describing it go
  into review. Never reword a clause to fit the code.
- **Deleting**: `grep` its id first; retarget or delete every annotation, or
  they become `Dangling`.

Before any of these, `tracelean-trace . --context REQ-X --parts all` shows
what the change touches.
