---
id: ARCH-EFFECT-LAW
title: Effects are modelled by their laws
status: approved
decomposition: open
clauses:
  axiomatised: An effectful operation shall be declared in Lean as an axiomatised constant with no definition.
  law_stated: Every axiomatised operation shall carry at least one stated law relating its inputs to its observable result.
  law_checked: Every stated law shall be checked against the real implementation by differential testing.
  axioms_confined: All axioms shall live in one module, so the project's assumptions can be read in one sitting.
  exempt_last: A feature shall be marked exempt only when no observable law about it is worth stating.
---

# Effects are modelled by their laws

Lean cannot spawn a process or watch a filesystem. It does not follow that such
features cannot be modelled — only that the model cannot *execute* them.

An effectful operation is declared as an opaque constant, and the model states
the laws it must obey: a sandbox copy preserves protected paths, applying a
mutation twice equals applying it once. Differential testing runs the real
implementation and checks the laws hold of what it did.

A wrong axiom is believed by the kernel, so they are confined to one module and
kept few. Anything provable is proved instead.

See [ADR-0002](../../docs/decisions/ADR-0002-axiomatised-effects.md).
