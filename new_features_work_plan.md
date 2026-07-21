# new_features.md — Prioritized Work Plan

Source: `new_features.md`. Each item below restates the request, ranks it, justifies the rank, and sketches a high-level implementation approach grounded in the actual code (not a rewrite proposal — every plan reuses existing structure). Ranked highest-impact/lowest-risk first.

---

## 1. Kill hardcoded tool text/errors in providers — make tools fully JSON-driven

**Source item:** "Tools hard coded in some places... no hardcoded code of tools should exist on the bedrock provider or the ai_runtime... error messages should be syntactically generated from the JSON."

**Why #1:** This is a maintenance-multiplier bug, not a feature. Every new tool or provider currently means editing prose embedded in Rust (`core/src/ai/bedrock.rs` builds the `<tools>` system-prompt block and error strings by hand, e.g. the hardcoded `"MiniMax invoke: missing function name in <invoke> tag."` at `bedrock.rs:390`). Fixing this now is cheap relative to fixing it after more tools/providers exist, and every other roadmap item that touches tools (LSP tools, undo tools, TUI parity) inherits this cost if left alone.

**How I'd implement it:**
- Extend `ToolFunction`/`ToolSchema` (`core/src/ai/provider.rs:139-153`) with an optional `param_errors: Option<HashMap<String, String>>` or a `validation` block per parameter, authored alongside each tool's JSON schema (`core/src/ai/tools.rs` is the single source today per `docs/TOOL_SINGLE_SOURCE.md`).
- Replace the hand-written error strings in `bedrock.rs::parse_json_body`/`parse_minimax_native` (the `errors.push(...)` call sites at lines 244, 260, 271, 275, 390) with a generic validator: given parsed call args + the tool's schema, diff required vs. provided params and render the message from the schema's own text (falling back to a generic "expected N params, got M" only when the schema doesn't supply a custom message).
- Keep `render_tools_block` (bedrock.rs:61-108) as the only place that turns schema → prompt text, but make the wording (missing-name, malformed-JSON, wrong-arity) pull from schema-level constants instead of literals scattered through the parser.
- Add a couple of unit tests in `bedrock.rs`'s existing test module feeding a tool with a custom per-param error message and asserting it surfaces verbatim.

---

## 2. Finish the cache-prediction accuracy fix (system prompt + tools accounting)

**Source item:** "Cache predictions are a bit off... not considering system prompts and tools."

**Why #2:** Already mid-flight and highest ROI-per-hour on this list — the display-layer bug (log list/detail never showing cache health, and under-counting when it did) is already fixed this session. What's left is two narrowly-scoped bugs in the actual cost/pruning model, both already root-caused:
- **Bug A**: `cost_aware_compact` (`core/src/agent/runtime.rs`, call site ~line 233) feeds `prune_ctx.cached_prefix_tokens` an *absolute* cached-token count (includes system+tools) directly against `entry_position_tokens`, a *relative* offset that excludes system+tools (`estimate_prefix_invalidation` / `batch_prune_decisions` in `cost_trimmed_summary_model.rs`) — a constant-offset mismatch that skews every pruning decision on the Bedrock/MiniMax path the user actually runs.
- **Bug B**: `plan_cache_breakpoints` (runtime.rs ~line 1156) hardcodes `static_tools_tokens=0, dynamic_tools_tokens=0` when calling `CacheMarkerPlanner::plan_markers`, so explicit-marker providers (OpenRouter/Anthropic-style) never get realistic economics for the "after tools" marker candidates.

This is real-money accuracy (cost estimates shown to the user) and the fix is small and isolated — no reason to leave it half-done.

**How I'd implement it:**
- Bug A: compute a `system_tokens + tools_tokens` offset at the `cost_aware_compact` call site (using the already-tracked `prev_sent_tools`/`prev_sent_dynamic_tools` and the existing `estimate_tools_tokens` helper), pass it in, and saturating-subtract it from the hint before assigning `prune_ctx.cached_prefix_tokens`.
- Bug B: thread real tool-token estimates into `plan_markers` instead of `0, 0`; decide (and comment) how `CacheBlock::StaticTools`/`::DynamicTools` map onto the message-index-only `cache_breakpoints` Vec, since that's currently a structural gap, not just a missing number.
- `cargo check --workspace --all-targets` + `cargo test --workspace` (no live-provider call needed for this part — it's pure arithmetic).

---

## 3. Make MYTH/keyboard-only control actually complete

**Source item:** "MYTH command integration is not working well... app is not at the moment capable of being totally controlled using the keyboard" + the deferred bug "which-key... not visible to the user how to use at all."

**Why #3:** This isn't a new feature — `core/src/myth/` (keymap, bindings, actions, surface — ~780 lines) and `WhichKeyBar.tsx` already exist and are architecturally sound (bindings → actions → invertible Commands, per the doc comment in `myth/mod.rs`). The gap is coverage and discoverability, not design. Shipping a feature that's advertised as "control everything by keyboard" but silently isn't is worse than not having it — it's actively confusing users right now, which is why it's ranked above net-new work like LSP/TUI.

**Done (this pass) — root-caused and fixed the which-key discoverability bug:**
Read `WhichKeyBar.tsx`, `Editor.tsx`/`FileTree.tsx`'s key handling, `keymap.rs`, and `gui_backend/src/ipc/myth_commands.rs`. The CSS-sizing theory didn't hold up (`.which-key-bar` in `App.css` has ordinary 13px monospace text, no mode-conditional styling that could make it illegible) — the real bug was in `WhichKeyBar.tsx`'s own logic: it unconditionally returned `null` whenever the mode was `"Main"` (`if (mode === "Main" || bindings.length === 0) return null`), and cleared `bindings` back to `[]` every time the mode machine returned to Main. Since `Main` is both the resting/default state *and* the only state that contains the leader keys (`C-Space`/`C-.` → `Options`, per `ui_settings/keymap.json`), the bar could never show a user the one thing they'd need to discover the whole system: which key opens it. It also only ever populated from the reactive `"myth-mode"` event dispatched by `Editor.tsx`/`FileTree.tsx` on keydown, so before the user's first keypress in one of those two components it had never received any bindings at all, even on a fresh app load.

Fix applied: `WhichKeyBar.tsx` now fetches Main's bindings on mount via the already-existing (but previously uncalled from the frontend) `myth_which_key` IPC command, keeps whatever bindings the last mode change reported instead of discarding them for Main, and the only hide condition left is a genuinely empty binding list. `npx tsc --noEmit` passes. I could not exercise this live (a Tauri host is required for the `invoke` calls to resolve; plain `vite dev` renders the shell but every IPC call rejects) — please confirm in the running app that the bar now shows the `C-Space`/`C-.` hint immediately on load.

**Coverage audit — what MYTH keyboard control covers today vs. not** (answers the user's "what abilities do we have with this now?"):
- `core/src/myth/actions.rs`'s `ActionRegistry` has 12 actions total, all scoped to file/editor operations: `open_file`, `rename_file`, `delete_file`, `create_file`, `save_file`, `undo`, `redo`, `copy`, and 4 semantic-nav actions (`go_parent`/`go_child`/`go_prev_sibling`/`go_next_sibling`). `ui_settings/keymap.json` defines 3 states (`Main`, `Options`, `File`) over exactly those actions.
- The mode machine (`myth_key_event`) is only ever invoked from **two** components: `Editor.tsx` and `FileTree.tsx`. Grepping `react_frontend/components/` for `myth_key_event`/`myth-mode` confirms `AiChatPanel.tsx`, `DiffReviewPanel.tsx`, `UndoTreePanel.tsx`, `TerminalPanel.tsx`, `MenuBar.tsx`, and `EditorDiffBar.tsx` have **zero** MYTH wiring — no actions, no bindings, no key routing. Those panels are not "silently broken" so much as never built out for this system at all.
- That said, they're not keyboard-*inaccessible* today either: they're built from plain `<button>` elements (20 in `AiChatPanel.tsx`, 15 in `EditorDiffBar.tsx`, 7 in `UndoTreePanel.tsx`, etc.), which get ordinary browser Tab-focus and Enter/Space activation for free. So "totally controlled using the keyboard" is accurate for Editor/FileTree's modal, which-key-driven flow, and roughly true elsewhere via plain Tab navigation, but there is no MYTH-style leader-key/which-key discoverability anywhere outside those two components.
- Extending coverage into the other 6 components (new actions for accept/reject-hunk, chat send/stop, undo-tree jump, etc., plus bindings) is real, additional work on the same scale as this point itself — not something to fold silently into "fix the visibility bug." Flagging it as a follow-up rather than doing a rushed, untested pass across six more files in the same commit.

---

## 4. Sandboxed tmp-scripts folder + system-prompt nudge toward scripting

**Source item:** "Suggest the agent do scripting for repetitive tasks... create tmp scripts in `.tracelean/tmp` with full write/execute permission... saves tokens, safe because sandboxing works and is undoable."

**Why #4:** Cheapest item on the list with a direct, measurable payoff (fewer round-trip tool calls = lower cost, which is the same axis as item #2). The hard part — safe, undoable sandboxing — is *done* (`core/src/ai/shell_sandbox.rs`, `PROJECT_ALLOWLIST` already includes `.tracelean/tmp`). This is almost purely: extend an allowlist, add a system-prompt sentence, done.

**How I'd implement it:**
- No allowlist change needed — `.tracelean/tmp` is already in `PROJECT_ALLOWLIST` (`shell_sandbox.rs:26-34`) with full write/execute permission inside the sandbox. This item is purely a prompt change: point the agent at the directory that already exists rather than inventing a new one.
- Add a short paragraph to the system-prompt builder (wherever the agent's base instructions are assembled — check `core/src/agent/context.rs`) suggesting: for repetitive multi-step shell/python work, write a script to `.tracelean/tmp/` and run it, rather than re-issuing many small tool calls.
- No new tool needed — the agent already has a shell-execution tool; this is purely a directory permission + prompt change.
- Verify with a quick manual smoke test (have the agent write+run a script there) since sandbox behavior is easiest to validate by actually exercising it — flag this as one the user should confirm live, consistent with how sandboxing changes have been verified before.

---

## 5. Developer + user documentation (agent-discoverable)

**Source item:** "Create good documentation on TraceLean IDE — for developers (AI agents describing where things are, tools to use to discover more) and proper user READMEs."

**Why #5:** High value but diffuse — unlike #1–#4 there's no single bug driving urgency, and doc quality compounds best once the APIs it describes (tool JSON schema, MYTH action registry, cache model) have stopped shifting under items #1–#3. Starting now on the parts that are already stable (architecture overview, sandboxing model, undo tree) avoids documenting things about to change.

**How I'd implement it:**
- Two tracks, not one doc: `docs/ARCHITECTURE.md`-style agent-facing docs (already partially started: `docs/architecture.md`, `docs/AgentArchitectureChanges.md`, `docs/TOOL_SINGLE_SOURCE.md` exist) vs. a top-level `README.md` user guide (build/run, key bindings, settings).
- For the agent-facing side, the most leveraged doc is a "where things live" map: provider layer (`core/src/ai/`), agent loop (`core/src/agent/runtime.rs`), MYTH surfaces, sandboxing, undo tree — one page per subsystem with file pointers, not prose explaining what the code already says.
- Consider making part of this self-maintaining: a short doc-freshness check (does the file the doc points at still exist / still have the referenced function) run in CI or as a periodic agent task, so docs don't silently rot the way `docs/` already shows some staleness (several docs were deleted this session per git status — worth checking nothing load-bearing was lost).

---

## 6. LSP integration

**Source item:** "We need an LSP integration... synergies with MYTH-like infrastructure."

**Why #6:** High long-term value (real diagnostics, go-to-definition, symbol search — currently absent; grep confirms zero LSP code exists today) but it's a genuinely large subsystem, and the user's own framing ties it to MYTH ("synergies... possibly made in heaven") — meaning the natural hook point (surfaces, actions, undo-safe edits) is more solid once #3 closes MYTH's current gaps. Building LSP against a half-finished action/surface model risks rework.

**On the MYTH synergy specifically — worth being precise about where it's real vs. not:** the client/transport layer (spawning a language server, JSON-RPC over stdio, capability negotiation) has *zero* overlap with MYTH — that's plain infrastructure regardless of how the rest of the app is built. The real synergy is narrower and, on inspection of `core/src/myth/surface.rs`, stronger than I first gave it credit for:
- `SurfaceNode`'s doc comment already says `capture` covers "highlight captures for code" — the struct (capture + line + char range + text + JSON meta) was seemingly designed with exactly this kind of code-level annotation in mind, not just the file tree it currently powers. Diagnostics/hover fit it almost directly.
- **Code actions are the strongest fit of all.** LSP's `codeAction` response is literally "a list of named operations valid at this location" — that's near-exactly MYTH's capture→actions model plus which-key discoverability, arguably a *better* use of that abstraction than the file tree's simple open/rename/delete set.
- Symbol search / outline / references are a natural fit too — same list-navigation pattern `FileTreeSurface` already implements, just symbols instead of paths.
- Rename/code-action edits going through `Command`/`undo_tree` is solid, but that's general architecture reuse available to any feature, not something specific to MYTH.
- **The real open question, not a solved synergy:** `FileTreeSurface` re-parses static content once per view build. Diagnostics need to update on every keystroke in a live-editing buffer — that's a genuinely new capability the surface model doesn't have yet (incremental re-parse), not something that drops in for free. Worth scoping as its own design question rather than assuming it's covered.

**How I'd implement it:**
- New `core/src/lsp/` module: a client over `tower-lsp` or hand-rolled JSON-RPC-over-stdio, one language server process per project root, keyed by language — built independent of MYTH, since this half has no synergy to leverage.
- Model diagnostics/symbols/hovers as data the existing `Surface`/`SurfaceNode` abstraction can render — i.e., LSP becomes another producer of marked nodes, not a parallel UI system. Prioritize code actions and symbol search here first; they're the parts of the surface model that need no new capability.
- Treat live-buffer diagnostics (inline squiggles updating as you type) as a separate, harder sub-problem gated on extending `Surface` to support incremental re-parse — don't block the rest of LSP on it.
- Edits coming back from LSP (rename, code actions) should go through the same invertible-`Command`/undo-tree path everything else uses (`core/src/commands.rs`, `undo_tree.rs`) — no special-cased apply path.
- Start with one language server (whatever the example project already uses — `example/src/*.rs` suggests rust-analyzer is the natural first target) before generalizing.

## 7. Single-file undo (branch-scoped undo within the global tree)

**Source item:** Undoing file 1 currently also undoes file 2's edits because undo is global; wants a per-file "target node" that only rewinds that file's edits.

**Why #8:** Real and correctly diagnosed by the user, but it's a UX refinement of a working system (`core/src/undo_tree.rs` already does proper branch-on-undo tree history, not a linear stack), not a correctness bug — nothing is lost today, it's just coarser-grained than ideal. Worth doing, but after the items above that are either actively wrong (#1–#3) or cheap+high-leverage (#4).

**How I'd implement it:**
- The user's own sketch is right: for a target node reached via `Edit f1 → Edit f1 → Edit f2 → Edit f2`, a "single-file undo on f1" should walk back through the tree to the nearest ancestor node whose outgoing edit was to f1, then replay every *other-file* edit between there and now as a new branch — i.e., it's a tree rewrite (skip-and-reattach), not a new undo mechanism.
- Concretely: add a `UndoTree::rewind_single_file(&mut self, target: NodeId, path: &Path)` that walks `parent` links from `current`, collects intervening `UndoNode`s whose `Command` doesn't touch `path`, and creates a new branch node that applies the target's inverse followed by re-applying those collected commands in order (this is exactly the same shape as the existing hover-diff branch logic the user references, per `Command`/`inverse` already being per-node).
- Before optimizing storage, note the user is right that this is repetitive as a plain tree — worth a follow-up pass on structural sharing (e.g., persistent/immutable tree with path copying) or compact serialization (delta-encode `Command` sequences) once single-file undo is correctness-proven, not before — premature compression risk here is real.

## 8. Agent memory files (cross-task persistence)

**Source item:** "Ways for the model to make memory files so context isn't lost across tasks... prompting + tool creation... after trimmed/context is fully optimized."

**Why #11:** User explicitly deferred this until context/trimming work is "almost done" — which, per #2, it now nearly is. Ranked here (not higher) because it's mostly a prompting/tool-design exercise that benefits from #1 (clean tool-JSON error/param model) so a new "memory" tool doesn't inherit the same hardcoding pattern.

**How I'd implement it:**
- A single new tool (`write_memory`/`read_memory`) scoped to a project-local file (naturally `.tracelean/agent_memory/` alongside the sandboxing/tmp-scripts convention from #4), with the tool's own JSON schema carrying its usage guidance (per #1's pattern) rather than baking instructions into the system prompt.
- Keep it to plain files the agent manages via its normal file tools where possible — a bespoke "skills" format is more machinery than the request calls for; only add structure if plain files prove insufficient in practice.

---


## 9. Bedrock/Claude (Sonnet 5) expansion

**Source item:** "Expand and test infrastructure using Claude models through Bedrock... but first I want all infrastructure optimized to smaller/dumber models."

**Why #10:** The user explicitly self-sequenced this behind cost/infra optimization — items #1, #2, and #4 are exactly that optimization work. Doing this before them means re-validating cost/cache behavior twice (once now, once after the trimming model changes land).

**How I'd implement it:**
- Once #2 (cache model correctness) is closed, add Sonnet 5's model config entry (pricing, context window, `supports_caching`) to whatever catalog `ModelConfig` is sourced from, and run the existing cost/cache integration tests (`tracelean/tests/integration_cost_reduction.rs`) against it before wider rollout.
- This is primarily a config + validation task, not new code — `bedrock.rs` already speaks the Converse API generically.

---

## 10. Lean/formal-methods synergy with code generation

**Source item:** "Really start thinking how lean things and formal methods would interact with code creation."

**Why #12 (last):** Explicitly framed by the user as a "start thinking about" — i.e. exploratory, no concrete deliverable requested, largest unknown scope of anything on the list. Treat as a standing research thread, not a scheduled build.

**How I'd approach it:**
- Don't build anything yet. Spend a short spike identifying the one or two highest-value touch points — e.g., using the existing `spec_conformance`/`coverage` fields already present on `CommitPoint` (`undo_tree.rs:16`) as a hook for attaching a formal-verification result to a commit point, since that plumbing already exists for test coverage and is a natural extension point for proof status too.

---

## Suggested additions (not in the original list)

A few things surfaced while reading the code that seem worth the user's attention:

1. **Docs deletion sanity check.** Git status shows `docs/DESIGN.md`, `docs/HANDOFF-P17.md`, `docs/IMPLEMENTATION.md`, `docs/REQUIREMENTS.md`, `docs/SPEC-todo-p1-p7-and-roadmap.md`, `todo.md` and others deleted but not committed. Worth confirming this was intentional cleanup before it's committed — several of the new root-level `.md` files (`bugs.md`, `tickets.md`, `Regression.md`) look like their replacements, but it's worth a deliberate pass rather than an implicit one, especially since #5 (documentation) will want to build on whatever survives.
4. **Sandbox allowlist visibility.** `PROJECT_ALLOWLIST`/`HOME_ALLOWLIST`/`PROTECTED` in `shell_sandbox.rs` are compile-time constants; as #4 and #9 add more sandboxed-write locations, consider surfacing the *effective* allowlist somewhere in the UI/settings (even read-only) so it's auditable without reading Rust source — small trust/transparency win for a security-relevant list that's growing. (Also this should be on a json files, the project allowllist, and home allowlist and protected. the settings will only like display where this configation file is). (this can be more heneral and under the hood all settings are on jsons files, but can be edited from the UI and discover, think somethin like VScode, but basically new feature).

5. **MYTH coverage audit as a standing CI check**, not a one-time task — once #3 closes current gaps, a lightweight check that every new interactive component registers a MYTH action would stop "control everything by keyboard" from silently regressing again as the frontend grows (same shape as the doc-freshness idea in #5).
6. **Stand up a CI pipeline (GitHub Actions).** Right now there is *no* CI at all — no `.github/workflows`, no `rustfmt.toml`/`clippy.toml`, no lint config on the frontend. "Things are stabilizing" is exactly the right trigger: everything currently relies on someone remembering to run `cargo test`/`tsc` by hand before pushing. Concretely, and checked against what actually exists in the repo:
   - **`rust-check` job**: `cargo check --workspace --all-targets` + `cargo test --workspace`. This is safe to run unattended with zero secrets — the *only* test that hits a live provider is `tests/integration_bedrock.rs`, and it's already behind a `live_bedrock` Cargo feature that's off by default (`tests/Cargo.toml`). Nothing else in `core`/`gui_backend`/`tui`'s test suites is feature-gated or `#[ignore]`d, so the default `cargo test --workspace` is exactly what should run on every push.
   - **`rust-lint` job**: `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --check`. Do this as a two-step change, not one: land `rustfmt.toml`/`clippy.toml` (with whatever the current codebase's implicit style already is) *before* CI starts enforcing them, or the very first run fails wall-to-wall on pre-existing drift instead of catching new problems.
   - **`frontend` job**: `npx tsc --noEmit` — matches the type-check step you already ask me to run by hand today; just needs to run on a schedule instead of only when I remember.
   - **`tauri-build` job (manual/release-only, not per-PR)**: the exact Linux system-package list this needs is already written down in `Dockerfile` (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libsoup-3.0-dev`, `libjavascriptcoregtk-4.1-dev`, `libdbus-1-dev`, `libssl-dev`) — reuse it verbatim in the workflow's `apt-get install` step rather than re-deriving it. Gate this to `workflow_dispatch`/release branches since a full Tauri build is slow and doesn't need to gate every commit.
   - Explicitly *keep* live-provider testing (Bedrock, OpenRouter, anything costing money) out of CI, same convention already established for local work — a human runs those deliberately, not a bot on every push.
7. **Turn `example/` into a scheduled agent-capability regression suite.** `example/reqs/REQ-01.md`..`REQ-03.md` plus `run_tests.py` and the paired `src/`/`tests/unit/` fixture files (`auth.rs`, `search.rs`, `upload.rs`) already look like a hand-built "did the agent implement this spec correctly" harness — it's an eval suite in everything but automation. Given how much of this session's actual work was tuning the pruning/cache-trimming model, a periodic (weekly cron or manual `workflow_dispatch`, *not* per-PR — it spends real tokens and is non-deterministic) run that has the agent solve REQ-01/02/03 against the current model config and diffs pass/fail against the last run would catch capability regressions in the trimming model the same way `integration_cost_reduction.rs` catches cost-model regressions numerically. Output can be as simple as a log file or an issue comment — the value is in having *any* automated signal here, not in a fancy dashboard.
8. **Crash/panic visibility for the packaged app.** Once this ships as a real Tauri desktop app there's no terminal for a user to see a Rust panic from `gui_backend` — today that likely just looks like "the app closed, no idea why." A `std::pan]ic::set_hook` writing panic info to `.tracelean/crash.log` (project-scoped, consistent with the existing convention that TraceLean artifacts live under `.tracelean/`) costs almost nothing and turns an unreproducible bug report into an actual stack trace.
9. **Supply-chain check (`cargo-deny`/`cargo-audit`).** The codebase already treats security as a first-class concern — the entire design of `shell_sandbox.rs`, the `PROTECTED`/allowlist model, `sandboxing_better.md` — and file headers mark this proprietary ("All rights reserved"). Once #6 exists, adding `cargo-deny` (license + duplicate-version hygiene) and an advisory/CVE check as a companion job is a small addition that extends that same security posture to the dependency tree, which nothing currently checks at all.
10. **One scripted smoke test for the single golden path.** Existing guidance already asks for manual browser verification of UI changes, which is right for review but leaves zero regression net between reviews for the one path everything else depends on: open a project → send a chat message → accept a diff. A minimal Tauri-webdriver or Playwright script covering just that path (not broad UI coverage) would catch a silently-broken "accept diff" button or a chat-panel regression introduced while working deep in Rust — cheap relative to the cost of that class of bug shipping unnoticed.
