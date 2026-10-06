---
adr: 2
title: Effectful operations are axiomatised in Lean, not exempted
status: accepted
affects: [ARCH-CORE-SHELL, ARCH-EFFECT-LAW, REQ-SBX, REQ-MIRROR, REQ-PERSIST]
---

# ADR-0002 — Axiomatised effects

## Context

Lean cannot spawn `bwrap`, watch a filesystem, issue a syscall or receive a key
event. Most of an IDE is made of exactly those things.

The obvious conclusion is that effectful features cannot be modelled and must be
marked `@exempt`. Taken seriously that exempts the sandbox, the mirror, the
watcher, persistence and every input path — which is to say, it applies the
methodology only to the parts of the system that were never going to be wrong in
an interesting way, and leaves the parts that actually break unmodelled.

## Decision

A model does not have to *perform* an effect in order to *describe* it.

Effectful operations are declared in Lean as **axiomatised constants** — opaque,
with no definition — and the model states the laws they must obey. The
implementation performs the real effect. Differential testing then checks the
implementation's observed behaviour against the stated laws on generated cases.

```lean
axiom applyMutation : Tree → Mutation → Tree

axiom apply_protected :
  ∀ t m, isProtected m.path → applyMutation t m = t

axiom apply_idempotent :
  ∀ t m, applyMutation (applyMutation t m) m = applyMutation t m
```

Lean never runs `applyMutation`. It does not have to: the content of the claim is
in the laws, and the laws are checkable. An implementation that writes through to
a protected path produces a divergence exactly like an arithmetic mismatch.

The split is therefore not "pure code is modelled, effectful code is exempt", but:

- **the decision** — which paths are protected, what mutation a diff implies,
  which transition a key causes — is an ordinary total function, modelled and
  compared directly;
- **the effect** — the write, the spawn, the watch — is axiomatised, and what is
  checked is the law relating its inputs to its observable results.

## Consequences

Effectful subsystems reach L3. The sandbox path policy, the mirror's
diff-to-command translation, self-write suppression and replay determinism are
all modelled and differentially tested rather than exempted.

`@exempt` is reserved for things with no observable law worth stating — pixel
layout, whether a window appeared. That is a much smaller set than the naive
reading implied, and keeping it small is what keeps grey meaningful.

A cost: an axiom is an assumption, and a wrong axiom is believed by the kernel.
Axioms are therefore confined to `formal/TraceLean/Effects.lean`, so the full set
of things this project asks Lean to take on faith is one file somebody can read
in a sitting. Anything provable is proved rather than axiomatised.

## Rejected

*Model effects in a monad and interpret it.* A free monad or `IO` embedding would
let Lean describe programs rather than laws. It is more faithful and much more
work, and it moves the checking problem rather than solving it: the interpreter
then needs its own correspondence to reality. Laws plus differential testing get
the same guarantee against the real implementation, which is the thing that
actually ships.
