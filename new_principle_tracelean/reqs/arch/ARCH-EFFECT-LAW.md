---
id: ARCH-EFFECT-LAW
title: Effects are modelled by their laws
status: approved
decomposition: open
clauses:
  axiomatised: The system shall declare each effectful operation in Lean as an axiomatised constant with no definition.
  law_stated: The system shall state, for every axiomatised operation, at least one law relating its inputs to its observable result.
  law_checked: The system shall check every stated law against the real implementation by differential testing.
  axioms_confined: The system shall keep all of its axioms in one module.
  exempt_last: The system shall mark a feature exempt only when no observable law about it is worth stating.
---

# Effects are modelled by their laws

Lean cannot run an effect, but it can state the laws the effect obeys, and
differential testing checks them against what the implementation did. The
kernel believes a wrong axiom, so the axioms are few and sit in one module,
where they can be read in one sitting. See
[ADR-0002](../../docs/decisions/ADR-0002-axiomatised-effects.md).

One effect, `readTranscript`, has no law. Its type, `String → String`, already
says a read changes nothing.
