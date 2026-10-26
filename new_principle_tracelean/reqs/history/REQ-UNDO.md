---
id: REQ-UNDO
title: History is a tree
refines: [REQ-CMD, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  no_loss_on_branch: When an edit follows an undo, the system shall create a branch and shall not discard the abandoned path.
  jump_equivalence: When jumping to a node, the system shall reach the same state as applying that node's ancestry in order from the root.
  path_via_ancestor: When jumping, the system shall travel by way of the nearest common ancestor, inverting on the way up and applying on the way down.
  preview_is_pure: When producing the diff a node represents, the system shall not change the current state.
  reachable: The system shall keep every node ever created reachable.
  tree_is_shown: The system shall show the history with every node named, the node the workspace is at marked, and each node a way to jump to it.
  tree_is_drawn: The system shall draw the history as a tree, each node once, under the node it was made after, a node's first child continuing its column and every later child opening a column of its own joined to it.
  filtered_view: The system shall show the history whole, or only the nodes touching one file, or only those at which the work was saved, each shown node under its nearest shown ancestor and the one nearest the workspace's position marked.
  hover_shows_change: When a node is pointed at, the system shall show the change it made, as the lines it added and removed, without moving the workspace.
---

# History is a tree

A linear stack destroys work: undo three steps, type one character, and the
three are gone, though they were an alternative rather than a mistake. A state
can be reached by replaying from the root or by travelling from where you are,
and the tree is trustworthy only if the two agree; stating both lets the faster
one be checked against the definition.
