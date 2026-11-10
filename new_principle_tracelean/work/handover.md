# HANDOVER — GUI usability, slice 3 in progress

*Written 2026-11-10. Repo: `new_principle_tracelean/`. Branch: `gui-usability`. Nothing is committed (the tree had unrelated uncommitted work before this started; ask the user before committing).*

Read first: `work/ROLLOVER_gui.md` (slices 1–2, done and verified) and `work/plan_gui_usability.md` (the plan). This file covers slice 3 only.

## Goal of slice 3
Bigger tutorial with sections that unfold; clause card on hover; graph filters + treemap; test-runs panel; edit impact; smoother scrolling; requirements and tests linked to all new features.

## State: code written, PARTLY verified

### Verified (ran and passed)
- `cargo build -p tracelean-desktop -p tracelean-editor --examples` — builds.
- `node web/build.mjs && node web/test/pointer.mjs` — passes (run before the tutorial rewrite; web code unchanged since except `overview.ts` hint text).
- `node web/test/overview.mjs` — passes (new; filters, treemap, layers).

### Written but NOT yet run
1. **Lean**: `formal/TraceLean/Act.lean` gained `Watch.tutorialToggle` and the `"help.more"` dispatch + action-list entry. Run `cd formal && lake build`.
2. **Core unit tests**: `cargo test -p tracelean-core tutorial` (new `crates/core/src/surface/tutorial.rs`; one test asserts the shipped tutorial has ≥6 folds, every fold closed, and unfolding adds >40 lines).
3. **Differential tests touched**: `cargo test -p tracelean-core --test differential_act -- --include-ignored` (act.rs/Act.lean changed), `--test differential_screen` (`DOCUMENT_RECORDS` gained `"tutorial"`), and the welcome one (`cargo test -p tracelean-core welcome -- --include-ignored`). Also keymap validation: `cargo test -p tracelean-core keymap` (new binding `Space m` → `help.more`).
4. **No screenshot taken** of: unfolded tutorial, hover card, treemap, filters, runs table, impact badge. Take them:
   ```bash
   node web/build.mjs
   node web/test/look.mjs /tmp/t.png demo 'act_here={"action":"help.tutorial"}'
   EVAL='(async()=>{document.querySelector(".menubar-tool.overview").click(); await new Promise(r=>setTimeout(r,700)); return 1})()' node web/test/look.mjs /tmp/o.png .
   ```
   (`look.mjs` copies the tree without `.tracelean/`, so marks J/T/P and runs show empty there.)

## What was added in slice 3 (all uncommitted)

| Feature | Where |
|---|---|
| Tutorial folds: `+++ Title` … `+++` in `assets/tutorial.md`; row `▸ Title` carries action `help.more` | `crates/core/src/surface/tutorial.rs` (`unfold`, `folds`, `title_of`); `act.rs` `Watch::TutorialToggle`; editor `tutorial_buffer()`, field `tutorial_open`, handler in `perform` |
| Tutorial text rewritten: ~15 folds, ASCII schematics, "Vim-inspired but ignorable" | `assets/tutorial.md` (the user or a tool edited it after my last write — re-read it, keep their version) |
| Clause card on hover (chips, requirement names, overview nodes/rows) | editor `Editor::card` + structs `Card`, `CardClause`; Tauri command `card`; `web/src/app.ts` `showCard`, `cardOn`, `hideCard`; CSS `#card` |
| Edit impact: clauses claimed by unsaved files | editor `Editor::impact`, pure `impact_of`, struct `Impact`; command `impact`; status badge `Δ n` in `show()`; "Your unsaved edits" section in overview |
| Test runs on record | editor `Editor::runs` (reads `.tracelean/evidence/*.json` `detail.drt`), struct `Run`; command `runs`; overview section with Run / Check / Coverage buttons |
| Graph filters + treemap | `web/src/overview.ts`: `filtered`, `under`, `treemap`, `step`, `left`, `showing` state, `controls`, `tree` |
| Scroll: wheel ticks coalesced, one scroll per frame, panes-only redraw, full redraw 140 ms after stop | `web/src/app.ts` `wheel`, `pumpWheel`, `show(..., panesOnly)` |
| `serde` dependency for editor crate | `crates/editor/Cargo.toml` |
| `look.rs` example answers `card`, `impact`, `runs` | `crates/editor/examples/look.rs` |

## Requirements added (frontmatter clauses)
- `reqs/surface/REQ-GUIDE.md` (new): `tutorial_in_the_editor`, `sections_unfold`.
- `reqs/surface/REQ-FIGURE.md` (new): `card_on_hover`, `graph_filtered`, `treemap_areas`, `impact_before_saving`, `runs_listed`.
- `reqs/surface/REQ-LOOK.md`: `theme_switched`, `pointer_reaches_actions`, `menu_bar_lists_leader`, `zoom_steps`, `overview_drawn`, `scroll_is_one_draw_per_frame`.

## Next steps, in order
1. Run the "NOT yet run" list above; fix what fails.
2. **Tests still missing** (clause has `@implements` but no `@tests`). Add `crates/editor/tests/figures.rs` (copy the `project()` helper from `crates/editor/tests/design.rs`, but give each test its own temp dir — the shared pid-named dir is why `design` is flaky in parallel):
   - `REQ-GUIDE.sections_unfold` + `tutorial_in_the_editor`: `editor.act_here("help.tutorial")`; put `editor.offset` on a `▸` row; `editor.act_here("help.more")`; assert text grew and row is now `▾`; toggle back.
   - `REQ-LOOK.menu_bar_lists_leader`: `editor.leader()` has a group per Leader `enter` binding and every entry's action is in `keymap::actions()`.
   - `REQ-FIGURE.impact_before_saving`: unit-test `tracelean_editor::impact_of` (pure): dedupes per file, ignores unchanged files, sorted.
   - `REQ-FIGURE.card_on_hover`: tree with a requirement + annotated file; `editor.card("REQ-A.a")` returns that clause and its claim; unknown id → `None`.
   - `REQ-FIGURE.runs_listed`: write one evidence JSON with `detail.drt`; `editor.runs()` returns it.
   - `REQ-LOOK.scroll_is_one_draw_per_frame`: add a check in `web/test/pointer.mjs` near "the wheel scrolls the pane under it": dispatch several wheel events at once, assert fewer `scroll` calls than events and summed `lines` equal.
   - Add `run("web/test/overview.mjs");` to the node runner in `crates/editor/tests/pointer.rs` (~line 1195) and to the list in `docs/DEVELOPER_GUIDE.md` "Tests to keep green".
3. `cargo run -q -p tracelean-core --bin tracelean-trace -- .` must say `blocking: false`; read findings for `REQ-GUIDE`, `REQ-FIGURE`, `REQ-LOOK` (expect `Unmodeled` — information; no `Dangling`).
4. Update `user_manual.md` §7.4 (card on hover, Δ badge, filters/treemap, runs, tutorial folds, `Space m`) and append a "slice 3" block to `work/ROLLOVER_gui.md`.
5. Only then the full suite, once: `cargo test --workspace --no-fail-fast -- --include-ignored`.
6. User feedback still open: scrolling felt "sickening" — the coalescing fix is untested in the real window; ask the user to try `node web/build.mjs && cargo run -p tracelean-desktop -- demo`. If still rough, next lever: `show()` makes ~11 invokes per draw → add one batched Tauri command; and `Editor::scroll` moves the cursor (`step_lines`), consider scrolling the view only.
7. Remaining plan items: terminal twin for the figures, module dependency graph (plan §3.8), agent timeline (§3.9), export (§3.10), SVG station icons.

## Gotchas
- User rule: while working run only new + impacted tests; full suite once at the end. Be terse, low token use.
- Known failures unrelated to GUI: `fixpoint` (needs `tools/stage0-trace` built); `proofs_are_earned::every_theorem_lean_accepts_earns_its_clause_a_record`. **`proofs_are_earned` rewrites `.tracelean/evidence` and `proofs.json` in the repo** — do not run it casually.
- Never `git stash` here: the tree carries the user's uncommitted work.
- A new Tauri command must also be added to `crates/editor/examples/look.rs`, and tolerated as missing by the stub in `web/test/pointer.mjs` (wrap invokes; see `openOverview`'s `list()` helper).
- New action = 4 places: `act.rs` match, `keymap.rs` action list, `Act.lean` match + action list; plus a key in `assets/keymap.json` (every action must be reachable).
- Welcome text lives twice and must be identical: `welcome.rs` `INTRO`, `formal/TraceLean/Welcome.lean` `introLines`.
- Files not in the editor's workspace snapshot (anything under `.tracelean/`) cannot be opened with `file.open`; that is why the tutorial is a record buffer.
- Tutorial claims must match the keymap: design station is `Space w t r` (also `d`), context menu is `Space c` (`t` toggle, `y` copy), no `Space c o`.
- Zoom is the webview's native zoom, set inside the `theme` Tauri command; do not use CSS `zoom` (breaks `region()`).
