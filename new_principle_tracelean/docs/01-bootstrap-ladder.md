---
describes: [ARCH-SELFHOST, REQ-DRT-RUST]
described_hash:
  ARCH-SELFHOST: 4a50cc455df5b2ad
  REQ-DRT-RUST: 590a37eb03100b9a
---

# The bootstrap ladder

```
stage 0   ../tracelean          the existing IDE. Untraced. A tool, not a deliverable.
             │  scans, indexes, runs DRT
             ▼
stage 1   new_principle_tracelean    written under the methodology
             │  once its trace kernel passes
             ▼
stage 2   it traces itself
             │  compare stage-1 and stage-2 indexes
             ▼
          agreement ⇒ fixpoint-stable
```

## Stage 0

The existing tree can already scan annotations, resolve anchors, build an index,
generate and build a Lean model runner and run differential tests. It stays
untraced: a stage-0 compiler is allowed to be a binary somebody else built.

Everything this port writes is therefore constrained to what stage 0 parses.
Those formats were read out of the stage-0 source rather than invented, so the
new tree is a valid input to the old tool from the first commit
([ADR-0001](decisions/ADR-0001-bootstrap-on-stage0-formats.md)).

`tools/stage0-trace` is the driver that runs it.

## The one thing stage 0 cannot do

It refuses to differentially test Rust:

> differential testing has no runner for {language} implementations yet — only
> Python — `tracelean/core/src/drt/bind.rs:265`

The protocol is language-independent and the Lean side is already generated and
built; only the implementation-side runner is missing. Since this port is Rust,
nothing in it reaches L3 until that exists, so `REQ-DRT-RUST` is built first.

It is validated the only way a differential tester honestly can be:
differentially. A corpus answered by the shipped Python runner and by the new
Rust runner over equivalent implementations must agree.

## Stage 2 — the fixpoint

1. **Agreement.** The index stage 1 produced for this tree equals the one stage 2
   produces. Disagreement means one is wrong about a repository both can read.
2. **Idempotence.** A second run changes nothing.

Neither proves correctness. Both catch the errors a traceability tool makes about
the class of thing it is about.
