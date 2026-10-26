---
id: REQ-MIRROR
title: Mirroring a workspace into the editor
refines: [REQ-OBS, REQ-CMD, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  diff_is_pure: The system shall derive the mutations between two tree states as a function of those two states alone.
  apply_reproduces: When the derived mutations are applied to the original tree, the system shall produce the observed tree.
  minimal: The system shall derive no mutation whose removal still reproduces the observed tree.
  protected_excluded: The system shall derive no mutation under a protected path.
  ordering_defined: The system shall order the derived mutations so that applying them in sequence never depends on a state that does not yet exist.
  binary_handled: If a file the editor cannot represent as text changed, then the system shall report it as a change rather than mirror it as text.
---

# Mirroring a workspace into the editor

The diff is a function from two tree snapshots to mutations, and its law is
that applying the output to the first gives the second, so a subsystem that
looks effectful is tested directly. Every spurious mutation becomes an undo node
somebody must read, and one batch may create a file inside a directory it also
creates.
