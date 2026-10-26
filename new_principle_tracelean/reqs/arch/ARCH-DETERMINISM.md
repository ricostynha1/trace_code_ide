---
id: ARCH-DETERMINISM
title: Derived artefacts are deterministic
status: approved
decomposition: open
clauses:
  same_input_same_bytes: When the same repository state is serialised, the system shall produce identical bytes.
  seeded_generation: The system shall generate cases reproducibly from their seed alone.
  stable_ordering: The system shall give every collection it writes to disk or compares a defined total order.
  replay_exact: When a recorded history is replayed, the system shall reach the state it was recorded from.
  no_ambient_time: The system shall not embed in a derived artefact a timestamp, path or machine identity that varies between runs of the same input.
---

# Derived artefacts are deterministic

A lockfile diff means something changed only if the same repository always
gives the same bytes, and a record of generated cases can be checked only if
its seed still reproduces them.
