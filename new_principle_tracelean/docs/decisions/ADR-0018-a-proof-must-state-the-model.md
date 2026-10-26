---
adr: 18
title: A proof must state the model, and strength is per model
status: accepted
affects: [REQ-STRENGTH, REQ-EVID, REQ-CHECK]
supersedes-in-part: [ADR-0014]
---

# ADR-0018 — A proof must state the model

## Context

`P` was earned by any `@proves` theorem that built without `sorry`. A check of
all 289 such theorems found one that holds of every function, 19 that never
mention their clause's model, 11 that reach it only through a wrapper, and
statements about types rather than functions. Mutation testing was considered
and rejected: deciding whether a surviving mutant is equivalent is undecidable,
and a theorem that pins the model's output answers the same question exactly.

Strength was asked per clause, from one `@specifies` predicate and one model,
while a clause rarely says enough to fix a model that several clauses share; and
the per-model obligation it also generated held `True` placeholders.

## Decision

- Lean decides. The tool ships a short Lean prelude defining one command,
  `#tracelean_model f [t₁, …, tₙ] (p)?`, and runs it on the built models. For
  each `tᵢ` it reports accepted with its axioms, or refused with one reason:
  `sorry`, `axiom X`, `silent`, `example`, `indirect X`, `vacuous`, `type`.
- The obligation for `f` is built by Lean from the accepted statements with
  `f` abstracted to `g`: `∀ g, S₁ g → … → Sₙ g → g = f`. A `@pins` theorem
  `p` on any of `f`'s clauses is checked against it by definitional equality.
- `@specifies` stays as a role for a readable `Prop` beside a model, used in
  theorem statements (`∀ x, Spec x (f x)`); pinning no longer depends on it.
- The verdicts are written to `.tracelean/proofs.json`; proof records carry the
  hash of the prelude as an input, so a change to the rules makes every proof
  record stale.

## Consequences

Clauses whose theorems are refused lose `P` until restated. A model that is a
type earns no `P`; its clause is judged. Strength moves from 277 clause plans to
one obligation per model function (183), shown on each of its clauses.
