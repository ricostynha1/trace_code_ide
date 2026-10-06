---
id: REQ-OBS
title: Observing an external agent
refines: [ARCH-NO-DRIVING, ARCH-EFFECT-LAW]
status: approved
decomposition: open
clauses:
  workspace_is_a_copy: An external tool shall act on a copy of the project, and the real tree shall be unmodified while it runs.
  effects_become_commands: What the tool did shall reach the editor as commands, through the same path as any other edit.
  undoable: Every mirrored change shall be undoable by the same mechanism as a change made by hand.
  no_instruction_channel: There shall be no channel by which the editor sends the tool anything.
  visible_while_running: What the tool has changed shall be observable before the user decides to accept it.
---

# Observing an external agent

The one agent-facing capability that survives the scope cut, and the inversion of
the one that did not: the user runs the tool, the editor watches a workspace it
wrote.

`effects_become_commands` is why this is small rather than merely different. An
agent's work arriving as commands inherits undo, the history tree, provenance and
diff review without any of them knowing an agent exists. A bespoke apply path for
agent edits is how the old design ended up needing to understand tools,
permissions and streaming protocols in order to change a file.

`no_instruction_channel` is stated as an absence because that is what can be
checked: not that the editor uses the channel carefully, but that there is
nothing to use.
