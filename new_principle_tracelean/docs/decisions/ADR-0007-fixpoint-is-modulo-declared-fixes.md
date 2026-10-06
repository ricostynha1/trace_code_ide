---
adr: 7
title: The stage-2 fixpoint is agreement modulo named, justified divergences
status: accepted
affects: [ARCH-SELFHOST, REQ-ANCHOR]
---

# ADR-0007 — The fixpoint is modulo declared fixes

## Context

`ARCH-SELFHOST.fixpoint` asks that the index the ported kernel produces for this
tree equal the one the bootstrap tool produces. The two agree on every link —
the same set of claims, anchored to the same declarations — and differ only in
how some anchors are spelled:

| Stage 0 | The port |
|---|---|
| `anchor.rs::impl Anchor::ident` | `anchor.rs::Anchor::ident` |
| `Evidence.lean::bondLevel` | `Evidence.lean::TraceLean::bondLevel` |
| `Anchor.lean::@file` | `Anchor.lean::` |
| `doclink.rs::State` | `doclink.rs::State::blocks` |
| `Checker.lean::Kind` | `Checker.lean::Kind.progress` |

The second is a defect in stage 0. The Lean declaration genuinely is
`TraceLean.bondLevel`; stage 0 drops the namespace because it treats a scope as
something a node *contains*, and Lean's `namespace … end` is a pair of siblings.
Two declarations with the same name in different namespaces would collide there.
Since one side simply lacks the scope, Lean anchors are compared by the
declaration's own name within its file rather than by rewriting one spelling
into the other.

The last two are the same defect seen twice: stage 0 stops at the type or the
namespace that contains a member, so every method of a type shares one anchor.
The port treats a Rust `impl` block as a scope rather than a declaration
(ADR-0011) and reads a Lean `def Kind.progress` as one dotted name, so it names
the member. A stage-0 anchor is therefore satisfied by a port anchor that
*extends* it; the member's name cannot be recovered from the type's, so this is
matched rather than rewritten.

## Decision

The fixpoint criterion is agreement **modulo a declared list of divergences**,
each carrying the reason the port is right. The list lives in the comparison test
(`crates/core/tests/fixpoint.rs`), so a new divergence fails while a known one
does not.

The current entries are the ones above. The port keeps its spelling in each.

## Consequences

An unqualified "the outputs are identical" was never going to be true of a port
that fixes bugs, and treating it as the criterion would have forced the port to
reproduce a defect in order to pass its own acceptance test.

The cost is that the criterion is only as good as the honesty of the list. It is
kept short, each entry states why the port is right rather than merely different,
and an entry is removed rather than reworded when it stops applying.

## Rejected

*Match stage 0 exactly until stage 2.* Reproducing the namespace defect would put
wrong anchors in the lockfile — anchors that collide — for the sake of a
comparison, which inverts what the comparison is for.
