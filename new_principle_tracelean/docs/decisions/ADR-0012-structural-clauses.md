---
adr: 12
title: A clause about the tree is structural, not exempt
status: accepted
affects: [REQ-ANNOT, REQ-CHECK, ARCH-EFFECT-LAW, ARCH-NO-DRIVING, ARCH-SELFHOST]
---

# ADR-0012 — Structural clauses

## Context

Roughly a third of this project's clauses are not about a value. *Nothing here
calls a model.* *A link exists only where somebody wrote one.* *The index this
system produces for its own tree equals the one the bootstrap tool produces.*

There is no `f : Input → Output` to write in Lean and no second implementation
to compare it against. The checker nonetheless demanded a model for every
clause, which left three bad options: write a Lean function that restates the
sentence and proves nothing, mark thirty clauses `@exempt` and drop them out of
the denominator, or let `Unmodeled` sit at thirty forever and stop meaning
anything.

## Decision

A fourth qualifier, `@structural(reason=...)`.

A structural clause is checked by a test that **reads the repository** — the
suite in `crates/core/tests/architecture.rs`, and its neighbours. It requires a
`@tests` and nothing else. It does not require a model or a binding, because
neither can exist, and it does not require an `@implements` either: there is
nothing to point one at. The tree realises the property by being the shape it
is. It **stays in the coverage denominator**: a project that
quietly dropped its own architectural constraints out of its figures would be
flattering itself, which is the failure `ARCH-HONEST` exists to prevent.

It caps at L2. A structural check is a check of *this tree*, not of a law over
all inputs, and the ladder already has a rung for "somebody established this by
reading rather than by execution".

The `reason=` is required. A structural clause that does not say why there is no
law to state is indistinguishable from one nobody got round to modelling, and
`qualifier_kinds` reports it as an unsound qualifier.

## Why not `@exempt`

`ARCH-EFFECT-LAW.exempt_last` reserves exemption for things with no observable
law worth stating — pixel layout, whether a window appeared. These clauses have
a law and it is checked; what they lack is a *second implementation to compare
against*. Calling that an exemption would make grey mean two different things,
and the one it already means is the one worth keeping rare.

## Consequences

`Unmodeled` goes back to meaning *somebody should write a model for this*. The
structural clauses are visible as structural, in the denominator, capped at the
level their evidence supports.

The risk is the obvious one: `@structural` is easier to write than a model, so
it can be used to make a finding go away. The `reason=` and the required test
are what make that expensive — a structural claim with no test is reported as
untested, and a reason that does not survive being read is a reviewable thing
in a way an absent model is not.
