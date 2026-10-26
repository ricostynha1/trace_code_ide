---
id: REQ-OBS
title: Observing an external agent
refines: [ARCH-NO-DRIVING, ARCH-EFFECT-LAW]
status: approved
decomposition: open
clauses:
  workspace_is_a_copy: While an external tool runs, the system shall have it act on a copy of the project and leave the real tree unmodified.
  effects_become_commands: The system shall deliver what the tool did to the editor as commands, through the same path as any other edit.
  undoable: The system shall make every mirrored change undoable by the same mechanism as a change made by hand.
  no_instruction_channel:
    text: The system shall have no channel by which the editor sends the tool anything.
    environment: The system shall treat what a sandbox is made with — the copy, the tools and skills it may read, and one brief naming where they are, the same for every task and written before the tool starts — as no channel, and shall let nothing typed into the editor or decided while the tool runs reach the tool through it.
  visible_while_running: The system shall show what the tool has changed before the user decides to accept it.
  only_what_the_tool_changed: The system shall offer as the tool's changes only the paths whose content in the copy differs from what the copy held when it was made, shall not offer a path the project changed since and the tool did not, and if the copy's starting content is unknown, then the system shall offer nothing.
---

# Observing an external agent

The user runs the tool and the editor watches the workspace it wrote. Arriving
as commands, an agent's work gets undo, the history tree, provenance and diff
review without any of them knowing an agent exists. `no_instruction_channel` is
stated as an absence because an absence is what can be checked.
