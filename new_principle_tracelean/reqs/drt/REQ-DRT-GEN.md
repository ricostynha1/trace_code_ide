---
id: REQ-DRT-GEN
title: Case generation and shrinking
refines: [REQ-DRT, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  seed_reproduces: When given the same seed and schema, the system shall produce exactly the same case sequence.
  fixed_stream: The system shall define its generator's random stream itself, so that the stream does not change with a dependency version.
  edges_sampled: The system shall sample declared boundary values heavily rather than draw them uniformly.
  shrink_terminates: When shrinking a case, the system shall terminate.
  shrink_preserves: When shrinking a case, the system shall produce only a case that still produces the divergence it was shrunk from.
  shrink_minimal: The system shall not stop shrinking while a strictly smaller diverging case is reachable by its reduction steps.
---

# Case generation and shrinking

A record claiming seed 7 and twelve million cases can be checked only while
seed 7 means those cases, so the stream is defined here. A model branching at
5000 and 20000 and tested with integers drawn uniformly is exercised almost
entirely above both. A reduction that stops diverging has found a different
case, not a smaller one.
