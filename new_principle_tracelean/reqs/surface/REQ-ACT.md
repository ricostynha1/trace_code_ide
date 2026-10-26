---
id: REQ-ACT
title: An action resolves to an intent, and nothing acts on a name
refines: [REQ-MYTH, REQ-VIEW, REQ-CMD]
status: approved
decomposition: complete
clauses:
  action_to_intent: The system shall resolve every action name to an intent, what the editor is to do next, and shall act on no action name directly.
  focus_is_carried: The system shall resolve an intent from the action together with what is under the cursor, so that an action naming a target is given one.
  one_path: When a key and a rendered affordance carry the same action, the system shall resolve both to the same intent.
  unknown_is_refused: If no keymap names an action, then the system shall resolve it to a refusal rather than to nothing.
  missing_target_is_refused: If an action needs a target the focus does not supply, then the system shall resolve it to a refusal naming what was missing.
  edits_are_commands: The system shall have every intent that changes the workspace carry a command, so that every change goes through the one path that has an inverse.
  dispatch_is_pure: The system shall resolve an action as a function of the action, the focus, the workspace it would change and the changes waiting to be taken in, and shall read nothing else.
  everything_is_offered: When a position is pointed at, the system shall offer every action that applies there — each action a span declares at it, the actions of the buffer it is in, and the places always reachable — each with the keys that reach it, and shall offer no action the dispatcher does not know.
---

# An action resolves to an intent, and nothing acts on a name

A key becomes an action name ([REQ-MYTH](REQ-MYTH.md)) and a span carries action
names ([REQ-VIEW](REQ-VIEW.md)); this requirement says what an action does. An
intent is what the editor is to do next:

```
Show    a buffer to produce, and from what
Edit    a command to apply (REQ-CMD, so it has an inverse)
Travel  a move through the history tree
Observe a change to what is being watched
Refuse  why nothing happened
```

The focus is the buffer, the offset and the text of the span under it, so
`file.open` opens the path under the cursor. A key that appears to do nothing is
the failure nobody reports, so an action without what it needs refuses by name.
A key and a button carrying the same action reach the same function, so neither
frontend can grow behaviour the other lacks, and the pointer's menu, computed by
the core from the buffer, the position and the keymap, lists what a key could
do at the same place, with its keys.
