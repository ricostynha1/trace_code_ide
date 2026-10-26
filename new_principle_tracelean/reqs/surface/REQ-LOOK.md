---
id: REQ-LOOK
title: What is shown can be seen and used
refines: [REQ-VIEW, REQ-SCREEN]
status: approved
decomposition: open
clauses:
  roles_drawn_in_theme_colours: The system shall draw every character of a buffer in the look the theme gives the role of the first span covering it — its foreground colour and whether it is bold — and a character in no span in the frontend's own plain colour, as read back from what the frontend actually emitted, not from what it says it drew.
  setting_edited_in_place: When a setting listed by the definitions buffer is changed, the system shall ask for its new value, write it to the project's override file at once and show it on the next draw without a restart, and if the value is not a colour, then the system shall refuse it and change nothing.
  regions_present: The system shall draw each place of the screen in its own region — the stations, the strip, every pane at the rectangle the layout gives it, the status and the bar of what can be done — with nothing of one drawn inside another's.
  row_fits: When a menu is shown on one row of a given width, the system shall lay it out to that width — each entry's key and description when all of them fit, and only the keys when they do not — so that no entry is cut off while a description takes its room, and each entry still carries what it reaches.
  focus_visible: The system shall make the focused pane visible on the screen alone — every edge it shares with another pane drawn in a style no edge between unfocused panes has — for every pane, including one with no edge of its own.
  accessible_structure: The system shall give assistive technology what the page shows — each region a landmark with a name, each control with a name — as the browser's own accessibility tree reports it.
  journeys_replay: When a person's task is written once as keys, text typed and what must or must not be visible after them, the system shall replay it with the same outcome in every frontend, each driven as a person drives it.
  contrast_sufficient: The system shall give every pair of colours the theme declares as drawn together a contrast ratio, by WCAG's relative luminance and computed from the theme itself, of at least the minimum the pair declares — 4.5 for text, 3 for marks that are not text — unless the pair carries the theme's reason for keeping it below, and shall report a reason on a pair that meets its minimum.
---

# What is shown can be seen and used

[REQ-VIEW](REQ-VIEW.md) checks what every frontend shows; this checks how it is
shown, starting with whether a role's colour can be read against what it is
drawn on. `assets/theme.json` declares the pairs drawn together, each with the
least WCAG ratio it needs; a pair kept below names why, and a reason left on a
pair that has since been fixed is reported. A terminal's output is read back as
a grid of styled cells (`surface/cells.rs`), so the colour a character was drawn
in is a value a test compares with the theme.
