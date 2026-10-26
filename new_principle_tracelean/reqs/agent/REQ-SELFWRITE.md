---
id: REQ-SELFWRITE
title: Self-write suppression
refines: [REQ-MIRROR, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  own_writes_ignored: The system shall not ingest a change it wrote itself as an external change.
  suppression_is_consumed: The system shall apply a suppression to one observation, and shall not keep it for later ones.
  content_matched: The system shall match a suppression on what was written, and not on the path alone.
  unmatched_is_external: If an observation matches no pending self-write, then the system shall treat it as external.
  no_deadlock: If a self-write is never observed, then the system shall let its suppression expire rather than suppress a later change indefinitely.
---

# Self-write suppression

The mirror writes the real tree and the watcher watches it, so without
suppression the system sees its own write and oscillates or fills the history
with duplicates. Each clause is one way naive suppression fails, and all of them
are a pure predicate over an observation and the pending writes.
