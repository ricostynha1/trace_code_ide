---
id: REQ-LOOK
title: What is shown can be seen and used
refines: [REQ-VIEW, REQ-SCREEN]
status: draft
decomposition: open
clauses:
  roles_drawn_in_theme_colours: Every character of a buffer a frontend draws shall be drawn in the look the theme gives the role of the first span covering it — its foreground colour and whether it is bold — and a character in no span in the frontend's own plain colour; what is drawn shall be read back from what the frontend actually emitted, not from what it says it drew.
  regions_present: The screen a frontend draws shall show each place in its own region — the stations, the strip, every pane at the rectangle the layout gives it, the status and the bar of what can be done — with nothing of one drawn inside another's.
  row_fits: A menu a frontend shows on one row of a given width shall be laid out to that width — each entry's key and description when all of them fit, and only the keys when they do not — so that no entry is cut off while a description takes its room, and each entry shall still carry what it reaches.
  focus_visible: Which pane has the focus shall be visible on the screen alone — every edge the focused pane shares with another pane drawn in a style no edge between unfocused panes has — for every pane, including one with no edge of its own.
  accessible_structure: The page shall give assistive technology what it shows — each region a landmark with a name, each control with a name — as the browser's own accessibility tree reports it.
  journeys_replay: A person's task, written once as keys, text typed and what must or must not be visible after them, shall replay with the same outcome in every frontend, each driven as a person drives it.
  contrast_sufficient: Every pair of colours the theme declares as drawn together shall have a contrast ratio, by WCAG's relative luminance and computed from the theme itself, of at least the minimum the pair declares — 4.5 for text, 3 for marks that are not text — or shall carry the theme's reason for keeping it below; a reason on a pair that meets its minimum shall be reported.
---

# What is shown can be seen and used

[REQ-VIEW](REQ-VIEW.md) checks *what* every frontend shows: the text, the spans
and the actions on them. Nothing checked *how* it is shown — whether a role's
colour can be read against the background it is drawn on. Only a person looking
at the window noticed. This requirement makes that decidable, starting where it
needs no frontend at all: the theme.

`assets/theme.json` declares, beside its colours, the pairs drawn together
(`contrast`), each with the least ratio it needs. The ratio is WCAG's, so the
thresholds are the ones accessibility tooling uses. A pair kept below its
minimum names why, which keeps a palette decision visible instead of silently
failing or silently passing; a reason left on a pair that has since been fixed
is reported, so reasons do not outlive their cause.

A terminal's output is read back as a grid of styled cells
(`surface/cells.rs`), so the colour a character was drawn in is a value a test
compares with the theme rather than something only a person sees. The page's
computed styles are the same check's next frontend
(`work/visual-testing-proposal.md`).
