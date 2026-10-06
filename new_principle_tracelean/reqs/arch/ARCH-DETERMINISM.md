---
id: ARCH-DETERMINISM
title: Derived artefacts are deterministic
status: approved
decomposition: open
clauses:
  same_input_same_bytes: The same repository state shall serialise to identical bytes.
  seeded_generation: Case generation shall be reproducible from its seed alone.
  stable_ordering: Every collection written to disk or compared shall have a defined total order.
  replay_exact: Replaying a recorded history shall reach the state it was recorded from.
  no_ambient_time: A derived artefact shall not embed a timestamp, path or machine identity that varies between runs of the same input.
---

# Derived artefacts are deterministic

A lockfile diff means something changed only if the same repository always
produces the same bytes. Otherwise the diff is noise and people stop reading it.

Evidence makes the point sharper. A record claiming *twelve million cases, seed
7, no divergence* is auditable only if seed 7 still reproduces those cases.
Non-reproducible evidence is not weak evidence; it is a claim nobody can ever
check, which is worse, because it looks like one.
