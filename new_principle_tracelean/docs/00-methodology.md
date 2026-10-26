---
describes: [ARCH-EFFECT-LAW, ARCH-HONEST, REQ-EVID]
described_hash:
  ARCH-EFFECT-LAW: 760deab61fe27500
  ARCH-HONEST: b38bca7ade9341f0
  REQ-EVID: 015e7db9658ef75a
---

# Methodology

TraceLean defines what a *proper* project is: requirements with identity, a
formal model saying what each clause means, an implementation claiming to realise
it, and the claim checked rather than believed. The existing codebase defines
that and does not follow it. This project is TraceLean rebuilt under its own
rules — the same move as compiling a compiler with itself.

## The four artefacts

Every feature exists as four things, linked by annotations in comments, never by
filename or directory:

1. **A requirement document** — markdown with frontmatter carrying an immutable
   `id`. Identity is the `id`; the path is decoration. Requirements decompose
   into *clauses*, and a clause is what everything else attaches to.
2. **A Lean model** — `@models REQ-X.clause`.
3. **An implementation** — `@implements REQ-X.clause`.
4. **A binding**, making model and implementation answer the same questions. It
   is derived from the `@models` function and the Rust function marked `@drt`
   (else `@implements`); `.tracelean/drt.json` holds only what cannot be read
   off them.

Theorems about the model are `@proves`; the theorem that those properties
*determine* the model is `@pins`. Tests are `@tests`.

## Evidence

| Mark | Means |
|---|---|
| `A` | The annotation resolves. Nothing checked. |
| `J` | A human judged requirement and model consistent. |
| `T` | Differential testing found no disagreement, over a stated case count meeting a coverage floor. |
| `P` | A Lean theorem discharges the property. |

Each mark is read from its own bond — requirement↔model, model↔implementation,
model-property — and held or not on its own, written `AJTP` with a dash for each
gap. A proved model with no implementation bound to it reads `A--P`. Averaging,
or ranking the marks on a ladder, is the most effective way to build a dashboard
that lies: an aggregate holds a mark only when every part holds it.

A clause checked only by reading the tree (`@structural`, ADR-0012) has no model,
so T and P cannot apply: written `·`, done at `AJ··`, and leaving an aggregate's
T and P to the clauses they apply to.

## Modelling what Lean cannot run

A model need not *perform* an effect to *describe* it. Effectful operations are
axiomatised — opaque constants with stated laws — and differential testing checks
the real implementation against those laws.

```lean
axiom sandboxCopy : Tree → Tree
axiom sandboxCopy_preserves_protected :
  ∀ t p, isProtected p → (sandboxCopy t).at p = t.at p
```

Lean never runs it. The content of the claim is the law, and the law is
checkable. This is what stops the method applying only to arithmetic. See
[ADR-0002](decisions/ADR-0002-axiomatised-effects.md).

`@exempt` is reserved for things with no observable law worth stating. An effect
whose type already says everything, like a read typed `String → String`, needs no
law, and its requirement says so.

## Requirement format

```markdown
---
id: REQ-EVID
title: Evidence algebra
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: draft
decomposition: complete
clauses:
  marks: The system shall record four marks for a clause, each held on its own.
  mark_per_bond: The system shall read each mark from its own bond.
---
```

`decomposition: complete` claims the clauses exhaust the requirement, and is the
denominator of every percentage shown about it. Clauses are written in EARS
([ADR-0017](decisions/ADR-0017-clauses-in-ears.md)). It defaults to `open`, under
which a figure is rendered `≤ x% (provisional)`: an unwritten clause has no
evidence, so writing it can only lower the figure.

## Reading order

[01-bootstrap-ladder.md](01-bootstrap-ladder.md) ·
[02-scope.md](02-scope.md) ·
[03-doc-sync.md](03-doc-sync.md) ·
[decisions/](decisions/) ·
[what is left](../work/action-plan.md)
