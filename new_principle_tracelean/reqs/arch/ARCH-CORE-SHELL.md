---
id: ARCH-CORE-SHELL
title: Decision and effect are separate
status: approved
decomposition: open
clauses:
  decision_total: Every feature exposes its decision as a total function from data to data, with no IO, clock, randomness or global state.
  shell_thin: The effectful shell shall contain no branching that a decision function could have made.
  decision_public: A decision function shall be reachable as a public library item, so it can be called by a conformance runner.
  no_hidden_input: A decision function's result shall depend only on its declared arguments.
---

# Decision and effect are separate

Differential testing compares two implementations by asking the same question. A
function that reads the clock, walks a directory or consults a global cache
cannot be asked one — its answer depends on things the question did not contain.

So the part of a feature that *decides* is separated from the part that *does*.
"Which paths may the sandbox write" becomes a function from a path to a
classification, not a condition buried in the code that builds a container
command line. Effects that carry a law rather than a decision are covered by
`ARCH-EFFECT-LAW`.

Without this the models would be real and would describe a tenth of the code.
