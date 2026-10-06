---
id: REQ-SELFWRITE
title: Self-write suppression
refines: [REQ-MIRROR, ARCH-EFFECT-LAW]
status: approved
decomposition: complete
clauses:
  own_writes_ignored: A change the system itself wrote shall not be ingested as an external change.
  suppression_is_consumed: A suppression shall apply to one observation and shall not persist to later ones.
  content_matched: Suppression shall match on what was written and not on the path alone.
  unmatched_is_external: An observation not matching a pending self-write shall be treated as external.
  no_deadlock: A self-write that is never observed shall expire rather than suppress a later change indefinitely.
---

# Self-write suppression

The mirror writes to the real tree; the watcher watches the real tree. Without
suppression the system observes its own write and either oscillates or fills the
history with duplicates.

Every clause is a way naive suppression fails. Matching on path alone swallows a
genuine external change to a file the system touched. Suppressions that are not
consumed accumulate until the system has stopped listening to a file. A
suppression for a write that is never observed blocks the next real change
forever unless it expires.

None of this is testable by inspection, and all of it is a pure predicate over an
observation and a set of pending writes.
