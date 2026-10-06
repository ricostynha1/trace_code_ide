---
id: REQ-DRT-GEN
title: Case generation and shrinking
refines: [REQ-DRT, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  seed_reproduces: The same seed and schema shall produce exactly the same case sequence.
  fixed_stream: The generator's random stream shall be defined here and shall not change with a dependency version.
  edges_sampled: Declared boundary values shall be sampled heavily rather than drawn uniformly.
  shrink_terminates: Shrinking shall terminate.
  shrink_preserves: A shrunk case shall still produce the divergence it was shrunk from.
  shrink_minimal: Shrinking shall not stop while a strictly smaller diverging case is reachable by its reduction steps.
---

# Case generation and shrinking

`seed_reproduces` is not a convenience. A record claims *seed 7, twelve million
cases*, and that is auditable only while seed 7 means those cases. A generator
whose stream moves with a dependency update invalidates every record ever written.

`edges_sampled` fixes a failure that looks like success: a model branching at 5000
and 20000, tested with an unbounded integer drawn uniformly, is exercised almost
entirely above both, and a floor computed over the whole range still reports
itself met.

`shrink_preserves` makes shrinking trustworthy. A reduction that stops diverging
has not found a smaller case — it has found a different one.
