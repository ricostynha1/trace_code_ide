---
id: REQ-SBX
title: Sandbox workspace and path policy
refines: [REQ-OBS, ARCH-EFFECT-LAW, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  classification_total: Every path shall classify as exactly one of protected, mirrored, passed-through or outside.
  protected_never_mirrored: A change under a protected path shall never be replayed onto the real tree.
  passthrough_not_mirrored:
    text: Regenerable build and cache directories shall be writable in the workspace and shall not be mirrored.
    anywhere: A directory only tools write — target, node_modules, __pycache__, .lake, .venv, .cache, .mypy_cache, .pytest_cache, .ruff_cache, .gradle, .tox — is regenerable at any depth.
    at_root: A name a person also gives a source directory — build, dist, venv — is regenerable only at the project root, so src/build/mod.rs is mirrored.
  real_tree_untouched: The real tree shall not be written while the workspace is live.
  capability_reported: An unavailable containment mechanism shall be reported as a named state, and the system shall not silently fall back to no containment.
  escape_is_not_silent: A write outside the workspace shall be reported.
---

# Sandbox workspace and path policy

The classification function is the whole security content of this feature, and it
is a total function from a path to one of four answers — which is why it is
separated from the code that builds a container command line. Stated once, it can
be checked directly; buried in argument assembly, only by running a container and
hoping the test covered the case.

`protected_never_mirrored` stops a runaway tool rewriting history: the
version-control and state directories are writable inside the workspace, because
tools legitimately read them, but nothing written there comes back.

`capability_reported` prevents the worst failure — a sandbox that silently is
not one.
