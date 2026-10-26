---
id: REQ-LOCK
title: The committed index
refines: [ARCH-DETERMINISM, ARCH-SELFHOST, REQ-STALE]
status: approved
decomposition: complete
clauses:
  deterministic_bytes: The system shall serialise the same repository state to identical bytes.
  evidence_preserved: When serialising, the system shall carry evidence records through unchanged.
  diff_is_meaningful: The system shall change the serialised form only when the repository changes.
  pure_render: The system shall render the index as a function of the index alone, with no filesystem access.
  version_stamped: The system shall stamp the format with a version, and if the version is unreadable, then the system shall report it rather than partially parse it.
---

# The committed index

The index is committed so that a change to what a project claims, or to what
backs it, shows up in review. That works only if serialisation is
deterministic, and a render that is a function of the index alone can be
checked by calling it twice.
