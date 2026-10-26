---
id: ARCH-CORE-SHELL
title: Decision and effect are separate
status: approved
decomposition: open
clauses:
  decision_total: The system shall expose the decision of every feature as a total function from data to data, with no IO, clock, randomness or global state.
  shell_thin: The system shall keep out of its effectful shell any branching that a decision function could have made.
  decision_public: The system shall make every decision function reachable as a public library item.
  no_hidden_input: The system shall compute the result of a decision function from its declared arguments alone.
---

# Decision and effect are separate

Differential testing asks a model and an implementation the same question. A
decision that reads the clock, walks a directory or consults a global cache
cannot be asked one, so the part of a feature that decides is kept apart from
the part that acts, and is public so that a conformance runner can call it.
