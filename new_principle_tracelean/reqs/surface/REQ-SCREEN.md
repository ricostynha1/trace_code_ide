---
id: REQ-SCREEN
title: What is opened, and what is shown
refines: [REQ-VIEW, REQ-SHOW, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  screen_is_a_value: What is on screen shall be one value — the buffers opened, the layout placing some of them, and which pane has focus.
  opened_outlives_shown: A buffer shall remain opened while it is not shown, and showing it again shall not produce it a second time.
  layout_tiles_the_region: Placing a layout that holds at least one pane shall give every pane of it a rectangle, and those rectangles shall cover the region without gap or overlap.
  every_pane_is_opened: Every buffer a layout places shall be one of the opened buffers.
  panes_are_distinct: No two panes of a layout shall have the same identity, so that a pane can always be named.
  split_keeps_the_buffer: Splitting a pane shall leave the buffer that was there in both halves, so that neither half is empty.
  close_collapses_the_pane: Closing shall drop the buffer from the opened set and give its region to the rest of the layout, leaving no region without a pane — except the last opened buffer, which stays.
  resize_moves_one_divider: Resizing shall move weight between the two sides of one divider and shall change no other pane's weight.
  resize_has_a_floor: A resize shall not leave either part beside its divider below a weight of one; a resize that would shall be refused, changing no weight.
  focus_is_placed: The focused pane shall be one the layout places.
  one_arrangement_path: Every change to the arrangement shall go through one function, so that a key and a pointer make the same change.
  focus_follows_geometry: Moving the focus in a direction shall move it to the pane adjacent in that direction, and shall leave it where it is when there is none.
  strip_is_the_opened_set: The strip shall be a buffer whose rows are the opened buffers, in the order they were opened, each carrying an action that names the buffer it shows.
  stations_are_constant: The stations shall be produced for every state, so that a session can always reach them.
  station_produces_a_buffer: Pressing a station shall produce the buffer that station stands for.
  station_opens_a_buffer: Opening a station's buffer shall show it in the focused pane and shall change the layout in no other way.
  workbench_has_three_places: A session shall open on three panes side by side — the explorer holding the listing, the document, and the side panel — with the focus on the explorer.
  buffer_goes_home:
    text: Showing a buffer shall show it in the pane its kind belongs in — a listing in the explorer; a file, a review or a record read as a page in the document; a menu or any other record in the side panel — and, when the layout no longer places that pane, in the focused pane.
    pages: A record is read as a page when its title starts with "requirement ", "judge ", "context ", "definitions of ", "uses of ", "search " or "keys".
---

# What is opened, and what is shown

[REQ-VIEW](REQ-VIEW.md) says the core produces one representation and a frontend
renders it. [REQ-SHOW](REQ-SHOW.md) says where each buffer comes from. Between
them there is a hole: they describe *a* buffer, and an editor shows several at
once. Until this document, `shown` answered with one buffer, so the window could
only ever draw one thing — not because anybody chose that, but because there was
no value that said otherwise.

This is that value.

```
Screen
  opened   : List Buffer     every buffer this session holds
  layout   : Layout          which of them are on screen, and where
  focus    : PaneId          which pane a key goes to
  nextPane : Nat             where the next pane's identity comes from

Layout = Pane PaneId BufferId
       | Split axis (List (weight, Layout))
```

## A pane is not a buffer

`panes_are_distinct` and the pane identity it is about were not in the first
draft of this document, and the omission is worth recording because the method
found it rather than a reviewer. `split_keeps_the_buffer` says splitting leaves
the same buffer in both halves — and the moment it does, a focus that named a
*buffer* cannot say which half it means. Two panes, one name.

So a pane has its own identity, the way an Emacs window does, and focus names a
pane. Identities come from `nextPane`, a counter the screen carries, so that
minting one stays a function of the value rather than a reach for a clock or a
random number (`ARCH-DETERMINISM`).

## Opened is not shown

`opened_outlives_shown` is the distinction the old surface did not have. A panel
in the old tree was visible or it did not exist; closing one and reopening it
rebuilt it, and anything it had accumulated was gone. Here a buffer is opened
once and placed or not placed. The strip is how a person reaches one that is
opened and not currently on screen, and `strip_is_the_opened_set` makes the
strip a *rendering of that list* rather than a second list maintained beside it
— which is the same failure `REQ-VIEW` prevents one level up. A row's text is a
number and a title, not the buffer's identity, so its action names the buffer
(`screen.show <id>`): a row that only said `screen.show` left the shell to work
out which buffer from the row, a second path for the action.

`every_pane_is_opened` and `focus_is_placed` are the two directions of the same
invariant, and both are worth stating because both are reachable by an ordinary
mistake: closing a buffer while a pane still names it, and moving the focus to a
pane that was collapsed. Either one is a frontend asked to draw something that
does not exist.

## Weights, not fractions

A part carries a natural number, and a pane gets `weight / sum` of its region.
The obvious alternative is a fraction of the whole, and it is wrong here for a
reason specific to this project: the layout is modelled in Lean, implemented in
Rust and implemented again in TypeScript, and the three are compared case for
case. Floating point gives three answers at the last digit, and a differential
suite would report divergences that are nothing but rounding — which trains
everybody to ignore it.

`resize_moves_one_divider` says resizing is local: weight leaves one side and
arrives at the other, and no third pane moves. A resize implemented by
recomputing every weight would satisfy a test that checked only the two panes
either side, and would quietly drift everything else.

`resize_has_a_floor` puts the floor at one rather than at zero. A pane asked for
nothing is not one a person can find again, and a layout that can reach that
state has one from which the only escape is closing something. A resize that
would cross the floor is refused rather than cut short: a cut moved what the
giver could spare, which for a part already at zero was nothing, so the floor
held only for parts that started on it. A drag arrives a column at a time, so
refusing is where it stops anyway.

The floor is on the *weight*, not on the characters a pane ends up with. A
region too narrow to give every pane a character is a small screen, and the
honest answer there is a pane of no width — not a refusal to draw. What this
forbids is arriving at nothing by resizing, which is a thing a person did.

## The stations

Four entries that must be reachable from any state: Project, Sandbox,
Requirements, Design. `stations_are_constant` is stated over *every* state
because the failure it prevents is the interesting one — a station produced from
"the current project" is unreachable exactly when no project is open, which is
when a person most needs the one that opens a project.

Pressing a station does two separable things, so it is two clauses.
`station_produces_a_buffer` is the shell's half — reading a project tree,
tailing a transcript, rendering the refinement graph — and none of it is built
yet, so it reads as outstanding work and should. `station_opens_a_buffer` is the
half that is a value: the buffer goes into the focused pane and the layout
changes in no other way. A station that rearranged the screen would make the
arrangement something a person cannot rely on having built.

They were one sentence joined by an "and" until a judgement pointed out that
half of it was modelled and half was not, and a single clause cannot say which
half its evidence is about.

The stations are a `Menu` buffer, like the strip, because
[REQ-VIEW](REQ-VIEW.md) admits nothing that is not a buffer. A frontend with a
rich medium draws a station as an icon; the icon's accessible name is the row's
own text, so what the harness reads is what the buffer said. A frontend that
does not know a role draws its text.

## The workbench

The first TraceLean was a window with three places: a file tree on the left, the
document in the middle, and a panel on the right whose content depended on what
was asked for — requirements, the trace, an agent. People who used it found
things by where they were. `workbench_has_three_places` keeps that arrangement
as the screen a session opens on, so the terminal and the window start from the
same value rather than each drawing its own idea of a sidebar.

The three panes are *named* — `explorer`, `document`, `side` — rather than
minted, because `buffer_goes_home` has to say where a buffer belongs after a
person has resized, split beside or closed things. A minted identity is `pane`
and a number, so the names never collide with one.

`buffer_goes_home` moves the focus and then shows the buffer, which is two
changes this document already has: `focusPane` and `showBuffer`. So
`station_opens_a_buffer` still holds as it was written — the buffer goes into
the focused pane and the layout changes in no other way — and what is new is
only *which* pane is focused first. When the home pane is gone the buffer goes
where the focus is, rather than the panel being brought back: a layout a person
arranged stays the one they arranged.

## One way in

`one_arrangement_path` is `REQ-ACT.one_path` one layer down, and it is here
because this is where the temptation is. A terminal resizes with a key and a
window resizes by dragging a divider with a pointer; the pointer knows a number
of pixels and the key knows it means "a bit more". The easy thing is for each
frontend to work out the new weights itself, and then there are two answers to
what a resize does and only one of them is ever exercised by a test.

So there is one function from an arrangement and a region to a screen, every
frontend calls it, and a drag is converted to an amount before it arrives rather
than after.

## What this does not say

Nothing here says how wide a pane is in pixels, where a pointer was when it was
released, or what a divider looks like. A region arrives as an argument and a
rectangle comes back. Converting a drag into a weight is the shell's job
([ARCH-CORE-SHELL](../arch/ARCH-CORE-SHELL.md)), and it is the same conversion
whichever frontend performs it, because both end in the one `resize` this
document describes.
