---
id: REQ-PERSIST
title: Persistence and replay
refines: [ARCH-DETERMINISM, ARCH-EFFECT-LAW, REQ-CMD]
status: approved
decomposition: complete
clauses:
  replay_exact: Replaying a recorded history shall reach exactly the state it was recorded from.
  checkpoint_equivalent: Replaying from a checkpoint shall reach the same state as replaying from the beginning.
  append_only: The recorded history shall be appended to and shall not be rewritten in place.
  truncated_is_reported: A record that ends mid-entry shall be reported and shall replay up to the last complete entry.
  portable: A recorded history shall be replayable in a copy of the project moved elsewhere.
---

# Persistence and replay

`replay_exact` is what makes the tree worth writing down: a history that does not
reproduce its own state is a log, not a record.

`checkpoint_equivalent` states what a checkpoint is allowed to be. It is a
performance device, so it is specified as an equivalence to the unoptimised path
rather than as a feature.

`truncated_is_reported` covers the case that actually happens — a process killed
mid-write. Replaying to the last complete entry and saying so is recoverable;
silently accepting a half-entry writes corruption over a good state.
