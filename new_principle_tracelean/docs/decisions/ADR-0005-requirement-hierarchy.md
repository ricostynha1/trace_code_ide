---
adr: 5
title: Architectural requirements are requirements, not prose
status: accepted
affects: [ARCH-CORE-SHELL, ARCH-EFFECT-LAW, ARCH-DETERMINISM, ARCH-HONEST, ARCH-NO-DRIVING, ARCH-SELFHOST]
---

# ADR-0005 — Architectural requirements are first-class

## Context

Cross-cutting properties — determinism, honest reporting, the refusal to drive an
agent — are normally written in an architecture document. Architecture documents
are not checked, are not linked to code, and are the first thing to become
false.

Meanwhile the feature requirements need a root. TraceLean's `refines:` field is a
DAG, not a tree, and nothing says its roots have to be features.

## Decision

Cross-cutting properties are ordinary requirement documents with `ARCH-` ids, in
`reqs/arch/`. They carry clauses, take annotations, accumulate evidence and are
graded, exactly like feature requirements.

Every feature requirement `refines:` the architectural requirements it is
answerable to. `REQ-MIRROR` refines `ARCH-EFFECT-LAW` because its effects are
axiomatised; `REQ-ROLLUP` refines `ARCH-HONEST` because its whole content is the
refusal to state a number it cannot support.

## Consequences

The refinement DAG becomes the architecture diagram, and it is a checked one:
`refines:` naming a requirement that does not exist is a `DanglingRefines`
finding, and a cycle is a `RefinesCycle` finding.

An architectural principle can be *violated visibly*. `ARCH-NO-DRIVING` has
clauses, and code that launches an agent has no way to satisfy them — the
principle stops being a paragraph somebody may not have read and becomes a
clause with no conformant implementation.

Architectural requirements are mostly `decomposition: open`. They are claims about
the whole system, and claiming their clauses exhaust them would license a
completeness percentage nobody should trust.

## Rejected

*Tags or a category field on feature requirements.* A tag cannot be refined,
cannot carry a clause, cannot take evidence, and cannot be violated — it is a
label, and the thing being described here is a claim.
