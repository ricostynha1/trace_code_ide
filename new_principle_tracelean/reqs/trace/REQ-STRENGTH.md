---
id: REQ-STRENGTH
title: Spec strength
refines: [ARCH-HONEST, REQ-EVID]
status: approved
decomposition: complete
clauses:
  obligation_generated: The system shall generate, for each modelled clause, the obligation that the proved properties determine the model.
  not_proved_by_us: The system shall generate the obligation and shall not attempt to discharge it.
  open_is_the_default: Where a clause has no pinning attempt, the system shall report it as open, and shall not present open as a pass.
  attempted_distinguished: The system shall distinguish an obligation stated but unfinished from one never attempted and from one discharged.
  nondeterministic_declared: Where a model cannot be pinned, the system shall let it be declared as such with a reason, and shall then exclude it rather than report it as open forever.
  qualifies_proof: The system shall present the proved mark together with its strength, in the way the tested mark is presented together with its coverage.
  per_input: Where a clause has both a specification predicate (its one specifies declaration) and a model function (its one models declaration), the system shall make the obligation that the function meets the predicate and that no input has two answers the predicate accepts.
  inputs_counted: The system shall quantify the obligation over every explicit argument the model takes, whether named in a binder or given only as an arrow of its type.
  kernel_decides: The system shall report a clause pinned only when Lean accepted its pinning theorem, stated as the obligation, without error and without depending on sorry.
  verdict_kept: When the theorem or a declaration a verdict was given for changes, the system shall return the clause to attempted.
---

# Spec strength

`@proves` says the model has a property, not how much that property rules out:
`discount s ≤ s` is a machine-checked theorem the constant-zero function also
satisfies. Unqualified, a proof is what a test would be without a coverage
floor. The specification is the set of proved theorems, and the question is
whether everything satisfying them is this model:

```
∀ f g, Spec f → Spec g → f = g
```

With a specification predicate `P` beside the model `f`, it is asked per input,
`(∀ x, P x (f x)) ∧ (∀ x y1 y2, P x y1 → P x y2 → y1 = y2)`, and
`tracelean-trace . --pins` checks each `@pins` theorem against exactly that
statement. Discharging it is the human's part, and until somebody has, the
report is open, never fine; an obligation that can never close would teach
everyone to ignore the column.
