---
id: REQ-SBX
title: Sandbox workspace and path policy
refines: [REQ-OBS, ARCH-EFFECT-LAW, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  classification_total: The system shall classify every path as exactly one of protected, mirrored, passed-through or outside.
  protected_never_mirrored: The system shall never replay a change under a protected path onto the real tree.
  passthrough_not_mirrored:
    text: The system shall let regenerable build and cache directories be written in the workspace, and shall not mirror them.
    anywhere: The system shall treat a directory only tools write — target, node_modules, __pycache__, .lake, .venv, .cache, .mypy_cache, .pytest_cache, .ruff_cache, .gradle, .tox — as regenerable at any depth.
    at_root: The system shall treat a name a person also gives a source directory — build, dist, venv — as regenerable only at the project root, so src/build/mod.rs is mirrored.
  real_tree_untouched: While the workspace is live, the system shall not write the real tree.
  capability_reported: If a containment mechanism is unavailable, then the system shall report it as a named state, and shall not silently fall back to no containment.
  escape_is_not_silent: If a write lands outside the workspace, then the system shall report it.
---

# Sandbox workspace and path policy

The classification is the whole security content of the feature. As a total
function from a path to one of four answers it is checked directly, not by
running a container. Version-control and state directories are writable inside
the workspace, because tools read them, but nothing written there comes back.
The worst failure is a sandbox that silently is not one.
