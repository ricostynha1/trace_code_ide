---
adr: 17
title: Clauses are written in EARS, and the prose says only why
status: accepted
affects: [REQ-REQDOC]
---

# ADR-0017 — Clauses in EARS

## Context

Clause texts had grown reasons, history and asides, and the prose beneath them
read as an essay: which key kept an old name, which failure some other
requirement owns, why a design was not chosen. A reader looking for what is
required had to separate it from why, and a judge comparing a model with a
clause was comparing it with the reasons too.

## Decision

Every clause is one sentence in one of the five patterns of EARS, the Easy
Approach to Requirements Syntax (Mavin et al., 2009):

| Pattern | Form |
|---|---|
| ubiquitous | The system shall *response*. |
| event | When *trigger*, the system shall *response*. |
| state | While *state*, the system shall *response*. |
| unwanted | If *condition*, then the system shall *response*. |
| optional | Where *feature*, the system shall *response*. |

The subject is always "the system", so a clause never hides who must act. A
clause states one response, and its reason does not go in it. A narrowing is a
clause in the same patterns, fixing one answer the clause leaves open.

The prose under a requirement says, in a few sentences, what failure the
clauses prevent. It does not record history, other requirements' business, or
alternatives.

## Consequences

Rewording a clause re-opens its judgement, its tests and its proofs, because
its evidence rests on its text. The rewrite was done once for every clause and
all of its evidence earned again; the meaning of no clause changed.
