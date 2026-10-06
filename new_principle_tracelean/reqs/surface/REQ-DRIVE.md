---
id: REQ-DRIVE
title: The editor is driven, not just its parts
refines: [REQ-MYTH, REQ-VIEW, ARCH-HONEST]
status: approved
decomposition: complete
clauses:
  session_is_a_value: A run of the editor shall be a value — a keymap, a starting mode and a list of keys — and what it produces shall be, for each key, the mode the key left the machine in and the menu offered there.
  walk_follows_the_machine: The modes a session passes through shall be the modes the mode machine gives for those keys, so that a session can be predicted before anything is run.
  menu_is_the_bar_there: The menu a session reports after a key shall be the which-key bar computed for the mode that key left the machine in, and no other.
  keys_arrive_as_bytes: A frontend shall be driven by the bytes a terminal sends, so that the frontend's own naming of a key is exercised rather than bypassed.
  screen_answers_for_the_frontend: What a driven frontend did with a key shall be read from the screen it painted, never from its own account of what it did.
  demo_is_what_is_driven: The project shall ship a tree that the driving suite opens and a person can open, so that what is demonstrated and what is tested are the same tree.
---

# The editor is driven, not just its parts

Every arrow in [06-editor.md](../docs/06-editor.md) was modelled and
differentially tested, and the editor still could not be used. The space bar was
bound as `Space` in `assets/keymap.json` and handed to `step` as `" "` by both
frontends, so `step` answered `PassThrough` for a bound key and the leader menu
was unreachable — from the terminal and from the window alike. Nothing was
broken. `REQ-MYTH.actions_reachable` held of the keymap, `REQ-VIEW` held of
every buffer, and `crates/core/tests/shipped_keymap.rs` pressed `"Space"`
against the data and found it bound, because a test that supplies the name
itself can never discover that nobody produces it.

That is the shape of the gap, and it is not specific to a key name. The suites
here check each arrow by calling the function at its tail. The one thing nothing
called was the editor: a person, a keyboard, and a screen. Between the last
checked function and the first thing a person sees sat one untested translation
per frontend, and a translation nothing exercises is a translation that is
wrong.

So a **session** becomes a value. A keymap, a mode to start in and a list of
keys is data; the modes those keys walk through and the bar offered in each is
data; the function between them is total and pure, which is what makes
`walk_follows_the_machine` and `menu_is_the_bar_there` laws rather than
intentions — modelled in `formal/TraceLean/Drive.lean`, implemented in
`crates/core/src/surface/drive.rs`, and compared by generating sessions.

`keys_arrive_as_bytes` is the half a model cannot state. A session predicted
from the keymap is worth nothing unless the frontend, given what a keyboard
actually sends, walks it — and the failure was exactly in the code that turns a
byte into a name. So the suite opens a pseudo-terminal, spawns the real binary,
and writes the bytes a terminal writes. Nothing is mocked and nothing is named
on the frontend's behalf: `crossterm` parses the bytes it would parse, `name_of`
names them, and the walk either matches the prediction or it does not.

`screen_answers_for_the_frontend` is why the answer is read off the screen. It
is the same argument [REQ-VIEW](REQ-VIEW.md) makes for `screen_is_readable`, one
step further out: a frontend that reported its own mode would be believed, and
the mode is the thing under test. The screen is the only account of a key press
that the frontend does not get to write.

`demo_is_what_is_driven` is the cheap clause and the one that keeps the rest
honest. A suite that drives a tree built for it drives a tree nobody has seen. A
suite that drives [`demo/`](../demo) — the tree
[09-using-the-tui.md](../docs/09-using-the-tui.md) tells a person to open —
fails when the thing a person is told to do stops working, which is the only
failure that matters here.

What this does not claim: that the editor is *good*. It claims that pressing the
keys the documentation names, on the tree the documentation names, reaches the
modes the keymap declares. That is the rung under everything a person judges by
looking, and it was missing.
