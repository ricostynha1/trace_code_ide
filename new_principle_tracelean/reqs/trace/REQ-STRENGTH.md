---
id: REQ-STRENGTH
title: Spec strength
refines: [ARCH-HONEST, REQ-EVID]
status: approved
decomposition: complete
clauses:
  obligation_generated: For each modelled clause the system shall generate the obligation that the proved properties determine the model.
  not_proved_by_us: The system shall generate the obligation and shall not attempt to discharge it.
  open_is_the_default: A clause with no pinning attempt shall report as open, and open shall not be presented as a pass.
  attempted_distinguished: An obligation stated but unfinished shall be distinguished from one never attempted and from one discharged.
  nondeterministic_declared: A model that cannot be pinned shall be declarable as such with a reason, and shall then be excluded rather than reported as open forever.
  qualifies_proof: A proof level shall be presented together with its strength, in the way a testing level is presented together with its coverage.
---

# Spec strength

`@proves` says the model has a property, not how much that property rules out.
`discount s ≤ s` is a genuine machine-checked theorem that the constant-zero
function also satisfies. Unqualified, a proof level is what a testing level would
be without a coverage floor.

The relational specification is the *set of proved theorems*, and the question is
whether everything satisfying them is this model:

```
∀ f g, Spec f → Spec g → f = g
```

Provable, and the theorems pin the model. Unprovable, and the slack is behaviour
the proofs never ruled out.

The system states the obligation; discharging it is the human's part, and while
nobody has, the report is "open" — never "fine". `nondeterministic_declared`
keeps the column meaningful: an obligation that can never close teaches everyone
to ignore it.
