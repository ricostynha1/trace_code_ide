# HANDOVER — read this alone; it assumes zero context

*Written 2026-11-11. Repo `new_principle_tracelean/`, branch `gui-usability`, remote `origin` (github ricostynha1/trace_code_ide). No pull request exists yet.*

TraceLean = a code editor (Rust core + editor + TUI + Tauri desktop + web frontend in `web/`) whose subject is *requirements → Lean models → code*, with every link a written annotation and every strength earned by a run. Method pages: `skills/00-the-method.md`… (`skills/03-requirements.md` is the requirement format). The app's own requirements are in `reqs/**`; a small example project is `demo/`.

## What exists now (all committed)

- Menus: File / Design / Trace / History / Agent / Window / Help, no command twice (`Editor::leader()` in `crates/editor/src/lib.rs`). Stations are a left icon rail.
- **Every action is a command** (palette `Ctrl+P`): key bar toggle, zoom, raw/formatted (`look.raw`, `Space w r`).
- **Settings are a grammar**: `assets/settings.json` (strict, like `assets/keymap.json`); loader `crates/core/src/surface/setting.rs`; groups by the text before the first dot, fuzzy search, edited in place in the GUI.
- **Markdown buffers are drawn formatted by default**, raw via the command only (no button anywhere, by the user's wish; double-click on a formatted page also toggles). Clickable parts: fold rows, file mentions, requirement links, buttons; theme colours; claim chips (I/T/M/D/P) at the end of each clause line of a formatted requirement.
- **Requirements are pure markdown**: `# REQ-X: Title`, `- status:` / `- decomposition:` / `- refines:` bullets, prose, then `## Clauses` with `- **key**: text` and `  - narrowing: text` under it. The old `---` YAML-header format is **removed** (parser, fixtures, docs). Parser: `parse_prose` in `crates/core/src/trace/requirement.rs`. Clause lines are found by `surface::chips::clause_lines`; anything that scans a requirement file for clauses must use it.
- Requirement views and the tutorial are markdown too (`crates/core/src/surface/requirement_view.rs`, `assets/tutorial.md` with nested `+++ … +++` folds, folded at start, demo-driven walkthrough with a simulated sandbox agent).
- Logo (`assets/logo.svg`, shown in the menu bar).
- Lean model of the requirement parser was **rewritten for the new format** (`formal/TraceLean/Requirement.lean`; old one kept nowhere in the repo — git history). Six REQ-REQDOC clauses re-proved; Rust↔Lean differential agrees on 2000 inputs (`crates/core/tests/differential_requirement.rs`, ignored test).
- Last fix: pane dividers sat inside buffers because formatted-markdown panes changed their own font, and panes are placed in `ch` (`.pane.md-preview > .md` in `web/style.css` now carries the look).

## State of the evidence (be honest: the project is NOT closed)

`tracelean-trace .` → `blocking: false`, findings: **Unbound 2, Unmodeled 68, Untested 2**.
`tracelean-trace . --proofs` → 295/295 theorems accepted, but only **2 of 183 models are pinned**, so *Proved* holds for ~2 of ~416 clauses. Coverage is incomplete, most clauses have no Lean model, and `--stale` lists **17 judgements, 3 documents, 2 differential tests** that need action. Nothing about "100% coverage / all proved" is true yet; do not claim it.

## What is missing, in order

0. **Stress-test the app for speed (user report, not yet investigated).** Opening TraceLean itself as the project (the big repo, not `demo/`) and opening some documents works, but opening others — notably files from the requirements (`reqs/**`) — was very slow or simply did nothing. Next: open this repo in the desktop app and time opening every kind of buffer (each `reqs/**` file, big source files, the overview, findings, design graph, coverage); find what blocks (suspects: the full `index()` rebuild per open, `chips_shown`/`coverage_shown` per frame, `shown` returning whole unwindowed panes for formatted markdown, `links::linked` on large files). Add a repeatable timing test over a large tree. Also: **the bottom bar (status line) should display errors**, and it probably does not when an operation fails or hangs — check that every failing operation (open, parse, Tauri command error) says something in the status line instead of silently doing nothing.

1. **Real-window check.** The user has only partly looked at the GUI. Run `node web/build.mjs && cargo run -p tracelean-desktop -- demo` and confirm: formatted requirement shows chips and clickable files; dividers sit on pane edges (with markdown panes beside code); no raw/formatted button; `look.raw` works from the palette; settings edit in place; tutorial folds; menu reachability; the sandbox flow (`Space a n`, agent edits/creates/deletes show in explorer + undo tree).
2. **Judgements need a person** (`tracelean-trace . --stale`; each prints a `--judge REQ-X.clause --verdict agrees|drift|unmodelable --by <who>` line). Never write a verdict for the user. Includes the six REQ-REQDOC clauses whose model was just rewritten.
3. **Differential tests stale:** `REQ-ACT.everything_is_offered` and one more (`--stale`): `bash tools/differential-all.sh` after the last source change (it was run before the final CSS/JS edits only; no Rust change since, so it may already be current — check `--stale`).
4. **Lean models for the newer clauses** (all `Unmodeled`): settings grammar (`REQ-LOOK.settings_declared`, `setting_edited_in_place`), key bar toggle, logo, markdown formatted-by-default / mentions act / requirement links, `REQ-REQDOC.markdown_document`, the five REQ-OBS live-mirroring clauses, `REQ-UNDO.current_branch_first`, `whole_by_default`. Pure candidates: `history_view::current_first`, `quick::commands`, `explorer::meanings`, `proofcheck::proofs_standing`. Mirror pattern: Rust list ↔ `assets/keymap.json` ↔ `formal/TraceLean/Act.lean` with both-ways tests.
5. **Pin models** (this is what raises Proved): `tracelean-trace . --pins` lists what each open model owes; `--strength <model>` prints the obligation; a `@pins` theorem is a person-reviewed statement.
6. **Unbound 2 / Untested 2**: run `tracelean-trace .` and read the findings; fix by writing the comparison/test, never by exempting.
7. **Small loose ends:** Overview / Search / Open-file buttons are not commands yet (the user's rule: every action must be a command); Tauri window icons should be generated from `assets/logo.svg`; sandbox panel keys `Space w t s` described in the tutorial but not verified live; live-mirroring state (`accepted_at`, `agent_nodes`, `agent_paths` in `Editor`) is in memory only — persist with the history (`crates/core/src/history/persistence.rs`); ending a sandbox leaves unaccepted agent changes as unsaved edits (ask the user what is wanted); agent+person editing the same file is not merged and not tested.
8. **Not built from `work/plan_gui_usability.md` §3:** layer-rule check, per-hunk accept/reject, CI badge per mark, sortable matrix, minimap/inline lens, live run panel.
9. **Formal direction** (not started): `work/tracelean_formal_ideas.md` — CBMC/Frama-C beside DRT, Kani pilot.
10. Open a PR to `main` only when the user asks.

## Earn and lock after any source change (in this order)

```bash
cargo run -q -p tracelean-core --bin tracelean-trace -- . --proofs     # Lean verdicts
cargo run -q -p tracelean-core --bin tracelean-trace -- . --coverage   # slow; any .rs change re-runs it
bash tools/differential-all.sh                                         # sets TRACELEAN_DRT_LINES=1
cargo run -q -p tracelean-core --bin tracelean-trace -- . --lock
cargo run -q -p tracelean-core --bin tracelean-trace -- . --stale      # should list only judgements and documents
```
Do not edit source while coverage/differential runs. Commit `.tracelean/` as its own evidence commit.

## Gotchas

- User rules: run only new + impacted tests while working, the full suite once at the end (`cargo test --workspace --no-fail-fast`); terse answers; do not write evidence marks or verdicts; no exemptions to silence findings.
- Uncommitted on purpose (the user's own trial edits): `demo/README.md`, `demo/src/blank.rs`, `demo/.tracelean/theme.json`, `demo/letssee.txt`. Do not commit or revert without asking. (`demo/reqs/*` were converted to the new format and are committed.)
- Known unrelated failure: `fixpoint` (needs `cargo build --release` in `tools/stage0-trace`).
- Checks in a browser: `web/test/pointer.mjs` (build with `node web/build.mjs`), run also by `crates/editor/tests/pointer.rs`. The checker does not scan `.mjs`.
- Panes are placed in `ch`: never set `font-size`/`font-family` on `.pane` itself.
- Editing a requirement file's format affects: `parse_prose`, `chips::clause_lines`, `requirement_view.rs`, `act.rs` (`requirement_template`, `approve` expects `\n- status: draft\n`), editor `go_to_definition`, and the Lean `Requirement.lean` + DRT generator. Change them together.
- Lean 4.12 (`formal/lean-toolchain`). `lake build` from `formal/`. In proofs, never `exact h` on a state built from string-op `if`s (whnf timeouts): state abstract lemmas over the flag (see `nested_fence'`).
- A new action = `act.rs` match + `keymap.rs` list + `Act.lean` match/list + a key in `assets/keymap.json`. A new Tauri command must also be answered by `crates/editor/examples/look.rs` and the stub in `web/test/pointer.mjs`.
- Only declared shells touch the filesystem (`crates/core/tests/architecture.rs` `SHELLS`).
- Zoom is webview-native (set in the `theme` Tauri command); no CSS `zoom`.
- Slice history and key paths from earlier work: `work/ROLLOVER_gui.md`.
