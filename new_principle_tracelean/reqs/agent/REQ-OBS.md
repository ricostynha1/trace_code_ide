---
id: REQ-OBS
title: Observing an external agent
refines: [ARCH-NO-DRIVING, ARCH-EFFECT-LAW]
status: approved
decomposition: open
clauses:
  workspace_is_a_copy: While an external tool runs, the system shall have it act on a copy of the project and leave the real tree unmodified.
  effects_become_commands: The system shall deliver what the tool did to the editor as commands, through the same path as any other edit, as the tool does it and without waiting to be accepted.
  undoable: The system shall make every mirrored change undoable by the same mechanism as a change made by hand.
  no_instruction_channel:
    text: The system shall have no channel by which the editor sends the tool anything.
    environment: The system shall treat what a sandbox is made with — the copy, the tools and skills it may read, and one brief naming where they are, the same for every task and written before the tool starts — as no channel, and shall let nothing typed into the editor or decided while the tool runs reach the tool through it.
  visible_while_running: The system shall show what the tool has changed and the project does not hold yet, as the difference between the project and the editor on the paths the tool touched, until it is accepted or taken back.
  waiting_shown_with_history: While what a sandboxed tool changed is not accepted, the system shall list those files under the history tree, each with a way to its diff, and shall list a file the tool made in the file tree, marked as made by the tool.
  accepted_is_a_node: When the tool changes the copy, the system shall add the change to the history tree as one node for what arrived together, named as the agent's, that undo and redo move across like any other.
  mirrored_live: While a sandbox is live, the system shall take as the tool's change only what the copy holds now and did not hold when it was last looked at, so that a change undone in the editor is not taken in again, and shall make each file it changed its own point of the history, so the history can be read and filtered by file.
  accepted_is_marked: When the tool's changes are accepted, or the work is saved while some wait, the system shall write them to the project and mark the point of the history the workspace is at as accepted, as a saved point is marked.
  rejected_is_a_node: When the tool's changes are rejected, the system shall add a node that sets those files as the project holds them, shall set the copy the same, and shall drop no node of the history.
  copy_follows_the_history: When the workspace is moved to another point of the history while a sandbox is live, the system shall set the copy to that point on every path the move changed.
  any_state_reachable: The system shall keep every state the workspace was in while the tool ran reachable from the history, whether or not it was accepted.
  only_what_the_tool_changed: The system shall offer as the tool's changes only the paths whose content in the copy differs from what the copy held when it was made, shall not offer a path the project changed since and the tool did not, and if the copy's starting content is unknown, then the system shall offer nothing.
---

# Observing an external agent

The user runs the tool and the editor watches the workspace it wrote. Arriving
as commands, as it happens, an agent's work gets undo, the history tree,
provenance and diff review without any of them knowing an agent exists. The
real tree stays as it was until the person accepts or saves; accepting is a mark
on the history, not the moment the change enters it, so that whatever the agent
mixes up, every state on the way is one jump away. `no_instruction_channel` is
stated as an absence because an absence is what can be checked.
