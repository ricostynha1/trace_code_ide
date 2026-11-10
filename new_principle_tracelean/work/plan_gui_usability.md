# GUI plan: prettier, more responsive, easier, more useful

*2026-11-10. Proposal only; nothing implemented. Scope: the window (`web/`, `crates/desktop`), with core changes where a buffer must change.*

## 0. What exists today (verified in code)

- Right-click works **only inside a pane** (`web/src/app.ts:323`) and on tabs (`:406`). It asks core for `offers(pane, offset)` and draws them, but **ignores the pointer**: `openContext` drops `_x,_y` (`:452`) and CSS `#context.which-key` pins it to the bottom, full width (`web/style.css:643`). It looks like the which-key bar, not a menu.
- No right-click on: station icons, tab strip background, which-key bar, status line, empty pane area.
- Welcome page (`crates/core/src/surface/welcome.rs`) is one dense paragraph + "Start here". It does not say the three things a newcomer needs: `Space` = menu, `.` = actions here, right-click = same.
- `style.css` (792 lines) has no `@media` rules and ~23 hover/focus/transition rules. Fixed 48px station rail, emoji icons, no tooltips with names.

## Constraint that shapes everything

A frontend renders and sends keys; the editor decides (`docs/07-frontends.md`). So:
- **Pure look** (CSS, placement, animation) → `web/` only, no requirement change.
- **New content or actions** → a clause in `reqs/surface/`, Lean model, core producer, then both frontends get it. `render.ts` is differentially checked against the model: do not invent text in `app.ts`.

---

## 1. Implementation plan

Ordered; each phase ships alone.

### P1 — Context menu everywhere (S, web only + 1 small core call)
1. Place `#context` at the pointer: new class `at-pointer` (single column, grouped, clamped to viewport, flips up/left near edges). Keep the bottom which-key layout for the keyboard `.` path.
2. Content = exactly what the bottom bar shows for that spot (same `offers` call) → menu and bar cannot disagree. Keys shown right-aligned, so the menu teaches the keyboard.
3. One `contextmenu` listener on `document`, resolving target → `(pane, offset)`:
   - pane text → as today;
   - empty pane area → offset of last line;
   - station icon → offers for that station (open, close, move side);
   - tab strip / status / bar → global offers (the leader root: File, Screen, …).
   Needs one core addition: `offers` with no offset returns the leader root menu. Clause in `REQ-ACT` (`everything_is_offered` already covers intent).
4. Submenus: a group with >8 entries collapses to a hover/arrow submenu. Arrow keys + Enter + Escape work inside the menu; type-to-filter.
5. Tests: extend `web/test` capture — right-click at N points, assert menu entries == bar entries.

### P2 — First-run guidance (S–M, core + web)
1. Rewrite `welcome.rs` `INTRO` as a 3-line "how to drive" block + a visual key legend (see §2.1). Update Lean model/test for `REQ-SHOW`.
2. Empty-state hints in every station buffer ("No findings. `Space t f` re-checks.") — core producers.
3. Status line gets a persistent right-hand hint: `Space menu · . actions · right-click · F1 keys`. Dismissible setting.

### P3 — Visual polish (M, web only)
1. Design tokens: spacing scale, radius, 2 elevation levels, one accent; audit the `--ui-*` variables into them. Light theme via `prefers-color-scheme`.
2. Station rail: SVG icons replacing emoji (consistent across OSes), active indicator, tooltip = name + key. Badge counts (findings, stale).
3. Tabs: dirty dot, close on hover, overflow scroll with fade, middle-click already there.
4. Hover/focus states on every clickable span; visible focus ring; 80–120 ms transitions on menus, folds, pane resize. Respect `prefers-reduced-motion`.
5. Which-key bar: keys as `<kbd>` pills, groups as columns with headers, max 2 rows then "more…".
6. Typography: UI font for chrome, code font only in buffers; line-height from token, not `21px`.

### P4 — Responsiveness (M)
1. **Layout**: `@media`/container queries — <900px side panel becomes an overlay drawer; <600px explorer collapses to the rail. Pane min-widths; splitter double-click resets.
2. **Speed**: measure first (performance marks around `show`/`paint`). Likely wins: diff-patch pane DOM instead of rebuild; virtualise long buffers (only visible lines — `shown(height)` already windows); debounce `poll`; optimistic cursor move before the round-trip returns.
3. Busy feedback: any invoke >150 ms shows a thin progress bar on the status line and names the job ("running differential tests…").

### P5 — Feature work from §3 (L, one requirement each)

**Order rationale**: P1+P2 fix "I don't know how to use it" in days. P3 is safe CSS. P4 needs measurement. P5 is where the methodology cost (req → model → impl → DRT) is paid.

---

## 2. Ease-of-use proposals

| # | Proposal | Why | Cost |
|---|---|---|---|
| 2.1 | **Welcome = 3 rules + legend.** "`Space` every command · `.` or right-click what works here · `Esc` back out." Then a legend of chips `I T M P D` and marks `AJTP` with colours, then Start here / Recent. | Today's paragraph explains the method, not the controls. | S |
| 2.2 | **Guided tour on the demo project** (5 steps, each highlights a region, advance by doing the action). Re-runnable from Help. | Learn by doing; demo/ already exists. | M |
| 2.3 | **Command palette as the front door**: `Ctrl+Shift+P` exists (`openPalette`) — add a visible search box/button in the tab strip, fuzzy match, shows key + group. | Familiar from VS Code; discoverable by mouse. | S |
| 2.4 | **Menu bar (optional, top)**: File / Trace / Design / Agent / Screen = the leader groups rendered as a classic menu. Same offers, zero new actions. | Mouse-first users find everything without knowing `Space`. | S |
| 2.5 | **Tooltips everywhere**: every chip, mark, icon, percent says what it is and what to do about it ("J0/4 — no clause judged. `Space t j` to judge."). | The dense rows (`I3/4 A4/4 J0/4…`) are unreadable to newcomers. | S–M |
| 2.6 | **Mode indicator** large and coloured in status line (NORMAL / INSERT / MENU) + cursor shape. | Modal confusion is the #1 Vim-style trap. | S |
| 2.7 | **"Click to type" option**: setting where clicking into a file enters Insert, `Esc` returns. Off by default for keyboard users. | Removes the modal barrier for mouse users. | S (core setting) |
| 2.8 | **Refusals offer the fix**: status `nothing happened: 'z' is not bound here` → clickable "show what is" (opens `.`). | Errors become navigation. | S |
| 2.9 | **Breadcrumb** above document: `REQ-THERMO › to_celsius › src/thermo.rs › fn to_celsius`, each segment clickable. | Always shows *why this code exists*. | M |
| 2.10 | **Cheat-sheet overlay** (`F1`/`?`): one screen, grouped, searchable, printable. | F1 exists as a list; make it a designed page. | S |
| 2.11 | Drag-and-drop: tabs between panes, file from explorer to pane, splitter snap. | Expected desktop behaviour. | M |
| 2.12 | Persist + restore layout per project (partly done); add "Reset layout". | Recover from a mess. | S |

---

## 3. Usefulness proposals — seeing architecture, testing, implementation

Goal: answer at a glance *what is the design, how much is built, how much is trusted, what should I do next*.

### 3.1 Project dashboard (new station, the landing page of an opened project) — **highest value**
- Headline funnel: clauses → Implemented → Asserted → Judged → Tested → Proved, as one horizontal stacked bar with counts.
- Tiles: findings by kind, stale evidence, line coverage, DRT divergences, last run time.
- "Next best actions" list: top 5 clauses ranked by (important × cheapest missing mark). Each row is clickable.
- Trend sparkline from history (evidence already versioned in `.tracelean/`).

### 3.2 Design graph as a real graph
- Today: an indented text tree. Add a **node-link / layered view** (roots at top, refinements below), node colour = weakest mark, ring = % clauses implemented, size = clause count. Pan/zoom, click = open, right-click = P1 menu.
- Toggle **treemap** (area = lines of code, colour = assurance) to spot big, weak areas.
- Filters: only open work · only stale · only changed since commit X · by `ARCH-*` root.

### 3.3 Traceability matrix
- Rows = clauses, columns = `I A J T P` + coverage %. Cell = filled / hollow / stale (striped). Sort, filter, sticky header. Click cell → the evidence or the gap's fix action.
- The heat-map view of what `REQ-COV`/`coverage_view` already compute.

### 3.4 Clause card on hover/peek
- Hover a chip or `REQ-X.clause` anywhere → card: clause text, marks, files (impl/test/model/proof), last run, staleness reason. `report.rs` already builds a card per clause — reuse it as a popover and a pinned side peek.

### 3.5 Code-side feedback
- **Minimap** with trace colours: covered / uncovered / unclaimed code stripes; findings as ticks on the scrollbar.
- **Unclaimed code highlight**: functions no clause claims get a faint marker (toggle). Shows architecture drift directly in source.
- Inline lens above a function: `REQ-THERMO.to_celsius · T✓ P✗ · 92% lines · run tests`. Each part clickable.
- Explorer: per-folder roll-up bar (assurance + coverage) beside the letters; dim files with no claims.

### 3.6 Testing state, live
- **Test/DRT run panel**: progress, cases/sec, seed, divergences with the shrunk counter-example and a side-by-side model vs implementation output diff. One-click "re-run this case".
- File watcher → auto re-check on save; chips animate from stale → fresh so cause and effect are visible.
- Flaky/slow markers from run history.

### 3.7 Change impact
- **"What does this edit put at risk?"**: on a dirty file, list clauses whose evidence goes stale, before saving. (Evidence diff exists for review — move it left to edit time.)
- Git-aware: gutter diff + per-commit evidence delta in the history station ("this commit lost T on 2 clauses").
- Blast radius in the graph: select a requirement → highlight everything refining it and every file claiming it.

### 3.8 Architecture view beyond requirements
- **Module dependency graph** (crates/modules, from `use` edges) overlaid with which `ARCH-*`/`REQ-*` each module serves. Flags modules serving many unrelated requirements or none.
- Layer rule check: declare allowed dependencies in an `ARCH-*` clause; violations show as red edges and findings.

### 3.9 Agent work, visible
- Sandbox station timeline: what the agent touched, evidence delta per step, accept/reject per hunk with the clause card beside it.

### 3.10 Sharing
- Export dashboard/matrix/report as static HTML or Markdown for PRs and CI; CI badge per mark level.

### Suggested priority
1. 3.1 dashboard · 3.4 clause card · 2.5 tooltips — biggest clarity per effort, mostly reuse existing producers.
2. 3.3 matrix · 3.5 lens/minimap · 3.6 run panel.
3. 3.2 graph view · 3.7 impact · 3.8 module graph — largest, need new models.

---

## 4. Open decisions (yours)

1. Graphical views (3.1–3.3, 3.8) are not text buffers. Either (a) extend `Buffer` with a typed `figure` payload both frontends handle (TUI draws a text fallback), or (b) allow window-only views. (a) keeps "one editor"; (b) is faster. **Recommend (a)** with a minimal schema: nodes, edges, cells, values.
2. Menu bar (2.4) and click-to-type (2.7): on by default in the window, or opt-in?
3. Light theme: wanted, or dark only?
4. Start with P1+P2 together (≈ one slice), then P3?
