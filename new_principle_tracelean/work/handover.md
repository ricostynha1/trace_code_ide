# HANDOVER — GUI usability, after slice 4

*Written 2026-11-10 (end of day). Repo: `new_principle_tracelean/`. Branch: `gui-usability`. Everything is committed and pushed, except `demo/.tracelean/theme.json` (untracked; probably written by running the app on `demo`; not ours to decide).*

Read first: `work/ROLLOVER_gui.md` (slices 1–4, each with Done / Next / Gotchas / Key paths) and `work/plan_gui_usability.md` (the plan). This file is only what to do next.

## State

- Slices 1–4 are implemented, tested and committed. `tracelean-trace .` ends `blocking: false` (Unbound 2, Unmodeled 43, Untested 1 = `REQ-THERMO.to_fahrenheit`, not GUI).
- Evidence is re-earned and locked. `tracelean-trace . --stale` lists only six judgements (a person's): `ARCH-NO-DRIVING.no_launch`, `.observation_only`, `.user_runs_it`, `REQ-LOOK.accessible_structure`, `REQ-OBS.no_instruction_channel`, `REQ-SCREEN.station_produces_a_buffer`.
- Commits of 2026-11-10, oldest first: `ed0c36c` Lean grammar · `8a5dd91` earlier core work (does not build alone; shares files with the next) · `1cdd3d6` GUI slices 1–4 · `f620bca` evidence · `6b188ee` manual and notes · `0d7314f` wheel scrolls the view only · `f0aeea4` press outside closes a menu · `131e6f1` evidence re-earned and locked · then this handover.

## What slice 4 and the late fixes added

| Feature | Where |
|---|---|
| Overview as text / Markdown: station `overview` (`Space w t o`), `tracelean-view . overview` (redirect to a file = the export) | `crates/core/src/surface/figures.rs` `overview`; editor `Editor::figures`, `overview_markdown`, `markdown_record` |
| Module dependency graph: station `modules` (`Space w t m`), `tracelean-view . modules`, "Modules" table in the window's overview | `figures.rs` `modules`, `serving_nothing`, `module_lines`; editor `Editor::modules`; `web/src/overview.ts` `modules`, `depth` |
| Agent's waiting changes (one step per file, in order, with clauses claimed and marks held) | editor `Editor::agent_steps`; `overview.ts` `steps` |
| "Copy as Markdown" button in the overview | `overview.ts` `Handlers.copy`; `web/src/app.ts` `openOverview` |
| SVG station icons | `app.ts` `STATION_ICONS`, `stationIcon` |
| Wheel moves the view only; cursor stays; view returns to the cursor when it next moves | editor `Editor::scroll`, field `scrolled`, `follow_cursor` |
| A press outside an open menu closes it | `app.ts`, capturing `mousedown` listener next to `document.addEventListener("click", …closeContext)` |
| Requirements | `reqs/surface/REQ-MODULES.md` (new); `REQ-FIGURE` `overview_as_text`, `agent_steps_listed`, `exported_as_markdown`; `REQ-LOOK` `stations_have_icons`, `wheel_moves_the_view_only`, `press_outside_closes_menu` |

## Next steps, in order

1. **User feedback pending.** The user had not yet confirmed, in the real window, that (a) scrolling is smooth now and the bottom key menu no longer changes while scrolling, (b) top-bar menus close on a click outside. Ask. Run: `node web/build.mjs && cargo run -p tracelean-desktop -- demo`.
   - If scrolling is still rough: a scroll frame makes 3 invokes with the cursor off screen (`scroll`, `shown`, `cursor`) and 6 with it on screen (plus `selection`, `occurrences`, `brackets`, already sent together). Next lever: one batched Tauri command returning all of them (add it to `crates/desktop/src/main.rs`, `crates/editor/examples/look.rs`, and the stub in `web/test/pointer.mjs`).
   - The first wheel turn over another pane still focuses that pane (`Editor::scroll` calls `focus_pane`, because `top` belongs to the focused buffer), so the key menu can change once.
2. **Flaky test.** `crates/editor/tests/design.rs` `the_design_opens_on_its_roots_and_unfolds_on_request` fails in the full parallel run: its tests share one pid-named temp dir. Give each test its own dir (see `project()` in `crates/editor/tests/figures.rs`: pid + atomic counter). Then re-measure coverage (step 6), because a changed test file makes its coverage stale.
3. **Look at `proofs_are_earned`.** `tools/differential-all.sh` reported the suite failing. The race between two tests sharing a scratch dir was fixed (each calls `fixture("<name>")`). `every_theorem_lean_accepts_earns_its_clause_a_record` was a known failure before this work; whether it is the only one left was not checked. Run `cargo test -p tracelean-core --test proofs_are_earned -- --include-ignored` (it rewrites `.tracelean/evidence` and `proofs.json`).
4. **Not built from the plan** (`work/plan_gui_usability.md` §3):
   - §3.8 layer-rule check: declare allowed module dependencies in an `ARCH-*` clause; violations as findings and marked rows. The module graph today is Rust only, a table, not a drawn node-link graph; a `crate::item` path where `item` is not a directory resolves to the crate's root module.
   - §3.9 accept/reject per hunk with the clause card beside it (the overview only lists the steps; accepting still happens in the sandbox station).
   - §3.10 static HTML export and a CI badge per mark (Markdown export exists).
   - Earlier sections never started: §3.3 a sortable matrix with stale cells, §3.5 minimap / inline lens / unclaimed-code highlight, §3.6 live run panel with counter-examples.
5. **Screenshots never taken**: unfolded tutorial fold, hover card, treemap view, runs table, `Δ` badge, agent section with steps, station bar with icons (the default layout hides the station bar; `layout.stations` must be `left` or `top`).
6. **Earn and lock after any source change**, in this order:
   ```bash
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --coverage   # slow; any .rs change re-runs it
   bash tools/differential-all.sh                                         # sets TRACELEAN_DRT_LINES=1; skips current ops
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --lock
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --stale      # should list only judgements
   ```

## Gotchas

- User rule: while working run only new + impacted tests; full suite once at the end with `--no-fail-fast`. Be terse. The user wants requirements, tests, annotations and docs delegated to a cheaper subagent (Sonnet) where possible; feature code and verification stay with the main session.
- A plain `cargo test` does NOT re-earn a differential record: Rust-bound operations need the line half, recorded only with `TRACELEAN_DRT_LINES=1` (`tools/differential-all.sh` sets it). `tracelean-trace . --drt` only covers the auto-compared clauses (2 here).
- Do not edit source while a coverage or differential run is in flight: the records come out stale again.
- The full suite and the differential script rewrite files under `.tracelean/`; commit them as their own evidence commit.
- The checker does not scan `.mjs`. `@tests` links for browser checks live on `the_page_in_a_browser_passes_its_checks` in `crates/editor/tests/pointer.rs`, which runs `web/build.mjs`, `pointer.mjs`, `journeys.mjs`, `overview.mjs`.
- Only declared shells may touch the filesystem (`crates/core/tests/architecture.rs` `SHELLS`). The editor crate is not one: read evidence through `tracelean_core::trace::store` (`read_all`, `read_all_aged`).
- A new station = `station_entries` + `station_kind` (+ `DOCUMENT_RECORDS` if it belongs in the document pane) in `crates/core/src/surface/screen.rs`, the same three in `formal/TraceLean/Screen.lean`, the action in `crates/core/src/surface/keymap.rs`, a key in `assets/keymap.json` (Station mode, keys kept sorted), a title match in the editor's `view`, a view name in `crates/editor/src/bin/tracelean-view.rs`, and an icon in `STATION_ICONS`.
- A new action (not a station) = 4 places: `act.rs` match, `keymap.rs` action list, `Act.lean` match + action list; plus a key in `assets/keymap.json`.
- A new Tauri command must also be answered by `crates/editor/examples/look.rs` and tolerated as missing by the stub in `web/test/pointer.mjs`.
- Known failure unrelated to GUI: `fixpoint` (needs `cargo build --release` in `tools/stage0-trace`).
- Tutorial and manual: sections unfold with a click or `Space m`; `.` opens the "what can be done here" menu, it does not unfold.
- Zoom is the webview's native zoom, set inside the `theme` Tauri command; do not use CSS `zoom`.
- `web/test/look.mjs` copies the tree without `.tracelean/`, so marks and runs show empty in its screenshots.
