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
  tree_is_drawn: The history shall be drawn as a tree, each node once, under the node it was made after, a node's first child continuing its column and every later child opening a column of its own joined to it.
  filtered_view: The history shall be shown whole, or only the nodes touching one file, or only those at which the work was saved, each shown node under its nearest shown ancestor and the one nearest the workspace's position marked.
  hover_shows_change: Pointing at a node shall show the change it made, as the lines it added and removed, without moving the workspace.
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
