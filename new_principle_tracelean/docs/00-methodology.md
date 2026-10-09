---
describes: [ARCH-EFFECT-LAW, ARCH-HONEST, REQ-EVID]
described_hash:
  ARCH-EFFECT-LAW: a52673a3000aa966
  ARCH-HONEST: 8760068412007801
  REQ-EVID: c3dfc3a07c7808a1
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
4. **A binding** in `.tracelean/drt.json`, making model and implementation answer
   the same questions.

Theorems about the model are `@proves`; the theorem that those properties
*determine* the model is `@pins`. Tests are `@tests`.

## Evidence

| Level | Means |
|---|---|
| `L1` | The annotation resolves. Nothing checked. |
| `L2` | A human judged requirement and model consistent. |
| `L3` | Differential testing found no disagreement, over a stated case count meeting a coverage floor. |
| `L4` | A Lean theorem discharges the property. |

Three bonds — requirement↔model, model↔implementation, model-property — graded
separately and aggregated by **minimum**. A proved model with no implementation
bound to it reads `L4 model · L1 code`. Averaging those is the most effective way
to build a dashboard that lies.

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

`@exempt` is reserved for things with no observable law worth stating.

## Requirement format

```markdown
---
id: REQ-EVID
title: Evidence algebra
refines: [ARCH-HONEST, ARCH-CORE-SHELL]
status: draft
decomposition: complete
clauses:
  ladder: Evidence levels are totally ordered.
  weakest_link: A link's assurance is the minimum over its bonds.
---
```

`decomposition: complete` claims the clauses exhaust the requirement, and is the
denominator of every percentage shown about it. It defaults to `open`, under
which only `≥ x%` may be rendered.

## Reading order

[01-bootstrap-ladder.md](01-bootstrap-ladder.md) ·
[02-scope.md](02-scope.md) ·
[03-doc-sync.md](03-doc-sync.md) ·
[decisions/](decisions/) ·
[progress.md](../work/progress.md)
