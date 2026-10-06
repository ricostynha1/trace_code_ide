---
id: REQ-MIRROR
title: Mirroring a workspace into the editor
refines: [REQ-OBS, REQ-CMD, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  diff_is_pure: Deriving the mutations between two tree states shall be a function of those two states alone.
  apply_reproduces: Applying the derived mutations to the original tree shall produce the observed tree.
  minimal: The derived mutations shall contain no entry whose removal still reproduces the observed tree.
  protected_excluded: The derived mutations shall contain nothing under a protected path.
  ordering_defined: The derived mutations shall be ordered so that applying them in sequence never depends on a state that does not yet exist.
  binary_handled: A change to a file the editor cannot represent as text shall be reported as a change rather than mirrored as text.
---

# Mirroring a workspace into the editor

`diff_is_pure` and `apply_reproduces` make this checkable. The diff is a function
from two tree snapshots to mutations — no filesystem, no watcher, no timing — and
its law is that applying the output to the first yields the second. Generate tree
pairs, derive, apply, compare: a subsystem that looks inherently effectful becomes
a direct differential test.

`minimal` matters because every spurious mutation becomes an undo node and a diff
hunk somebody must read. A mirror that reports a file changed because its
modification time moved produces a history nobody can navigate.

`ordering_defined` covers the ordinary trap: creating a file inside a directory
the same batch creates.
