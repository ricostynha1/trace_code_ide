---
adr: 3
title: The judge is a person; TraceLean makes no model call
status: accepted
affects: [ARCH-NO-DRIVING, REQ-JUDGE]
---

# ADR-0003 — The judge is a human

## Context

Nothing can bind a requirement written in English to a formal model by execution:
there is no oracle for prose. Stage-0 TraceLean handled that by sending both to an
LLM and parsing a verdict, with an executed witness bolted on because the judge's
claims about Lean could not be trusted on their own.

That is an independent model call, and this port removes model calls.

## Decision

The judge is a person. TraceLean renders the requirement clause beside the model
and beside any divergence differential testing found, and the human records
`agrees`, `drift` or `unmodelable`.

TraceLean additionally **exports a judging prompt** — the same structured prompt
stage 0 would have sent — for the human to copy and paste into whatever agent
they choose. Whatever comes back is advice. The human enters the decision, and
the decision is attributed to them.

There is no button that calls a model, and no network code in this project.

## Consequences

The evidence record is unchanged in shape and still sits at L2, because L2 always
meant "somebody judged this", never "this was checked". What changes is the value
of `source`: `human` rather than `api`, with the judge identified.

Three safety rules survive the change and are stated in `REQ-JUDGE`, because they
were never about the judge being a machine:

- a judgement can write L2 and nothing more — it can never promote a link to
  conformant or proved;
- `unmodelable` produces a proposal, never a mutation;
- a judgement is invalidated when the requirement text or the model changes,
  since it was about the pair.

The witness-execution machinery is dropped with the LLM. It existed to catch a
model confabulating about Lean, and a human reading the model does not need a
second system to check that they read it.

## Rejected

*Keep an optional provider client behind a setting.* It reintroduces the entire
surface the scope cut removed — keys, spend caps, retries, cost accounting — for
one call. The copyable prompt gets the same help from an agent with none of it.
