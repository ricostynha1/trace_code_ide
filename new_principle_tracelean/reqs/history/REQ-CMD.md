---
id: REQ-CMD
title: Command algebra
refines: [ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  single_path: The system shall express every mutation of workspace state as a command, and shall change no state by another route.
  inverse_exists: The system shall give every command an inverse computable at the time it is applied.
  round_trip: When a command and then its inverse are applied, the system shall restore the prior state exactly.
  batch_reverses: The system shall make the inverse of a batch the reversed sequence of its members' inverses.
  witness_carried: If a command destroys information, then the system shall carry the destroyed information in the command, so that its inverse needs no other source.
  total_or_refused: If a command is applied to a state it does not fit, then the system shall refuse it with a reason and shall not partially apply it.
---

# Command algebra

When every mutation, whether a keystroke, a keymap action or a change mirrored
from a sandbox, is the same kind of object, undo, provenance, replay and
agent-diff review are one mechanism rather than four. A pure state machine with
a trivial model makes `round_trip` the strongest differential test in the
system, and a command that carries what it destroys can be inverted without
consulting the world.
