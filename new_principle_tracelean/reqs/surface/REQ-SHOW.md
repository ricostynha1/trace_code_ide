---
id: REQ-SHOW
title: The core produces every buffer, from the state it is given
refines: [REQ-VIEW, ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  core_produces: The system shall produce every buffer in the core, and a frontend shall construct none.
  actions_by_role: Where a region's role is all its producer knows of it, the system shall offer the actions of that role, the same in every buffer it is drawn in.
  file_from_text: The system shall make a file a buffer whose text is the file's text and whose spans come from parsing it.
  listing_from_entries: The system shall make a directory a buffer whose text is one entry a line and whose spans mark each entry.
  listing_is_a_tree: The system shall list the project's files as a tree whose folders open and close, each row showing a name and standing for its whole path.
  review_from_change: The system shall make a change between two workspace states a buffer whose text shows what changed and whose spans mark what was added and what was removed.
  menu_from_keymap: The system shall make a keymap mode a buffer whose text is the keys available in it, and whose spans carry the action a key dispatches where it dispatches one.
  record_from_events: The system shall make observed events a buffer whose text is those events and whose spans mark each one.
  index_from_requirements: The system shall make the requirement set a buffer whose text is one row a requirement and whose spans mark each requirement's identifier, the evidence marks it holds, and how many of its clauses hold each mark, each count in its mark's colour.
  children_lines: If the requirements refining a requirement left a line of their code unrun, then the system shall say on its row what share of the lines of it and of them together tests ran.
  open_filtered: When asked, the system shall show in either station only the requirements with a clause short of a mark it can hold or a line its tests never ran, and in the graph also those above them.
  graph_from_refinement: The system shall make the refinement relation a buffer whose text is that relation drawn as an indented graph, folded to the requirements that refine nothing except where unfolded, with a requirement that refines an unfolded one under it.
  sandbox_from_observation: The system shall make what a sandboxed agent changed, what it said, and what its usage is estimated to have cost the rows of one buffer, in that order.
  sandbox_session_shown: The system shall show in the sandbox station whether a session exists, the command that enters it, each change waiting with a way to review it, ways to accept and reject them, and the agent's conversation newest first, wrapped to the width it is shown in.
  requirement_opened: When a requirement is opened, the system shall show each of its clauses with the evidence marks it holds and every annotation that claims it, each claim a link that opens the claiming file at its line.
  references_are_links: When a comment of a file names a requirement, the system shall mark the name as a requirement that opens the requirement it names.
  recent_reopens: The system shall list on the page a project opens on the folders opened before, each as a path that opens that folder as the project.
  claims_beside_code: The system shall mark beside its line each declaration a file's annotations claim, with the role of each claim, from the text as it is being edited, and each mark shall open the requirement it claims.
  findings_lead_somewhere: The system shall show each finding with its place as a link that opens the file at that line, and each requirement its message names as a link that opens that requirement.
  producers_are_total: The system shall have a producer for every buffer kind, and a producer shall answer for every state it is given rather than fail on some.
  producer_is_pure: The system shall make a producer a function of the state it is given that reads nothing else.
  well_formed_by_construction: The system shall produce buffers with no faults, whatever state they are produced from.
  window_is_a_buffer: The system shall make a view of part of a buffer itself a buffer, so that a frontend showing part of something still draws everything it was given.
---

# The core produces every buffer, from the state it is given

[REQ-VIEW](REQ-VIEW.md) says a frontend renders one representation and computes
none of its own, so something else must compute it: for every kind of thing the
editor shows, the core has one function from the state to a buffer.

```
file         ← path, text, parsed spans
directory    ← the entries under a path
review       ← what changed between two workspace states
menu         ← a keymap mode
record       ← the events observed from an agent
requirements ← the requirement set, with the marks each holds
design       ← the same set, drawn along what it refines
sandbox      ← what an agent changed, said, and is estimated to have spent
```

None of them reads a disk, a terminal or a clock; the shell reads and hands the
result over ([ARCH-CORE-SHELL](../arch/ARCH-CORE-SHELL.md)). A span past the end
of the text is the bug a renderer turns into a panic, and generated states find
it, so well-formedness is stated for every state. The sandbox rows go in the
order of trust: what the workspace changed, then the tool's own account
([REQ-TRANSCRIPT](../agent/REQ-TRANSCRIPT.md)), then a reading of that account
against a price table ([REQ-COST](../agent/REQ-COST.md)).

`refines` is data, so a cycle in it is representable; the graph walk is bounded
and a node reached at the bound is drawn without its children. A terminal
showing forty lines of a long file is given those forty lines, so scrolling is
the core's transformation, not a frontend leaving things out. A menu row that
enters a mode carries no action, since mode entry is not a name the keymap
dispatches.
