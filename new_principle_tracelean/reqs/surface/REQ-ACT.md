---
id: REQ-ACT
title: An action resolves to an intent, and nothing acts on a name
refines: [REQ-MYTH, REQ-VIEW, REQ-CMD]
status: approved
decomposition: complete
clauses:
  action_to_intent: An action name shall resolve to an intent — what the editor is to do next — and nothing shall act on an action name directly.
  focus_is_carried: An intent shall be resolved from the action together with what is under the cursor, so that an action naming a target is given one.
  one_path: A key and a rendered affordance carrying the same action shall resolve to the same intent.
  unknown_is_refused: An action no keymap names shall resolve to a refusal rather than to nothing.
  missing_target_is_refused: An action needing a target the focus does not supply shall resolve to a refusal naming what was missing.
  edits_are_commands: An intent that changes the workspace shall carry a command, so that every change goes through the one path that has an inverse.
  dispatch_is_pure: Resolution shall be a function of the action, the focus, the workspace it would change and the changes waiting to be taken in, and shall read nothing else.
  everything_is_offered: Pointing at a position shall offer every action that applies there — each action a span declares at it, the actions of the buffer it is in, and the places always reachable — each with the keys that reach it, and shall offer no action the dispatcher does not know.
---

# An action resolves to an intent

[REQ-MYTH](REQ-MYTH.md) turns a key into an action name. [REQ-VIEW](REQ-VIEW.md)
puts action names on spans, so a button carries one too. Neither says what an
action *does*, and without that the editor has two ends of a wire and no middle.

This is the middle. An **intent** is what the editor is to do next:

```
Show    a buffer to produce, and from what
Edit    a command to apply (REQ-CMD, so it has an inverse)
Travel  a move through the history tree
Observe a change to what is being watched
Refuse  why nothing happened
```

Resolution takes the action *and the focus* — the buffer, the offset, and the
text of the span under it. That is what makes `file.open` mean anything: the path
it opens is the path under the cursor, which the span already marks. An action
whose focus does not supply what it needs refuses by name rather than silently
doing nothing, because a key that appears to do nothing is the failure nobody
reports.

`one_path` is the point of the whole arrangement. A key and a button that carry
the same action name reach the same function and produce the same intent, so a
frontend cannot grow behaviour the other one lacks. It is checkable because both
are the same call.

`edits_are_commands` keeps the editor's one invariant: nothing changes the
workspace except by a command, so undo is never a special case and provenance is
never missing.

## Offered where you point

A key sequence is only useful to somebody who already knows it. The window was
for a while a screen of buttons nobody could press and actions nobody could
find: thirty-five of them, every one behind a leader key. `everything_is_offered`
is the pointer's half of `one_path` — a right-click lists what a key could do
at the same place, computed by the core (`surface::offer`) from the buffer, the
position and the keymap, so the menu cannot offer what the dispatcher would
refuse as unknown, and cannot leave out what a span declared. Each entry names
its keys, so the menu is also how the keyboard is learnt.

