---
id: REQ-LOOK
title: What is shown can be seen and used
refines: [REQ-VIEW, REQ-SCREEN]
status: approved
decomposition: open
clauses:
  roles_drawn_in_theme_colours: The system shall draw every character of a buffer in the look the theme gives the role of the first span covering it — its foreground colour and whether it is bold — and a character in no span in the frontend's own plain colour, as read back from what the frontend actually emitted, not from what it says it drew.
  setting_edited_in_place: When a setting listed by the definitions buffer is changed, the system shall ask for its new value, write it to the project's override file at once and show it on the next draw without a restart, and if the value is not a colour, then the system shall refuse it and change nothing.
  theme_switched: When the theme is switched, the system shall write the other shipped base — dark or light — to the project's override file at once, keep every colour the project itself set, and draw in that base on the next draw without a restart, in every frontend.
  pointer_reaches_actions: When a frontend reports a pointer, the system shall put the cursor where a click lands, scroll the pane under the wheel, and on a right-click anywhere on the screen list at the pointer exactly the actions offered for that place — the same the bar lists there, each with the keys that reach it.
  menu_bar_lists_leader: The window shall list the leader menu's groups along the top, each opening the entries of its mode with their keys, read from the keymap in use and dispatched like a key, and the settings shall say whether the bar is shown and where the stations are drawn.
  zoom_steps: When the zoom is stepped, the system shall change the size of everything the window draws by ten percent per step, never below 60 nor above 200 percent, write it to the project's override file at once and draw at it on the next draw, and a reset shall return it to 100.
  overview_drawn: The window shall draw the design as figures — the share of clauses holding each mark, the requirements as their refinement graph, and a row per requirement with a cell per mark — from the same nodes the design buffer is produced from, and a requirement drawn shall open when chosen.
  scroll_is_one_draw_per_frame: While the wheel turns, the window shall add the turns up and ask for one scroll per frame, redraw only the panes for it, and draw the bars, the counts and what can be done once, when the wheel stops.
  wheel_moves_the_view_only: When the wheel turns over a pane, the system shall move the view of that pane and leave the cursor where it is, and shall keep the view where the wheel left it until the cursor next moves.
  regions_present: The system shall draw each place of the screen in its own region — the stations, the strip, every pane at the rectangle the layout gives it, the status and the bar of what can be done — with nothing of one drawn inside another's.
  row_fits: When a menu is shown on one row of a given width, the system shall lay it out to that width — each entry's key and description when all of them fit, and only the keys when they do not — so that no entry is cut off while a description takes its room, and each entry still carries what it reaches.
  focus_visible: The system shall make the focused pane visible on the screen alone — every edge it shares with another pane drawn in a style no edge between unfocused panes has — for every pane, including one with no edge of its own.
  accessible_structure: The system shall give assistive technology what the page shows — each region a landmark with a name, each control with a name — as the browser's own accessibility tree reports it.
  journeys_replay: When a person's task is written once as keys, text typed and what must or must not be visible after them, the system shall replay it with the same outcome in every frontend, each driven as a person drives it.
  contrast_sufficient: The system shall give every pair of colours the theme declares as drawn together a contrast ratio, by WCAG's relative luminance and computed from the theme itself, of at least the minimum the pair declares — 4.5 for text, 3 for marks that are not text — unless the pair carries the theme's reason for keeping it below, and shall report a reason on a pair that meets its minimum.
  stations_have_icons: The window shall draw each station of the station bar as a button holding a drawn icon of that station, named by the station's own name for assistive technology, and a station that has no icon shall keep its name as words.
---

# What is shown can be seen and used

[REQ-VIEW](REQ-VIEW.md) checks what every frontend shows; this checks how it is
shown, starting with whether a role's colour can be read against what it is
drawn on. `assets/theme.json` declares the pairs drawn together, each with the
least WCAG ratio it needs; a pair kept below names why, and a reason left on a
pair that has since been fixed is reported. A terminal's output is read back as
a grid of styled cells (`surface/cells.rs`), so the colour a character was drawn
in is a value a test compares with the theme.
