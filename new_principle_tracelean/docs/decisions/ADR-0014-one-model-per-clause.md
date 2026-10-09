---
adr: 14
title: One model per clause, and a specification is its own role
status: accepted
affects: [REQ-ANNOT, REQ-CHECK, REQ-STRENGTH, REQ-JUDGE]
---

# ADR-0014 — One model per clause

## Context

`@models` was used for five relationships: the function answering a clause, a
`Prop` specification (from pinning), an operation that must keep a property, a
type the clause mentions, and every member of a family. 79 clauses carried
several. Pinning chose its specification and its model by position among them,
and `--judge` showed whichever came first.

## Decision

- `@models REQ-X.c` means exactly *the function that computes what the clause
  talks about*. At most one per clause.
- A `Prop`-valued specification of that function is `@specifies REQ-X.c`. At most
  one per clause. It is not a model: it does not make a clause modelled.
- At most one `@pins` per clause.
- Pinning takes the spec from `@specifies` and the model from `@models`; nothing
  is chosen by position.
- `--judge` shows the model and the spec.
- The checker reports a second of any of the three as `SeveralModels`,
  `SeveralSpecs` or `SeveralPins`, naming every declaration. These are warnings,
  not blocking, until the 79 clauses are sorted (action plan §7); then they
  become errors.

The other three uses take no annotation: a kept property is a theorem
(`@proves`), a type is vocabulary, and a family is a universal claim. A clause
that still seems to need two functions says two things and is split.

## Consequences

A spec gets the model's chip (`M`), since it is the other half of the model.
An annotation in a constructor's doc comment binds to its inductive, not to the
next declaration in the file.
