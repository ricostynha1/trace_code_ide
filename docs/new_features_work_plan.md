# typing very fast seems to break app (teleported to top)
Potentially a performance problem in big files? (what yo do and how to measure this and avoid teletransport?)

# Bug 0 Clicking on Save checkpoint should save all open files to disck (the ones with unsynced state to disk)

This is vecause i am trying to edit using ai agent usgin a shell command, but cannot ever accept the adition with this error:
⚠ This diff no longer matches the file (it changed since this was staged) — discard it and let the agent re-propose the edit.

Also model entered a strange loop
and again
ASSISTANT:
→ run_shell({"command":"echo \"hello world\" >> run_tests.py"})
TOOL:
run_tests.py (modified)
[tool_call_id: call_0]
ASSISTANT:
→ run_shell({"command":"echo \"hello world\" >> run_tests.py"})
TOOL:
run_tests.py (modified)
[tool_call_id: call_0]
ASSISTANT:
→ run_shell({"command":"echo \"hello world\" >> run_tests.py"})
TOOL:
run_tests.py (modified)
[tool_call_id: call_0]
ASSISTANT:
→ run_shell({"command":"echo \"hello world\" >> run_tests.py"})
TOOL:
[run_shell rejected by user — command was NOT executed: echo "hello world" >> run_tests.py]
[tool_call_id: call_0]
ASSISTANT:
→ run_shell({"command":"echo \"hello world\" >> run_tests.py"})

(maybe the sucess infomration it receives is not very clear.)


# BUG 1. Find embedded not working:
ARGS
{"query":"haystack","case_sensitive":false}
RESULT
No matches found (exact or case-insensitive; semantic fallback unavailable: find_embed: embeddings index not initialized. Build it first (rebuild context button).) Your task is to make this embeddings work, so when project selected in background embeddin index shold be made.

Also improve resiliecne of this tool
ARGS
{"query":"*.py","mode":"regex","path":".","max_results":100}
ERROR
Invalid regex '*.py': regex parse error:
    *.py
    ^
error: repetition operator missing expression — escape special chars like . * ( for a literal match.
(maybe say instead o fregex is glob matching unclear really).

## 6.5. Really test and make openrouter work (we can test we the free openrouter models really).
Add test with openrouter, even better as openrouter also has free models some of our integrations test that were using mock provider AI or were using bedrock can be using this openrouter route, that would be cool potentially.

## 6.8 Investigate if openCode will be integradable in my framework .... (inclear but i believe it should be right?)
As i tried to isolate my agent I amd oing using ACP protocol.
(but we totally should allow openCode, or think on it as it would be very usefull in case we did not reach a good enough agentic coding state)

## 7. LSP integration

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



## 8. Agent memory files (cross-task persistence)

**Source item:** "Ways for the model to make memory files so context isn't lost across tasks... prompting + tool creation... after trimmed/context is fully optimized."

**Why #11:** User explicitly deferred this until context/trimming work is "almost done" — which, per #2, it now nearly is. Ranked here (not higher) because it's mostly a prompting/tool-design exercise that benefits from #1 (clean tool-JSON error/param model) so a new "memory" tool doesn't inherit the same hardcoding pattern.

**How I'd implement it:**
- A single new tool (`write_memory`/`read_memory`) scoped to a project-local file (naturally `.tracelean/agent_memory/` alongside the sandboxing/tmp-scripts convention from #4), with the tool's own JSON schema carrying its usage guidance (per #1's pattern) rather than baking instructions into the system prompt.
- Keep it to plain files the agent manages via its normal file tools where possible — a bespoke "skills" format is more machinery than the request calls for; only add structure if plain files prove insufficient in practice.

---

### STOP HERE


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
11. **Revisit standalone-agent prompt consolidation once spec-driven code-gen actually exists.** `ai_module_cleanup_plan.md` findings 6/9 removed the unreachable Elicitation/Formalisation/Implementation/Repair standalone agents (`ai/agents.rs`) and their separate tool-catalog/system-prompt path (`tools.rs::builtin_tool_definitions`/`tools_as_system_prompt`) because none of it was wired to the UI and it had drifted from `data/tools.json`. That logic wasn't reimplemented, only deleted — when spec-code-native spec-IDE features (turning a formalized `REQ-*`/Lean spec into generated code, item #10's Lean/formal-methods thread) are actually built, revisit whether they need their own agent loop/system-prompt (and if so, build it against the current `ToolRegistry`/`SYSTEM_PROMPT` single-source path from the start) rather than resurrecting the deleted standalone-agent machinery as-is.
