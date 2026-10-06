---
id: REQ-LOCK
title: The committed index
refines: [ARCH-DETERMINISM, ARCH-SELFHOST, REQ-STALE]
status: approved
decomposition: complete
clauses:
  deterministic_bytes: The same repository state shall serialise to identical bytes.
  evidence_preserved: Serialisation shall carry evidence records through unchanged.
  diff_is_meaningful: A change in the serialised form shall correspond to a change in the repository.
  pure_render: Rendering the index shall be a function of the index alone, with no filesystem access.
  version_stamped: The format shall carry a version, and an unreadable version shall be reported rather than partially parsed.
---

# The committed index

The index is committed so a change to what a project claims, or to what backs it,
shows up in review. That works only if serialisation is deterministic; otherwise
every diff carries noise and people stop reading them.

`pure_render` makes determinism testable rather than hoped for — a render that is
a function of the index alone can be checked by calling it twice.
