---
id: REQ-PERSIST
title: Persistence and replay
refines: [ARCH-DETERMINISM, ARCH-EFFECT-LAW, REQ-CMD]
status: approved
decomposition: complete
clauses:
  replay_exact: When a recorded history is replayed, the system shall reach exactly the state it was recorded from.
  checkpoint_equivalent: When replaying from a checkpoint, the system shall reach the same state as replaying from the beginning.
  append_only: The system shall append to the recorded history and shall not rewrite it in place.
  truncated_is_reported: If a record ends mid-entry, then the system shall report it and replay up to the last complete entry.
  portable: The system shall replay a recorded history in a copy of the project moved elsewhere.
---

# Persistence and replay

A history that does not reproduce its own state is a log, not a record. A
checkpoint is a performance device, so it is specified as equivalent to
replaying from the start. A process killed mid-write is the case that actually
happens: replaying to the last complete entry and saying so is recoverable,
where accepting half an entry is not.
