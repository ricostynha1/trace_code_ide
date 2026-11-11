# HANDOVER — what is still missing (after slice 5)

*Written 2026-11-11 (end of session). Repo: `new_principle_tracelean/`. Branch: `gui-usability`.*

Read this file alone to continue. What slice 5 built, with paths, is in `work/ROLLOVER_gui.md` ("Slice 5"). The discussion on proof routes and target languages is in `work/tracelean_formal_ideas.md`.

## Goal
Finish and land the GUI-usability work, then decide the formal-verification direction.

## State

- Slice 5 is committed and pushed on `gui-usability` (2026-11-11): one source commit, then `.tracelean/` as the evidence commit.
- `demo/README.md`, `demo/src/blank.rs`, `demo/.tracelean/theme.json` are the user's own trial edits, left uncommitted: do not commit or revert them without asking.
- `tracelean-trace .` ends `blocking: false`. Findings: Unbound 2, Unmodeled 58, Untested 1 (`REQ-THERMO.to_fahrenheit`).
- Evidence was re-earned and locked after the last source change. `tracelean-trace . --proofs`: 295 of 295 theorems accepted, 2 of 183 models pinned, so Proved is 2 of 416 clauses.
- Tests: editor, terminal and impacted core suites pass after the last change (live mirroring). The whole-workspace run was done BEFORE live mirroring (1007 passed; the failures then were fixed, except `fixpoint`). It has not been rerun since.
- Outside the repo: `~/.config/tracelean/agent.json` was created with `{"claudeConfig": "~/.claude-feup"}` so sandboxes use that Claude configuration.

## Missing, in order

1. **Open a pull request** to `main` when the user asks; none exists for this branch yet as far as this session knows.
2. **Whole suite once more**: `cargo test --workspace --no-fail-fast -- --include-ignored`. Expect only `fixpoint` to fail. It rewrites `.tracelean/`, so run step 9 after it if anything changed.
3. **The user has not tried any of slice 5 in the real window.** `node web/build.mjs && cargo run -p tracelean-desktop -- demo`, then check:
   - `Ctrl+P` finds commands by words and by letters; `Ctrl+O` / `Ctrl+Shift+P` opens a file; `F5` and **↻ Files** refresh.
   - **HTML report** in the overview writes the page and the browser opens.
   - Tooltips on the file-tree marks.
   - Undo tree: current branch first; whole tree by default.
   - Sandbox: `Space a n`, run `sh …/enter.sh`, start `claude`. It must use `~/.claude-feup` and list the `tracelean` skill. Change files in the shell: they appear in the editor, the file tree (`+`) and the undo tree (`agent: …`) without accepting. `Ctrl+Z` goes back and the copy follows. Accept marks `✓ accepted`.
   - Still unconfirmed from slice 4: smooth scrolling, menus closing on a click outside.
4. **Judgements, a person's** (`tracelean-trace . --stale` lists 11): 9 judgements and 2 documents in review (`docs/02-scope.md`, `docs/06-editor.md`). Read with care:
   - `REQ-OBS.no_instruction_channel`: going back in the history now writes into the sandbox copy (`Editor::copy_follows`).
   - `REQ-STRENGTH.proved_only_when_pinned`: text changed (waiver for `@nondeterministic`, withdrawal of held proofs).
   - `REQ-OBS.effects_become_commands`, `REQ-OBS.visible_while_running`: reworded for live mirroring.
5. **Live mirroring, loose ends:**
   - `accepted_at`, `agent_nodes`, `agent_paths` are in memory only: `✓ accepted` and `agent:` labels are lost when the editor closes. Persist them with the history (see `crates/core/src/history/persistence.rs`, `REQ-PERSIST`).
   - Ending a sandbox leaves unaccepted agent changes as unsaved edits. Decide with the user whether that is wanted, or whether ending should ask.
   - A file the person edits (unsaved) and the agent also changes: the agent's content replaces it as a node (undo brings the person's back). No merge. Not tested.
   - The five new REQ-OBS clauses and `REQ-UNDO.current_branch_first`, `whole_by_default` have no Lean model (`Unmodeled`). Candidates that are pure: `history_view::current_first`, `quick::commands`, `explorer::meanings`, `proofcheck::proofs_standing`.
6. **Terminal twin gaps:** tooltips on tree marks and `F5` are window-only (`Space f R` works in both).
7. **Pin more models.** `tracelean-trace . --pins` lists what each of the 181 open models owes; `tracelean-trace . --strength <model>` prints the obligation. This is what raises Proved from 2.
8. **Not built from the plan** (`work/plan_gui_usability.md` §3): §3.8 layer-rule check (allowed module dependencies as an `ARCH-*` clause, violations as findings); §3.9 accept/reject per hunk beside the clause card; §3.10 CI badge per mark; §3.3 sortable matrix with stale cells; §3.5 minimap / inline lens / unclaimed-code highlight; §3.6 live run panel with counter-examples. Screenshots of slices 3–5 were never taken (`layout.stations` must be `left` or `top` to show the station bar).
9. **Earn and lock after any source change**, in this order:
   ```bash
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --proofs     # Lean verdicts; withdraws unpinned proofs
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --coverage   # slow; any .rs change re-runs it
   bash tools/differential-all.sh                                         # sets TRACELEAN_DRT_LINES=1; skips current ops
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --lock
   cargo run -q -p tracelean-core --bin tracelean-trace -- . --stale      # should list only judgements and documents
   ```
10. **Formal direction** (not started; see `work/tracelean_formal_ideas.md`): first experiments are a C runner with CBMC/Frama-C beside DRT, and a Kani pilot for Rust.

## Key paths

`crates/editor/src/lib.rs` (`tick`, `recount_waiting`, `copy_follows`, `accept`, `take_back`, `persist`, `points`, `lock_held`, `palette`, `tree_rows`) · `crates/core/src/trace/proofcheck.rs` (`earns_proof`, `proofs_standing`) · `crates/core/src/drt/proofs.rs` (`held`, `standing_lock`, `run_lean`) · `crates/core/src/surface/{history_view.rs,quick.rs,explorer.rs,act.rs,keymap.rs}` · `crates/core/src/observe/{sandbox.rs,workcopy.rs,watch.rs}` · `formal/TraceLean/{Act.lean,HistoryView.lean,Proofcheck.lean}` · `web/src/{app.ts,overview.ts}` · `web/test/pointer.mjs` · `crates/editor/tests/sandbox.rs` · `crates/tui/src/main.rs` · `tests/journeys.json` · `reqs/agent/{REQ-OBS.md,REQ-SBX.md}` · `reqs/history/REQ-UNDO.md` · `reqs/trace/REQ-STRENGTH.md`

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
- A lock keeps every record it held; a proof record stands only while its model is pinned or `@nondeterministic`. Read the lock through `drt::proofs::held` / `standing_lock` (core) or `Editor::lock_held` (editor), never `lockfile::read` directly, or P shows for unpinned models again.
- `history_graph` (Rust) and `stepOf` (Lean) must push the first child in the same order; `differential_history_view` compares them.
- Adding a case to `dispatchNamed` in `Act.lean` can exhaust heartbeats in `a_missing_target_is_refused_by_name` (it reports as "tactic 'split' failed"); the theorem now runs under `set_option maxHeartbeats 1600000`.
- A terminal sends the same byte for `Ctrl+P` and `Ctrl+Shift+P`: keep open-file on `Ctrl+O`; `tests/journeys.json` is replayed by both frontends.
- The sandbox mounts the skill at `<config>/skills/tracelean`: do not put a symlink there (it would dangle inside the sandbox, whose home is hidden).
- `web/test/pointer.mjs`: the stub's explorer listing ends with a `+` mark (span 19..20) for the tooltip check.
- With a sandbox live, `agent_tree` is "the copy as last looked at", not "what waits": `tick` diffs the copy against it, so anything the editor writes into the copy (`copy_follows`, `take_back`) must also be written into `agent_tree`, or it comes back as an agent change on the next tick.
- Without a sandbox (`observe.start` on the working tree) the old flow stands: `pending` holds observed commands and accept pushes them. `accept`, `accept_file`, `reject`, `reject_file` and the review buffer branch on `self.session`.
- `accepted_at`, `agent_nodes` and `agent_paths` live in memory only; they are not persisted with the history.
- The sandbox panel reads the agent's conversation from the same Claude configuration the sandbox uses (`watch::claude_home(root)`); tests set `TRACELEAN_CLAUDE_CONFIG_DIR`, since `~/.config/tracelean/agent.json` on this machine outranks `CLAUDE_CONFIG_DIR`.
