---
id: REQ-SHOW
title: The core produces every buffer, from the state it is given
refines: [REQ-VIEW, ARCH-CORE-SHELL, ARCH-DETERMINISM]
status: approved
decomposition: complete
clauses:
  core_produces: Every buffer shall be produced by the core; a frontend shall construct none.
  actions_by_role: A region whose role is all its producer knows of it shall offer the actions of that role, the same in every buffer it is drawn in.
  file_from_text: A file shall become a buffer whose text is the file's text and whose spans come from parsing it.
  listing_from_entries: A directory shall become a buffer whose text is one entry a line and whose spans mark each entry.
  listing_is_a_tree: The project's files shall be listed as a tree whose folders open and close, each row showing a name and standing for its whole path.
  review_from_change: A change between two workspace states shall become a buffer whose text shows what changed and whose spans mark what was added and what was removed.
  menu_from_keymap: A keymap mode shall become a buffer whose text is the keys available in it, and whose spans carry the action a key dispatches where it dispatches one.
  record_from_events: Observed events shall become a buffer whose text is those events and whose spans mark each one.
  index_from_requirements: The requirement set shall become a buffer whose text is one row a requirement and whose spans mark each requirement's identifier and the level its evidence reached.
  graph_from_refinement: The refinement relation shall become a buffer whose text is that relation drawn as an indented graph, folded to the requirements that refine nothing except where unfolded, and a requirement that refines an unfolded one shall appear under it.
  sandbox_from_observation: What a sandboxed agent changed, what it said, and what its usage is estimated to have cost shall become the rows of one buffer, in that order.
  sandbox_session_shown: The sandbox station shall show whether a session exists, the command that enters it, each change waiting with a way to review it, ways to accept and reject them, and the agent's conversation newest first, wrapped to the width it is shown in.
  requirement_opened: A requirement shall open as a buffer showing each of its clauses with the level its evidence reached and every annotation that claims it, each claim a link that opens the claiming file at its line.
  references_are_links: A requirement named in a comment of a file shall be marked as a requirement, so that it opens the requirement it names.
  recent_reopens: The page a project opens on shall list the folders opened before, each as a path that opens that folder as the project.
  claims_beside_code: Each declaration a file's annotations claim shall be marked beside its line with the role of each claim, from the text as it is being edited, and each mark shall open the requirement it claims.
  findings_lead_somewhere: Each finding shall be shown with its place as a link that opens the file at that line, and each requirement its message names as a link that opens that requirement.
  producers_are_total: Every buffer kind shall have a producer, and a producer shall answer for every state it is given rather than failing on some.
  producer_is_pure: A producer shall be a function of the state it is given and shall read nothing else.
  well_formed_by_construction: A produced buffer shall have no faults, whatever state it was produced from.
  window_is_a_buffer: A view of part of a buffer shall itself be a buffer, so that a frontend showing part of something still draws everything it was given.
---

# The core produces every buffer

[REQ-VIEW](REQ-VIEW.md) says a frontend renders one representation and does not
compute its own. That is only true if something else computes it, and this says
what: for every kind of thing the editor shows, the core has one function from
the state to a buffer.

```
file         ← path, text, parsed spans
directory    ← the entries under a path
review       ← what changed between two workspace states
menu         ← a keymap mode
record       ← the events observed from an agent
requirements ← the requirement set, with the level each reached
design       ← the same set, drawn along what it refines
sandbox      ← what an agent changed, said, and is estimated to have spent
```

Each is a function from a value to a value, so each is modelled, differentially
tested and graded like everything else. None of them reads a disk, a terminal or
a clock: the state arrives as an argument. Reading the disk is the shell's job
([ARCH-CORE-SHELL](../arch/ARCH-CORE-SHELL.md)), and the shell hands what it read
to a producer.

`well_formed_by_construction` is the one that earns its place. `faults` already
says what is wrong with a buffer; this says a produced buffer has none — for
*every* state, not the states somebody thought to try. A span reaching past the
end of the text is the bug a renderer turns into a panic, and it is exactly the
bug that generated states find.

The last two are the stations' own content, and they are here rather than in the
frontends for the reason the rest are: a window that knew what a requirement
index looks like and a terminal that did not would be two editors. Both carry
the level in a span rather than leaving it in the text, so a frontend colours L1
apart from L4 without parsing what it was given.

`sandbox_from_observation` fixes an order rather than leaving one, and the order
is the order of trust. What the workspace changed is what happened; what the
tool said is its own account of what happened
([REQ-TRANSCRIPT](../agent/REQ-TRANSCRIPT.md)); what it is estimated to have
cost is a reading of that account against a table
([REQ-COST](../agent/REQ-COST.md)). Each row is one step further from the thing
that is actually true, and the buffer is read top to bottom.

`graph_from_refinement` is bounded on purpose. `refines` is data, so a cycle in
it is representable, and a walk without a bound would not be a function. A node
reached at the bound is drawn without its children: a missing row is a lie about
what exists, a childless row only a view cut short. It opens folded because a
project of hundreds of requirements is read a level at a time; each folded row's
mark unfolds one level, or everything under it.

`window_is_a_buffer` is what lets a frontend scroll without lying. A terminal
showing forty lines of a two-thousand-line file draws a fraction of it, and
[REQ-VIEW](REQ-VIEW.md) says a frontend draws everything it is given — so what
it is given is the fraction. Scrolling is a transformation of the
representation, performed by the core, and not a frontend deciding for itself
what to leave out. The alternative is a conformance check that has to know about
viewports, which is a check that no longer means what it says.

A menu row that *enters a mode* rather than dispatching an action carries no
action on its span, because [REQ-VIEW](REQ-VIEW.md) is explicit that a span's
affordances are the names the keymap dispatches and mode entry is not one. The
key is in the row's text and the keymap already handles it. Inventing an action
name for mode entry would put a name in the registry that no command answers to,
which is worse than a row a mouse cannot reach.

`core_produces` is what keeps the two frontends from diverging again. A frontend
with a producer of its own is a second answer to what is on screen, which is the
failure this whole area exists to prevent — and it is checkable by looking, since
a producer is a function returning a `Buffer` and a frontend that has none cannot
have written one.
