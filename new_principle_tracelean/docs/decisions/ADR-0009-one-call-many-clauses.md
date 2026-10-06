---
adr: 9
title: A binding may name several clauses; a symbol may name only one function
date: 2026-09-16
status: accepted
affects: [REQ-DRT-BIND, REQ-CHECK]
---

## Context

`Unbound` — a clause with a model, an implementation, and nothing comparing
them — is the finding this project cares most about. After the first fifteen
bindings it stood at 26, and almost all of them were false: `suppress` realises
four clauses of `REQ-SELFWRITE`, and one differential run over it checks all
four. One binding per clause would have meant running the same comparison four
times and calling the repetition evidence.

## Decision

A binding entry may carry `also_checks`: other clauses of the same requirement
that *this one call* checks. The claim it makes is specific — a disagreement
about the named clause would show up in this call's output — and it is held to
the same rule as the primary clause: annotated `@drt`, or not claimed.

Where a clause is realised by a *different* function, it gets its own binding.
That is why `mutations`, `expire`, `chain`, `state_blocks`, `detail_ceiling` and
`still_applies_to` are bound separately rather than folded into the calls beside
them: agreement on `mirrored` does not check the mutation list, because two
different lists can drive a tree to the same state.

## Consequence, found immediately

`state_blocks` and `detail_ceiling` are named that way because binding
resolution matches a *symbol* inside a file, and both files already had a method
of the shorter name. Resolution silently took the method, which takes `&self`,
and only an arity check caught it.

That is the failure this project exists to prevent, in its own binding
mechanism: the wrong function called, compared, and reported as agreeing.
`signature::declarations` now counts declarations and `resolve` refuses an
ambiguous name rather than taking the first.
