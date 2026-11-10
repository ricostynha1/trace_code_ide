# ROLLOVER — GUI usability (branch `gui-usability`)

*Last updated 2026-11-10. Plan: `work/plan_gui_usability.md`.*

## Goal
Make the window (and terminal) prettier, more responsive, obvious to a first-time user, then add the architecture/testing views of plan §3.

## Done (slice 1, uncommitted on `gui-usability`)
- **Pointer menu** — right-click opens a menu at the pointer (was a bottom bar). Panes: `offers(pane, offset)`. Bottom bar/status: `offers_here`. Elsewhere: leader groups. Arrow/Enter/Escape work. `web/src/app.ts` (`openContext`, `settle`, `openLeader`, `openGroup`, document `contextmenu`), CSS `#context.at-pointer`.
- **Menu bar** — `<nav id="menubar">`, drawn from new editor query `Editor::leader()` (`crates/editor/src/lib.rs`), exposed as Tauri command `leader`. Right side: Search commands, Open file, Theme, `?`.
- **Themes** — `assets/theme-light.json` (no contrast waivers). New action `look.theme` (`Space w c`): `act.rs` `Watch::SwitchTheme`, `Act.lean` `switchTheme`, `keymap.rs` + `Act.lean` action lists, editor handler calls `theme::switch_base` which writes `"base"` in `.tracelean/theme.json`. `theme::load` picks the shipped base then merges the override.
- **Layout settings** — theme section `layout` (`menuBar`: shown|hidden, `stations`: left|top), validated by `setting::takes`; `paint()` in `app.ts` sets body classes `menubar-hidden` / `stations-top`. Settings station has a `[ switch theme ]` row.
- **Welcome** — rewritten as "three things drive everything" + legend, in `welcome.rs` `INTRO` and `formal/TraceLean/Welcome.lean` `introLines` (must stay identical).
- **Terminal mouse** — `crates/tui/src/main.rs` `pointed`, `offset_at`, `offset_in_bar`; mouse capture on/off; theme reloads after "setting saved" (`draw::use_theme` now replaceable).
- **CSS** — transitions, focus rings, `@media` rules (900px, 640px, short heights), reduced motion. Appended at end of `web/style.css`.
- **Requirement** — `reqs/surface/REQ-LOOK.md`: new clauses `theme_switched`, `pointer_reaches_actions`, `menu_bar_lists_leader`.
- **Tests** — `cargo test --workspace -- --include-ignored`: all pass except `fixpoint` (needs `tools/stage0-trace` built; unrelated). `lake build` ok. `node web/test/pointer.mjs` passes (menu-position check rewritten). `tracelean-trace .` → `blocking: false`.

## Done (slice 2, uncommitted on `gui-usability`)
- **Stations rail hidden by default** — `layout.stations` = hidden|left|top (`setting::choices`); stations are their own menu-bar group (`Editor::leader` splits nested modes into groups).
- **Zoom** — `Ctrl +/-/0`, A+/A− buttons → Tauri command `zoom(steps)` → `Editor::zoom` → `setting::zoomed` (60..200, step 10) → `layout.zoom` in `.tracelean/theme.json`. Applied by the `theme` command through `WebviewWindow::set_zoom` (native zoom; CSS `zoom` broke pane placement because `region()` mixes zoomed and unzoomed measures). Fonts default 14px.
- **Tab ×** — was never drawn in the real app: tabs carry `screen.show <id>`, the page compared with `screen.show` exactly. Fixed in `drawBar`; × always visible.
- **Tutorial** — `assets/tutorial.md`, action `help.tutorial` (`Space ?`) → `Intent::Display Record "tutorial"` (`act.rs`, `Act.lean`), produced in the editor as a markdown-highlighted read-only buffer, shown in the document pane (`DOCUMENT_RECORDS` in `screen.rs` + `documentRecords` in `Screen.lean`). First row of welcome's Start here. Welcome text shortened (`welcome.rs` + `Welcome.lean`).
- **Overview** — `web/src/overview.ts` (tiles, per-mark bars, layered requirement graph in SVG, requirement×mark matrix), opened by the menu-bar button; data from `Editor::overview()` = the same `produce::Node`s as the design buffer; styles `.ov-*` at end of `web/style.css`. Window-only (no terminal twin yet).
- **Requirement** — `REQ-LOOK` clauses `zoom_steps`, `overview_drawn`.

## Next (ordered)
1. Decide with the user whether to commit (tree had unrelated uncommitted work before this slice; same files overlap).
2. `tracelean-trace . --stale` lists 43 items to redo (coverage and evidence; many predate this slice). Redo: `--coverage`, `--drt`.
3. New clauses have no Lean model (`Unmodeled`). Candidates to model: `Editor::leader` (pure over the keymap), `offset_at`.
4. Plan P2 leftovers: empty-state hints per station, tooltips on dense rows (plan §2.5), clickable refusals (§2.8).
5. Plan P3 leftovers: SVG station icons with names/badges, tab dirty dot.
6. Plan P4: measure `show()` round-trips (it makes ~10 invokes per draw; batch into one command), then diff-patch panes.
7. Plan §3 features, in the order of its "Suggested priority" (dashboard, clause card, matrix…). Needs the open decision: typed figure payload in `Buffer` vs window-only views.

## Gotchas
- Final full run (2026-11-10): everything passes except `fixpoint` (stage-0 driver not built), `proofs_are_earned::every_theorem_lean_accepts_earns_its_clause_a_record` ("only N proof records written"; proof-earning work in the tree, not GUI) and editor `design` once under parallel load (passes alone). `proofs_are_earned` rewrites `.tracelean/evidence` and `proofs.json` in this repo when run with `--include-ignored`.
- Final test run must be `cargo test --workspace --no-fail-fast -- --include-ignored`; without `--no-fail-fast` cargo stops at `fixpoint` (needs `tools/stage0-trace`) and later crates never run. While working run only new + impacted tests.
- A file not in the editor's workspace snapshot (anything under `.tracelean/`, or written to disk mid-session) cannot be opened with `file.open`; it fails silently. That is why the tutorial is a record, not a file.
- `web/test/look.mjs` copies the tree without `.tracelean/`, so screenshots show no evidence marks (J/T/P = 0).
- `web/test/pointer.mjs` and `crates/editor/examples/look.rs` stub the bridge: a new Tauri command must be added to `look.rs` or the screenshot shows nothing for it.
- Welcome pane is drawn in a proportional font: do not align columns with spaces.
- Key pills (`.context-keys` border) only inside `#context.at-pointer`; in the bottom bar they wrap long key sequences.
- `set_setting` refuses anything `setting::takes` rejects; a new non-colour setting needs an entry in `setting::choices`.
- Screenshot: `node web/build.mjs && node web/test/look.mjs out.png demo ['act_here={"action":"look.theme"}']`.
- TUI right-click shows offers in the bottom bar (no popup); terminals without mouse reporting are unaffected.

## Key paths
`web/src/app.ts` · `web/style.css` (end) · `web/index.html` · `crates/editor/src/{lib.rs,theme.rs}` · `crates/desktop/src/main.rs` · `crates/tui/src/{main.rs,draw.rs}` · `crates/core/src/surface/{act.rs,keymap.rs,setting.rs,welcome.rs,contrast.rs}` · `formal/TraceLean/{Act.lean,Welcome.lean}` · `assets/{theme.json,theme-light.json,keymap.json}` · `reqs/surface/REQ-LOOK.md`

---

# Slice 3 (uncommitted on `gui-usability`; written 2026-11-10)

## Goal
Bigger tutorial with sections that unfold; clause card on hover; graph filters + treemap; test-runs panel; edit impact before saving; smoother scrolling; requirements and tests linked to all of it. Details: `work/handover.md`.

## Done
| Feature | Where |
|---|---|
| Tutorial folds: `+++ Title` ... `+++` in `assets/tutorial.md`; row `▸ Title` carries action `help.more` (`Space m`) | `crates/core/src/surface/tutorial.rs` (`unfold`, `folds`, `title_of`); `act.rs` `Watch::TutorialToggle`; editor `tutorial_buffer()`, field `tutorial_open`, handler in `perform`; `Act.lean` `Watch.tutorialToggle` |
| Tutorial rewritten: ~15 folds, schematics, "Vim-inspired but ignorable" | `assets/tutorial.md` |
| Clause card on hover (chips, requirement names, overview nodes/rows) | editor `Editor::card`, `Card`, `CardClause`; Tauri `card`; `web/src/app.ts` `showCard`, `cardOn`, `hideCard`; CSS `#card` |
| Edit impact: clauses claimed by unsaved files; status badge `Δ n`; overview "Your unsaved edits" | editor `Editor::impact`, pure `impact_of`, `Impact`; Tauri `impact`; `show()` in `app.ts` |
| Test runs on record (Run / Check / Coverage buttons) | editor `Editor::runs` (reads `.tracelean/evidence/*.json` `detail.drt`), `Run`; Tauri `runs`; overview section |
| Graph filters + treemap | `web/src/overview.ts`: `filtered`, `treemap`, `controls`, `tree`, `showing` |
| Scroll: wheel ticks coalesced, one scroll per frame, panes-only redraw, full redraw 140 ms after stop | `app.ts` `wheel`, `pumpWheel`, `show(..., panesOnly)` |
| `serde` dependency for the editor crate; `look.rs` answers `card`, `impact`, `runs` | `crates/editor/Cargo.toml`, `crates/editor/examples/look.rs` |
| Requirements | `reqs/surface/REQ-GUIDE.md` (`tutorial_in_the_editor`, `sections_unfold`), `REQ-FIGURE.md` (`card_on_hover`, `graph_filtered`, `treemap_areas`, `impact_before_saving`, `runs_listed`), `REQ-LOOK.md` (+ `scroll_is_one_draw_per_frame`, ...) |

Verified 2026-11-10: `lake build` passes; `cargo test -p tracelean-core --lib tutorial` (2), `--test shipped_keymap` (6), `--test differential_keymap` (4), `--test differential_act -- --include-ignored` (2), `--test differential_screen -- --include-ignored` (26), welcome tests pass.
Tests: new `crates/editor/tests/figures.rs` (5 tests, one temp dir per test) covers REQ-GUIDE.sections_unfold, REQ-GUIDE.tutorial_in_the_editor, REQ-LOOK.menu_bar_lists_leader, REQ-FIGURE.impact_before_saving, REQ-FIGURE.card_on_hover, REQ-FIGURE.runs_listed. `web/test/pointer.mjs` has a wheel-burst check for REQ-LOOK.scroll_is_one_draw_per_frame. `web/test/overview.mjs` covers REQ-FIGURE.graph_filtered, REQ-FIGURE.treemap_areas, REQ-LOOK.overview_drawn. The checker does not scan `.mjs`, so the `@tests` links for browser checks live on `the_page_in_a_browser_passes_its_checks` in `crates/editor/tests/pointer.rs`, which runs build.mjs, pointer.mjs, journeys.mjs, overview.mjs. `tracelean-trace .` says `blocking: false`.
User manual §7.4 and §7.1 (Help table) updated.

## Next
1. Scroll feel is still unconfirmed by the user in the real window: ask them to try `node web/build.mjs && cargo run -p tracelean-desktop -- demo`. If rough: `show()` makes ~11 invokes per draw, so add one batched Tauri command; and `Editor::scroll` moves the cursor (`step_lines`), consider scrolling the view only.
2. No screenshots yet of the slice-3 features (unfolded tutorial, hover card, treemap, filters, runs table, impact badge). Take them: `node web/build.mjs && node web/test/look.mjs /tmp/t.png demo 'act_here={"action":"help.tutorial"}'`; overview: `EVAL='(async()=>{document.querySelector(".menubar-tool.overview").click(); await new Promise(r=>setTimeout(r,700)); return 1})()' node web/test/look.mjs /tmp/o.png .`
3. Nothing is committed; ask the user before committing (the tree had unrelated uncommitted work).
4. Remaining plan items: terminal twin for the figures, module dependency graph (plan §3.8), agent timeline (§3.9), export (§3.10), SVG station icons.
5. Full suite once, at the end: `cargo test --workspace --no-fail-fast -- --include-ignored`.

## Gotchas
- User rule: while working run only new + impacted tests; full suite once at the end. Be terse.
- Known failures unrelated to GUI: `fixpoint` (needs `tools/stage0-trace` built); `proofs_are_earned::every_theorem_lean_accepts_earns_its_clause_a_record`. `proofs_are_earned` rewrites `.tracelean/evidence` and `proofs.json` in the repo: do not run it casually.
- Never `git stash` here: the tree carries the user's uncommitted work.
- A new Tauri command must also be added to `crates/editor/examples/look.rs`, and tolerated as missing by the stub in `web/test/pointer.mjs` (wrap invokes; see `openOverview`'s `list()` helper).
- New action = 4 places: `act.rs` match, `keymap.rs` action list, `Act.lean` match + action list; plus a key in `assets/keymap.json` (every action must be reachable).
- Welcome text lives twice and must be identical: `welcome.rs` `INTRO`, `formal/TraceLean/Welcome.lean` `introLines`.
- Files not in the editor's workspace snapshot (anything under `.tracelean/`) cannot be opened with `file.open`; that is why the tutorial is a record buffer.
- Tutorial claims must match the keymap: design station is `Space w t r` (also `d`), context menu is `Space c` (`t` toggle, `y` copy), no `Space c o`.
- Zoom is the webview's native zoom, set inside the `theme` Tauri command; do not use CSS `zoom` (breaks `region()`).
- The checker does not scan `.mjs` files: `@tests` links for browser checks go on `the_page_in_a_browser_passes_its_checks` in `crates/editor/tests/pointer.rs`, which runs the `.mjs` scripts. `design` editor test is flaky in parallel (shared pid-named temp dir); `figures.rs` gives each test its own.

## Key paths
`assets/{tutorial.md,keymap.json}` · `crates/core/src/surface/{tutorial.rs,act.rs,keymap.rs}` · `formal/TraceLean/Act.lean` · `crates/editor/src/lib.rs` (`card`, `impact`, `impact_of`, `runs`) · `crates/editor/tests/{figures.rs,pointer.rs}` · `crates/editor/examples/look.rs` · `web/src/{app.ts,overview.ts}` · `web/test/{pointer.mjs,overview.mjs}` · `reqs/surface/{REQ-GUIDE.md,REQ-FIGURE.md,REQ-LOOK.md}` · `work/handover.md`

### Slice 3 — close-out (2026-11-10, late)

**Done**
- Full suite run once: `cargo test --workspace --no-fail-fast -- --include-ignored --skip every_theorem_lean_accepts_earns_its_clause_a_record` → 982 passed, 3 failed.
  - `architecture::only_declared_shells_touch_the_filesystem` failed because `Editor::runs` read `.tracelean/evidence` itself. Fixed: new `tracelean_core::trace::store::read_all_aged(root) -> Vec<(Evidence, Option<u64>)>` (store.rs is a declared shell); `Editor::runs` now reads through it and matches `Detail::Drt`. `architecture` passes (30), `figures` passes (5). The fixture in `figures.rs` now writes a full record (`linkHash`, `inputs`), since records are parsed as `Evidence`.
  - `fixpoint::the_two_kernels_agree_about_this_tree` — known (needs `tools/stage0-trace`).
  - `proofs_are_earned::each_way_of_proving_nothing_is_refused_by_name` — fails with `double_is_twice: left None, right Some(Accepted)`. Cause not investigated; no slice-3 file is involved in the message.
- `@tests` for browser checks (`overview_drawn`, `scroll_is_one_draw_per_frame`, `graph_filtered`, `treemap_areas`) sit on `the_page_in_a_browser_passes_its_checks` in `crates/editor/tests/pointer.rs`; that test passes.
- `tracelean-trace .` → `blocking: false`; findings: Unbound 2, Unmodeled 31, Untested 1 (`REQ-THERMO.to_fahrenheit`, not slice 3).
- Screenshots of the tutorial and the overview were taken and looked right (folds closed; filters, Graph/Treemap switch, funnel). Not taken: unfolded fold, hover card, treemap view, runs table, Δ badge.

**Next**
1. Earn and lock. `tracelean-trace . --stale` lists 49: 29 coverage, 15 differential (REQ-ACT ×7, REQ-VIEW ×3, REQ-LOOK ×2, REQ-MYTH, REQ-SCREEN, REQ-SHOW; "changed: implementation"), 5 judgements (ARCH-NO-DRIVING and others; a person's). Re-running the differential suites alone does NOT clear them: the harness (`crates/core/tests/harness/mod.rs` `compose`) writes a record only when the coverage half is current too. Order: `tracelean-trace . --coverage` (slow), then the differential suites for those operations, then `tracelean-trace . --lock`, then `--stale` again.
2. Look into the `proofs_are_earned` failure above.
3. The tutorial and `user_manual.md` say "press `.`" to unfold; `.` is bound to `screen.offers`, the direct key is `Space m`. Decide which text is right.
4. Items 6 and 7 of `work/handover.md` (scroll feel in the real window; remaining plan items).

**Gotchas**
- The full suite rewrites files under `.tracelean/` (442 touched in this run) even with the skip above.
- `tracelean-trace . --drt` only re-runs the auto-compared clauses (2 here); bound operations are earned by their cargo differential suite.

### Slice 4 — modules, overview as text, agent steps, export, station icons (2026-11-10)

**Goal**: plan §3.8 (module graph), §3.9 (agent steps), §3.10 (export) in their first form, plus SVG station icons.

**Done**
- Clauses written: new `reqs/surface/REQ-MODULES.md` (`one_per_directory`, `edges_from_paths`, `serves_from_claims`, `unclaimed_flagged`, `shown_as_text`, `drawn_in_overview`); `REQ-FIGURE` + `overview_as_text`, `agent_steps_listed`, `exported_as_markdown`; `REQ-LOOK` + `stations_have_icons`. Status `approved`, decomposition `open`, like their neighbours.
- Tests: `crates/core/src/surface/figures.rs` `mod tests` (13, `cargo test -p tracelean-core --lib figures`); `crates/editor/tests/figures.rs` (10 in all: modules, `overview_markdown`, the two stations land in the document pane, agent steps empty with no session and non-empty via `sandbox.new` + files written in the work copy + `tick()`); `web/test/overview.mjs` (`depth` of modules); `web/test/pointer.mjs` (draws the overview in the page: Modules rows/`claims no requirement`/order, agent section only when steps are given, Copy as Markdown calls `copy`; station buttons hold `svg.station-icon`, the stub's stations bar now returns three pieces). `@tests` for the browser checks sit on `the_page_in_a_browser_passes_its_checks`.
- Docs: `user_manual.md` (§7.1 table, stations table, §7.4, §18.2), `skills/README.md`.
- `tracelean-trace .` → `blocking: false`; no new Dangling/Untested.

**Next**
1. Layer-rule check (plan §3.8): declared allowed dependencies between modules, violations as findings.
2. Accept/reject per hunk beside a clause card (§3.9).
3. CI badge and static HTML export of the overview (§3.10).
4. The module graph is Rust-only and a table, not a drawn node-link graph.
5. Earn and lock still pending: `tracelean-trace . --coverage`, the differential suites, `--lock` (see Slice 3 close-out).

**Gotchas**
- The default layout hides the station bar (`layout.stations = hidden`), so icons show only with `left` or `top`.
- `crate::x` where `x` is not a directory module resolves to the crate's root module, so a file using `crate::some_item` gets an edge to the root directory.
- The checker does not scan `.mjs`; DOM checks run inside Chrome from `pointer.mjs` (it `import("./overview.js")` in the page); `overview.mjs` stays DOM-free.
- `Editor::agent_steps` reads `pending`/`agent_tree`, filled only by `tick()` with a session; tests reach it through `sandbox.new` (needs no bwrap to start; it is what `tests/sandbox.rs` does).

**Key paths**
`crates/core/src/surface/{figures.rs,screen.rs,keymap.rs}` · `formal/TraceLean/Screen.lean` · `crates/editor/src/{lib.rs,bin/tracelean-view.rs}` · `web/src/{overview.ts,app.ts}` · `web/test/{overview.mjs,pointer.mjs}` · `reqs/surface/{REQ-MODULES.md,REQ-FIGURE.md,REQ-LOOK.md}`
