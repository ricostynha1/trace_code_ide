# REQ-GUIDE: The editor teaches itself

- status: approved
- decomposition: open
- refines: REQ-SHOW, REQ-ACT

A first-time user cannot be expected to read a manual before pressing a key.
The tutorial is a page the editor itself produces (`assets/tutorial.md`), so
the terminal and the window show the same one and it can never be a file the
project has to carry. It is short when folded and deep when asked: a section
between `+++ Title` and `+++` is one row, `▸ Title`, until its action
(`help.more`) unfolds it. The fold is a pure function of the text and the set
of sections asked for (`surface/tutorial.rs`); which sections are open is the
editor's state, like the folds of the design graph.

## Clauses

- **tutorial_in_the_editor**: When the tutorial is asked for, the system shall show it as a buffer in the document pane of every frontend, coloured as Markdown, reachable from the welcome page, the menu and a key, and shall write nothing to the project to do so.
- **sections_unfold**: The system shall show each section the tutorial marks as folded as one row naming it, and when that row's action is chosen, shall show the section's lines under it, and fold them again when chosen again, changing no other section.
