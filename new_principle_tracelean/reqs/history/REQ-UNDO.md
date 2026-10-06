---
id: REQ-UNDO
title: History is a tree
refines: [REQ-CMD, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  no_loss_on_branch: Undoing and then editing shall create a branch and shall not discard the abandoned path.
  jump_equivalence: Jumping to a node shall reach the same state as applying that node's ancestry in order from the root.
  path_via_ancestor: Jumping shall travel by way of the nearest common ancestor, inverting on the way up and applying on the way down.
  preview_is_pure: Producing the diff a node represents shall not change current state.
  reachable: Every node ever created shall remain reachable.
  tree_is_shown: The history shall be shown with every node named, the one the workspace is at marked, and each node a way to jump to it.
---

# History is a tree

A linear stack destroys work: undo three steps, type one character, and the three
are gone. They were an alternative, not a mistake.

`jump_equivalence` is the correctness property, stated as an equivalence between
two computations rather than a description of one. There are two ways to reach a
state — replay from the root, or travel from where you are — and a tree is
trustworthy only if they agree. Travelling via the common ancestor is an
optimisation of the first, and stating both lets it be checked against the
definition.
