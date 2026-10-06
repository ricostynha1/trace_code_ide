---
id: REQ-CMD
title: Command algebra
refines: [ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  single_path: Every mutation of workspace state shall be expressed as a command, and no state shall change by another route.
  inverse_exists: Every command shall have an inverse computable at the time it is applied.
  round_trip: Applying a command and then its inverse shall restore the prior state exactly.
  batch_reverses: The inverse of a batch shall be the reversed sequence of its members' inverses.
  witness_carried: A command that destroys information shall carry the destroyed information, so its inverse needs no other source.
  total_or_refused: Applying a command to a state it does not fit shall be refused with a reason and shall not partially apply.
---

# Command algebra

`single_path` is the architectural claim everything else depends on. When every
mutation — a keystroke, a keymap action, a change mirrored from a sandbox — is
the same kind of object, undo, provenance, replay and agent-diff review are one
mechanism rather than four kept consistent by hand.

`round_trip` is the best differential test in the system: a pure state machine, a
trivial model, random command sequences compared step by step. If it holds for
every variant, undo cannot corrupt a buffer.

`witness_carried` is what makes an inverse computable without consulting the
world.
