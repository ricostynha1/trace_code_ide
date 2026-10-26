---
id: REQ-DRIVE
title: The editor is driven, not just its parts
refines: [REQ-MYTH, REQ-VIEW, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  session_is_a_value: The system shall treat a run of the editor as a value — a keymap, a starting mode and a list of keys — and shall produce, for each key, the mode the key left the machine in and the menu offered there.
  walk_follows_the_machine: The system shall pass a session through the modes the mode machine gives for its keys, so that a session can be predicted before anything is run.
  menu_is_the_bar_there: The system shall report, after each key of a session, the which-key bar computed for the mode that key left the machine in, and no other menu.
  keys_arrive_as_bytes: The system shall drive a frontend by the bytes a terminal sends, so that the frontend's own naming of a key is exercised rather than bypassed.
  screen_answers_for_the_frontend: The system shall read what a driven frontend did with a key from the screen it painted, never from its own account of what it did.
  demo_is_what_is_driven: The system shall ship a tree that the driving suite opens and a person can open, so that what is demonstrated and what is tested are the same tree.
---

# The editor is driven, not just its parts

Every function behind the editor can be modelled and tested while the editor
cannot be used: both frontends once handed the space bar to `step` as `" "`
while the keymap bound `Space`, so the leader menu was unreachable, and a test
that supplies a key's name itself can never find that nobody produces it.

So a session is a value, modelled in `formal/TraceLean/Drive.lean` and compared
by generating sessions, and the driving suite writes the bytes a terminal writes
into the real binary and reads the answer off the screen, the one account of a
key press the frontend does not write. It drives [`demo/`](../../demo), the tree
[09-using-the-tui.md](../../docs/09-using-the-tui.md) tells a person to open.
