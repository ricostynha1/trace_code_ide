# Proposal: requirements and tests for how the applications look and are used

Status: proposed 2026-10-10; to be implemented after the open plan items.

## The gap

Every frontend renders a `Buffer` the core produced, and the suites check that
*what* is shown is right: the text, the spans, the actions a span carries
(`frontend_conformance*`, `tui/tests/driving.rs` over a real pseudo-terminal,
`web/test/pointer.mjs` over headless Chrome). Nothing checks *how* it is shown
or whether a person can use it: that the screen has the three places it should,
that a span with role `added` is drawn in the theme's colour for `added`, that
text is readable against its background, that every action is reachable from
the keyboard and from the pointer, that focus is visible. Today only a person
looking at the window would notice. That is the blind spot.

## What already exists to build on

- One representation: `Screen` (layout, panes, focus) and `Buffer` (text,
  spans with roles and actions) are values, and `assets/theme.json` maps every
  role to a colour and font. So "the right colour" is decidable: the theme
  says what each role's colour is, the frontend's output says what it drew.
- The TUI is already driven as a person would, on a pseudo-terminal
  (`tui/tests/terminal.rs`); what comes back today is read as text only.
- The page is already driven in headless Chrome over the DevTools protocol
  (`pointer.mjs`, `look.mjs`), with a real editor behind it (`look.rs`).

## Requirements (a new `REQ-LOOK`, refining `REQ-VIEW` and `REQ-SCREEN`)

- `regions_present`: the screen shows each place `REQ-SCREEN` declares (the
  strip, the panes the layout has, the bar), each a distinct, non-overlapping
  region that tiles the window, in every frontend.
- `role_colours_from_theme`: every character a frontend draws in a span is
  drawn in the theme's foreground and background for that span's role; text in
  no span uses the theme's plain colours.
- `contrast_sufficient`: every foreground/background pair a frontend draws has a
  WCAG contrast ratio of at least 4.5:1 (3:1 for large text and dividers),
  computed from `theme.json` itself, so a theme edit cannot make a role
  unreadable unnoticed.
- `focus_visible`: the focused pane is distinguishable from the others in every
  frontend (a style difference the test can read, not a convention).
- `operable_both_ways`: every action a buffer offers can be reached by a key
  sequence and by the pointer, and both send the same command (extends
  `REQ-ACT.one_path` from "resolves to the same intent" to "can be reached").
- `accessible_structure` (page only): the page's accessibility tree names each
  region and control; an `axe` scan reports no violation.
- `journeys_replay`: a person's task (open a file, split, edit, review an
  agent's change, accept it) is written once as a journey of inputs and
  expectations over the screen, and replays identically in the TUI and the
  page.

## How each is tested

1. **A screen description, read back from each frontend.** One value per
   frontend capture: regions (name, rectangle), and per cell or element the text,
   foreground, background and modifiers.
   - TUI: parse the escape sequences the pseudo-terminal receives into a cell
     grid with styles (a small VT parser, or the `vt100` crate as a dev
     dependency), instead of stripping them.
   - Page: from DevTools, `getBoundingClientRect` for each region and computed
     `color`/`background-color` for each span element (`DOM.getBoxModel`,
     `CSS.getComputedStyleForNode`), and the accessibility tree
     (`Accessibility.getFullAXTree`).
2. **Checks are functions over that description**, written once in Rust and
   shared, each with a positive test (the real frontends) and a negative one (a
   description built to violate it), as the structural checks are.
   - `regions(description, screen) -> Vec<Violation>`: count, names, tiling.
   - `colours(description, buffer, theme)`: each cell's style against its role.
   - `contrast(theme)`: pure, over `theme.json`; runs without any frontend.
   - `focus_visible(description, screen)`.
3. **Contrast is modelled.** The WCAG relative-luminance formula is a pure
   function; a Lean model and a differential test, like any other decision.
4. **Journeys** are JSON files of steps (`key`, `click region/offset`, `expect`
   region text, colour, focus), run by the TUI harness and the page harness; a
   journey that passes in one frontend and not the other is the finding.
5. **Pixels are not the oracle.** Screenshot comparison (`toHaveScreenshot`-style)
   is brittle across fonts and platforms; it stays an optional artefact for a
   person (`look.mjs`). The oracle is the structured description: rectangles,
   colours and the accessibility tree, all deterministic.
6. **Accessibility on the page** with `axe-core` injected through DevTools
   (no Playwright dependency needed; the existing Chrome harness suffices). If
   the harness grows, Playwright's `toMatchAriaSnapshot` and bounding-box
   snapshots are the off-the-shelf equivalent.
7. **The desktop window** (Tauri) loads the same page; it is covered by the
   page tests, plus one smoke test through `tauri-driver` (WebDriver, Linux)
   when it is worth the dependency.

## Order of work

1. Done: `contrast` over `theme.json` (pure; Lean model + DRT).
2. Done: `surface/cells.rs` reads a terminal's bytes into styled cells (Lean
   model + DRT); `roles_drawn_in_theme_colours` checked on every buffer the
   TUI paints; `focus_visible` and `regions_present` over the pty. The grid
   models wrapping and scrolling as a terminal does, which is what found
   four things the text-only suites could not: the side panel focused with
   nothing marking it; the stations row wrapping at 100 columns and scrolling
   the screen a row; the last three stations off the edge (now
   `produce::menu_row`, `REQ-LOOK.row_fits`); the status line's reverse video
   undone by its first colour (`draw::painted_with`).
3. Done: the page over DevTools (`pointer.mjs`, run from cargo): regions,
   focus, and `accessible_structure` from Chrome's own accessibility tree
   instead of `axe` (no dependency). Using the shipped theme found the caret
   catching clicks. Open: role colours on the page (see review points).
4. `operable_both_ways`: already the model-level pair `REQ-MYTH.actions_reachable`
   (keys) and `REQ-ACT.everything_is_offered` (pointer, with keys); left there.
5. Done: `tests/journeys.json` (four tasks), replayed in the terminal
   (`driving.rs`) and in the page against a real editor (`journeys.mjs`, run
   from cargo); all pass in both. Add a journey with each new task.

## Sources consulted

- Playwright: visual comparisons and ARIA snapshots (`toHaveScreenshot`,
  `toMatchAriaSnapshot`); ARIA snapshots are the less flaky structural oracle.
- ratatui `TestBackend`: text snapshots miss styles; per-cell style assertions
  are the remedy (the reason for parsing styles, not just text).
- Tauri testing: WebDriver via `tauri-driver` on Linux/Windows.
- Layout assertions over bounding boxes (Galen-style `toBeLeftOf`/`toBeAbove`)
  and `axe-core` for WCAG rules, contrast among them.
