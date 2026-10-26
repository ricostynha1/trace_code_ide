---
id: REQ-SCREEN
title: What is opened, and what is shown
refines: [REQ-VIEW, REQ-SHOW, ARCH-CORE-SHELL]
status: approved
decomposition: complete
clauses:
  screen_is_a_value: The system shall hold what is on screen as one value: the buffers opened, the layout placing some of them, and which pane has focus.
  opened_outlives_shown: The system shall keep a buffer opened while it is not shown, and when it is shown again, the system shall not produce it a second time.
  layout_tiles_the_region: When a layout holding at least one pane is placed, the system shall give every pane of it a rectangle, and those rectangles shall cover the region without gap or overlap.
  every_pane_is_opened: The system shall place in a layout only buffers that are opened.
  panes_are_distinct: The system shall give no two panes of a layout the same identity, so that a pane can always be named.
  split_keeps_the_buffer: When a pane is split, the system shall leave the buffer that was there in both halves, so that neither half is empty.
  close_collapses_the_pane: When a buffer is closed, the system shall drop it from the opened set and give its region to the rest of the layout, leaving no region without a pane, except that the last opened buffer stays.
  resize_moves_one_divider: When resizing, the system shall move weight between the two sides of one divider and shall change no other pane's weight.
  resize_has_a_floor: If a resize would leave either part beside its divider below a weight of one, then the system shall refuse it and change no weight.
  focus_is_placed: The system shall keep the focus on a pane the layout places.
  one_arrangement_path: The system shall make every change to the arrangement through one function, so that a key and a pointer make the same change.
  focus_follows_geometry: When the focus is moved in a direction, the system shall move it to the pane adjacent in that direction, and where there is none, the system shall leave it where it is.
  strip_is_the_opened_set: The system shall make the strip a buffer whose rows are the opened buffers, in the order they were opened, each carrying an action that names the buffer it shows.
  stations_are_constant: The system shall produce the stations for every state, so that a session can always reach them.
  station_produces_a_buffer: When a station is pressed, the system shall produce the buffer that station stands for.
  station_opens_a_buffer: When a station's buffer is opened, the system shall show it in the focused pane and shall change the layout in no other way.
  workbench_has_three_places: When a session opens, the system shall show three panes side by side, the explorer holding the listing, the document and the side panel, with the focus on the explorer.
  buffer_goes_home:
    text: When a buffer is shown, the system shall show it in the pane its kind belongs in, a listing in the explorer, a file, a review or a record read as a page in the document, and a menu or any other record in the side panel, and where the layout no longer places that pane, in the focused pane.
    pages: The system shall read a record as a page when its title starts with "requirement ", "judge ", "context ", "definitions of ", "uses of ", "search " or "keys".
---

# What is opened, and what is shown

[REQ-VIEW](REQ-VIEW.md) describes a buffer and an editor shows several at once,
so the screen is one value:

```
Screen
  opened   : List Buffer     every buffer this session holds
  layout   : Layout          which of them are on screen, and where
  focus    : PaneId          which pane a key goes to
  nextPane : Nat             where the next pane's identity comes from

Layout = Pane PaneId BufferId
       | Split axis (List (weight, Layout))
```

Splitting leaves one buffer in two panes, so focus names a pane, not a buffer,
and identities come from a counter the screen carries (`ARCH-DETERMINISM`).
Opened and shown are apart so a buffer keeps what it accumulated while it is
off screen, and the strip is a rendering of the opened list rather than a
second list. A part carries a natural weight rather than a fraction because the
layout is compared across Lean, Rust and TypeScript and floating point would
differ at the last digit. A pane resized to nothing cannot be found again, and
a drag arrives a column at a time, so refusing is where it stops.

The stations (Project, Trace, Sandbox, Design, History, Findings and
Definitions) are reachable from every state, since a station produced from the
current project would be missing exactly when no project is open. The
workbench's panes are named `explorer`, `document` and `side`, so a buffer can
go home after the layout has been rearranged; when its home is gone it goes to
the focus, so a layout a person arranged stays theirs. A drag is converted to an
amount by the shell before it reaches the one `resize`
([ARCH-CORE-SHELL](../arch/ARCH-CORE-SHELL.md)).
