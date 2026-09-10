# ROLLOVER — Sandboxed workspace for external agents (Claude Code)

## Goal

Let the user run Claude Code (or anything) themselves inside a tracelean-managed
sandbox — a long-lived reflink copy of the project bound into a `bwrap` namespace —
and have tracelean observe the resulting filesystem changes and mirror them into
its own `AppState` (buffers, file tree, undo tree), plus surface the Claude Code
session's own JSONL transcript live in the AI chat panel. tracelean never launches
or drives Claude Code — it only creates the workspace, watches it, and reads the
transcript Claude Code already writes for itself.

Full design doc / phase plan: `/home/ricostynha/.claude-personal/plans/you-task-is-to-sunny-summit.md`
(approved by the user; 8 phases). This file tracks *implementation* progress against
that plan — read the plan file for the "why" behind each design choice (reflink
copy vs overlay vs bind-through, external-terminal-first, mirror-live, Tier-1-only
network observability).

## Done

**Phase 1 — reusable mutation collection.** `tracelean/core/src/sandbox/diff.rs`.
`collect_tree_mutations(work, real) -> Vec<FsMutation>` — two-way tree walk (no
overlay whiteout tricks needed for a plain copy). Reuses `ai::shell_sandbox`'s
`FsMutation`/`MutationKind`/`is_protected`/`is_allowlisted`/`read_capture`, which
were loosened from private `fn`/`const` to `pub(crate)` in
`core/src/ai/shell_sandbox.rs` (also loosened `HOME_ALLOWLIST`, `PROTECTED`,
`MAX_CAPTURE_BYTES`). 6 unit tests, all passing.

**Phase 2 — session lifecycle.** `tracelean/core/src/sandbox/session.rs`.
`SessionSpec { id, project_root, work_dir, allow_network (default true), created }`.
`create_session()` (`cp -a --reflink=auto`, skips `PROTECTED`+`PROJECT_ALLOWLIST`),
`bwrap_argv()` (binds work dir at the real project path, `.git` bound RW — sessions
intentionally do NOT protect `.git` like the ephemeral agent sandbox does — plus
`.tracelean` ro + `.tracelean/tmp` rw, `HOME_ALLOWLIST`, `~/.claude` + `~/.claude.json`,
`$XDG_RUNTIME_DIR`, network allowed by default), `destroy_session`, `load_session`,
`list_sessions`, `bwrap_available()`, `reflink_supported()`, `capabilities()`. 6 unit
tests including one that actually shells out to `bwrap` and reads a file through it
— passing on this machine.

**Phase 3 — CLI entry point.** `tracelean/core/src/bin/tracelean-sandbox.rs`,
registered as a third `[[bin]]` in `core/Cargo.toml`. `tracelean-sandbox shell
<project-root> <session-id> [-- <command>...]` — loads the spec, builds argv,
`exec`s `bwrap` (no trailing command → `$SHELL`). This is the *only* thing that
enters the namespace; tracelean's GUI never calls it, the user does, in their own
terminal (Phase 2 of the "launching" decision: external terminal first).

**Phase 4 — watcher.** `tracelean/core/src/sandbox/watch.rs`. `SandboxWatcher::spawn`
— `notify` recursive watch on `work_dir`, 300ms debounce (drains a burst before
acting, so one `Command::Batch` per burst, not one per file), calls
`collect_tree_mutations` → filters self-write echoes → `mirror::apply_mutations` →
emits `undo-tree-changed` + `files-changed` + `sandbox-changed` via `EventSink`.

**Phase 4/5 shared — mirror.** `tracelean/core/src/sandbox/mirror.rs`.
`apply_mutations()` — extracted from `ai::tool_executor::materialize_sandbox_run`'s
non-review branch (sessions never use `ReviewSink` — landing straight in the buffer
is the point). `SelfWrites` — content-hash table so a write mirrored in one
direction isn't immediately re-observed and re-applied from the other.
`SandboxLink { work_dir, self_writes }`.

**Phase 5 — IDE write-through.** `AppState` (`core/src/state.rs`) gained
`sandbox_link: Option<SandboxLink>` (`#[serde(skip)]`, runtime-only, not
persisted) + `set_sandbox_link`/`sandbox_link()` accessors.
`service::save_file()` (`core/src/service.rs:123`) now also mirrors the saved
content into `link.work_dir` and registers it in `self_writes` when a link is set.

**Phase 8 (backend half) — transcript parsing.**
`tracelean/core/src/sandbox/transcript.rs`. `TranscriptEvent` enum (UserText,
AssistantText, Thinking, ToolUse, ToolResult, Usage, SystemNote),
`transcript_path(claude_home, project_cwd)` (reproduces Claude Code's `/`→`-` slug
rule), `parse_line()`, `TranscriptTail` (byte-offset-based, `poll()` returns only
newly appended *complete* lines). 4 unit tests passing, verified against real
`~/.claude/projects/*/*.jsonl` shape captured earlier in this session.

**Tests.**
- `cargo test -p tracelean-core sandbox::` → 21/21 passing (includes one real
  `bwrap` execution).
- New integration test `tracelean/tests/integration_sandbox_session.rs`
  (registered in `tests/Cargo.toml`) — end-to-end: real `bwrap` command run inside
  a session → `SandboxWatcher` mirrors it into `AppState` (buffer + undo tree) and
  the real tree → single `undo()` call reverts the whole mirrored batch. Passing.
  Note: disk-level undo restore is deliberately NOT asserted — that's
  `sync_file_operations_to_disk` in `gui_backend/src/ipc/editor.rs` (`pub(crate)`,
  GUI-layer only), a pre-existing concern orthogonal to this feature.

**`cargo build -p tracelean-core --bins --lib` is clean.**

## Also done (Phases 6–8 frontend)

**Phase 6 — IPC (`gui_backend`).** `tracelean/gui_backend/src/ipc/sandbox_commands.rs`,
registered in `ipc/mod.rs` and in `lib.rs`'s `generate_handler![...]` list (the
`mcp_command_consistency_tests` tree-sitter test only checks `mcp_commands.rs`, so
it does not cover this file — commands were cross-checked by hand instead).
Commands: `sandbox_capabilities`, `sandbox_create_session`,
`sandbox_destroy_session`, `sandbox_list_sessions`, `sandbox_shell_command`,
`sandbox_open_terminal` (tries `$TERMINAL` then a fallback list of common
emulators, `-e <bin> <args>`), `sandbox_changes`, `sandbox_revert_file`,
`sandbox_transcript_history` (Phase 8 backfill — see below). New managed state
`SandboxSessionWrapper` (`ipc::sandbox_commands::SandboxSessionWrapper::default()`
in `lib.rs`'s `.manage(...)` chain) holding, per session id, the `SessionSpec`,
the live `SandboxWatcher` (drop = stop), the transcript-tail stop flag, and a
capped (4000-entry) transcript backfill buffer.
`sandbox_create_session` creates the session, spawns the watcher against the real
`AppStateWrapper`, calls `state.set_sandbox_link(...)`, and spawns a transcript-tail
thread that polls for `~/.claude/projects/<slug>/*.jsonl` every 2s until it
appears, then tails it every 500ms, both emitting `sandbox-transcript` events and
appending to the backfill buffer.

**Phase 7 — `react_frontend/components/SandboxPanel.tsx`.** New panel, wired into
`App.tsx` (own `Splitter`, `--sandbox-panel-width`) and `MenuBar.tsx` ("Sandbox"
toggle button). Session list + create/discard, the `tracelean-sandbox shell ...`
command with copy button, "Open terminal" button, live session-changes list
(A/M/D badges, click path → `onFileSelect`, per-file revert). Selecting a session
calls `setActiveSandboxSession()` from the new
`react_frontend/components/sandboxStore.ts` (module-scope store, same pattern as
`diffViewStore.ts`) so `AiChatPanel`'s Sandbox tab follows along.

**Phase 8 frontend — `AiChatPanel.tsx` "Sandbox" tab.** New `Tab` value
`"sandbox"`, a session picker (from `sandbox_list_sessions`, following
`sandboxStore` by default), a read-only hint, and a transcript view reusing the
existing `.ai-msg`/`.ai-msg-thinking`/`.ai-msg-tool-call` CSS classes rather than
the built-in agent's own grouping/expansion state (kept deliberately separate —
extracting a shared component from the 1799-line file's chip-batching logic was
judged higher-risk than the value of reuse). On session change it calls
`sandbox_transcript_history` for backfill, then subscribes to live
`sandbox-transcript` events; both paths dedupe through a `uuid:JSON(event)` key
set so the backfill/live-subscribe race can't double-render. `tool_use` events
are joined with their later `tool_result` (by id) into one row; a `tool_result`
with no matching `tool_use` in view still renders on its own so nothing is
silently dropped. No input box — this tab cannot send anything to the session.

**Frontend verification:** `npx tsc --noEmit` clean (strict mode,
`noUnusedLocals`/`noUnusedParameters` on). `npx vite build` transforms all 631
modules with no errors; it fails only at the final "write to `dist/`" step with an
`EACCES` on `dist/assets` — a pre-existing permission issue on that directory
(likely owned by root from a prior Docker build), unrelated to these changes and
not touched.

**Full-repo verification run:**
- `cargo build --workspace` (core + tui + gui_backend) — clean, no warnings.
- `cargo test -p tracelean-core sandbox::` — 21/21 passing.
- `cargo test -p tracelean-tests --test integration_sandbox_session` — passing.
- `cargo test -p tracelean-tests --test unit` — 116/116 passing (no regressions).

## Also done — External Agent mode replaces the standalone Sandbox tab

Follow-up from the user: no separate "Sandbox" tab in AiChatPanel — instead a
**Built-in / External Agent** mode toggle on the **Chat** tab itself, with
Claude Code's transcript registered as real `InteractionLog`/`SessionStats`
entries (so Log/Stats "just work" for it), cost estimated automatically via
`ai::model_catalog` (no manual price-editing UI — the user explicitly said not
to bother with that once auto-lookup was possible).

**`data/models.json` fix (prerequisite).** `claude-opus-5` — the model a real
Claude Code session actually reports — was missing from the pricing catalog
entirely, so `model_catalog::pricing_for("claude-opus-5")` silently returned
`None`. Root-caused via `scripts/fetch_model_metadata.py`'s source (LiteLLM's
public `model_prices_and_context_window.json`) and confirmed the entry now
exists there ($5/$25 per 1M, matching Anthropic's real pricing). **Did not**
run the full `merge_model_data.py` regeneration pipeline — doing so once
overwrote `cache_min_tokens` and region-prefix ranking backfills that were
hand-patched into `data/models.json` in the last commit (confirmed via two
now-documented regression tests in `model_catalog.rs`;
`git show HEAD:tracelean/data/models.json` vs a full regen reintroduced both
failures). Instead added exactly one entry (`claude-opus-5`, provider
`anthropic`), hand-mirroring the sibling `claude-sonnet-5`/`claude-opus-4-8`
entries' shape/fields (`git diff` on `data/models.json` is a clean 23-line
insertion, nothing else touched). New regression test:
`model_catalog::tests::pricing_for_finds_claude_opus_5`.

**`core/src/sandbox/cost.rs`** (new). `estimate_cost(model_id, fresh_input,
output, cache_read, cache_write_5m, cache_write_1h) -> Option<(TokenUsage,
CostEstimate)>` — prices Claude Code's own reported usage using
Anthropic's documented, model-independent cache ratios (read 0.1x, write
1.25x/2x for 5m/1h TTL — confirmed exact values, not guessed) against
whatever base rate `model_catalog::pricing_for()` has for the reported
model. Returns `None` for a model not yet in the catalog (rather than
inventing a price) — the live transcript view still shows those turns,
they just don't get logged/costed. 3 unit tests, including one against the
exact usage shape captured from a real transcript earlier in this feature's
development.

**`core/src/sandbox/transcript.rs` — `Usage` event reshaped.** Was one
combined `cache_creation_tokens` field; now `cache_write_5m_tokens` +
`cache_write_1h_tokens` separately, parsed from the transcript's nested
`usage.cache_creation.{ephemeral_5m,ephemeral_1h}_input_tokens` (falls back
to treating the aggregate as all-5m if that nested object is absent — older
transcript format). 2 new tests.

**`core/src/ai/log.rs` — `InteractionLog::record_external(...)`.** A third,
narrow constructor alongside `record_success`/`record_failure` — takes
already-assembled pieces directly (no `AiRequest`/`AiResponse` exists for a
session tracelean never called) and leaves provider-call-specific fields
(`request_messages`, `tool_schemas`, `request_raw`, `context_window`)
empty/unknown rather than inventing values. `context_window: 0` is exactly
the sentinel the Log tab's frontend already treats as "unknown, skip the %
calculation" (`AiChatPanel.tsx` — guards on `entry.context_window ?` truthy
before dividing), so no frontend special-casing was needed there. 1 unit test.

**`core/src/ai/tracking.rs` — `SessionStats::record_external(...)` +
4 new segregated fields** (`external_requests`, `external_input_tokens`,
`external_output_tokens`, `external_estimated_cost_usd`). Deliberately never
touches `total_cost_usd` — that field gates the built-in agent's spend cap
(`AiChatPanel.tsx:708`, confirmed by an earlier research pass), and blending
in an *estimate* against a session that might be on a subscription plan
(not metered per-token at all) would let external usage silently trip or
mask that cap. 1 regression test asserting the totals stay at zero.

**`gui_backend/src/ipc/sandbox_commands.rs` — wiring.** `sandbox_create_session`
now also takes `State<AiLogWrapper>` + `State<AiSessionStatsWrapper>`.
`spawn_transcript_tail` gained a `TurnBuffer` (accumulates `AssistantText`/
`Thinking`/`ToolUse` pieces between `Usage` events — Claude Code reports
these as separate records but usage/cost once per turn, so `Usage` is the
natural flush point, one `InteractionEntry` per turn, mirroring how the
built-in agent logs one entry per provider call). On flush: `cost::estimate_cost`
→ `InteractionLog::record_external` + `SessionStats::record_external` →
emits `ai-stats-updated` so the Stats tab picks it up live. Unpriced models
(not yet in the catalog) still emit the live `sandbox-transcript` event for
the chat view; they just don't get logged/costed.

**Frontend restructuring (`AiChatPanel.tsx`).** Removed `"sandbox"` from the
`Tab` type and its tab button entirely — deleted the old duplicate rendering
block. Added `ChatMode = "built-in" | "external"` + a toggle row at the top
of the **Chat** tab's content (session picker + refresh appear only in
`external` mode). The `.ai-messages` list branches on `chatMode`: `external`
renders the (unchanged) transcript-event rendering that used to live under
the standalone tab; `built-in` is the original content, now wrapped in a
fragment instead of being the only option. `sendMessage()` short-circuits
with a redirect message when `chatMode === "external"`; the textarea/Send
button are disabled and re-labeled in that mode too — this is the only place
the "can't talk to the external agent from here" constraint is enforced.
Stats tab gained an "External Agent Sessions (estimated)" table, rendered
only when `external_requests > 0`, explicitly captioned "not counted against
the spend cap above." `App.css` renamed the old `.ai-sandbox-toolbar` section
to `.ai-chat-mode-toggle` (adds `.active` button styling); `.ai-sandbox-usage`/
`.ai-sandbox-readonly-hint` kept as-is.

**No manual price-override settings UI** — the user explicitly dropped that
requirement once auto-lookup via `model_catalog` covered the actual gap
(`claude-opus-5` missing). Nothing to build there.

**Verification:** `cargo build --workspace` clean. `cargo test -p
tracelean-core --lib`: 288 passed, 2 pre-existing failures unrelated to this
work (see below). `cargo test -p tracelean-tests --test unit --test
integration_sandbox_session`: 117/117. `cargo test -p tracelean --lib`
(gui_backend, includes the mcp-command consistency check): 1/1. `npx tsc
--noEmit`: clean under strict mode.

**Pre-existing, unrelated test failures** (confirmed present on a clean
`git show HEAD:tracelean/data/models.json` checkout, before any change in
this session touched that file): `model_catalog::tests::
region_prefixed_claude_entries_carry_the_same_ranking_as_their_bare_sibling`
and `...enrich_surfaces_ranking_for_a_region_prefixed_bedrock_claude_model`.
Not touched — out of scope, flagging so they aren't mistaken for a
regression from this feature.

## Bug fix — second sandbox session on the same project showed nothing

User report: created a second sandbox session for a project already used by
an earlier one; the earlier session's transcript showed fine in the AI Chat
External Agent tab, but the new session's live conversation never appeared.

**Root cause:** every sandbox session for the same project binds its work
dir at the *identical* real project path (`sandbox::session::bwrap_argv` —
paths inside must match paths outside), so Claude Code's own path-based
project slug is identical across sessions too. `transcript_path()` just
picked "the newest `.jsonl` in that slug directory" — the first time a new
session's tail looked (before its own `claude` had written anything yet),
that was the *older* session's still-live file, and once `tail` found
something it never looked again. The new session's tail got permanently
stuck watching someone else's transcript.

**Fix:** `transcript_path_excluding(claude_home, project_cwd, exclude:
&HashSet<PathBuf>)` in `core/src/sandbox/transcript.rs` — same lookup, minus
paths already claimed. `gui_backend/src/ipc/sandbox_commands.rs` gained a
process-wide `claimed_transcripts()` registry (`OnceLock<Mutex<HashSet<PathBuf>>>`);
each session's tail claims its file into both that registry and its own
`ActiveSession.claimed_transcript` cell once found, so a second session's
tail correctly skips the first's file and keeps polling until its *own* new
`.jsonl` appears. `sandbox_destroy_session` releases the claim, so a later
`claude --resume` of the same underlying file can still be picked up.
`transcript_path()` (no exclusion) kept as a thin wrapper over the new
function for the one existing test that uses it. New regression test:
`transcript::tests::excludes_already_claimed_paths_from_the_newest_pick`.

Verified: `cargo test -p tracelean-core sandbox::transcript::` 8/8 (was 7).
Full sweep unchanged otherwise: 289/291 core (same 2 pre-existing,
unrelated failures), 116/116 unit + 1/1 integration in the tests crate,
gui_backend consistency check 1/1, `tsc --noEmit` clean.

## Bug fix — transcript stopped updating after the first `claude` process ended (same sandbox)

User report, immediately after the previous fix: within *one* sandbox
session, the External Agent tab showed the first couple of turns then
stopped — even though many more messages were sent. Terminal showed
"Welcome back!" (a fresh `claude` launch/resume in the same terminal).

**Root cause:** the previous fix only handled *different* sandbox sessions
fighting over which file to claim. It didn't handle the same sandbox
hosting *multiple `claude` process lifetimes* — exit and relaunch,
`--resume`, "Welcome back" all start a **new** transcript file rather than
appending to the old one. `spawn_transcript_tail` only searched for a file
when `tail` was `None`; once found, it never looked again, so it stayed
locked onto the first `claude` process's now-dead file forever.

**Fix:** moved the "find the file to follow" check from a one-time
bootstrap into the top of every loop tick (~500ms–2s). It picks the newest
`.jsonl` not claimed by some *other* session (this session's own current
claim is always eligible against itself); when that's a different path
than the one currently tailed, it releases the old claim, takes the new
one, replaces `tail`, and resets `TurnBuffer` (a partial turn against an
abandoned file shouldn't get folded into the new process's turns). This
subsumes the original bootstrap logic — no separate `tail.is_none()`
branch needed anymore.

Not unit-tested directly (the switching logic lives in a private
thread-spawning fn in `gui_backend`, not easily isolated); verified by
full rebuild + the existing suite staying green — `sandbox::transcript::`
core logic it depends on (`transcript_path_excluding`) already has
coverage. If this needs tightening later, the natural next step is
extracting the "which file should I be tailing right now" decision into a
pure, testable function in `core/src/sandbox/transcript.rs` that
`sandbox_commands.rs` just calls each tick.

## Feature — destroy sessions left over from a previous app run + bulk destroy

User report: had 4 sandbox sessions piled up (dev-mode restarts create a
fresh process each time, but old session directories persist on disk); the
"🗑" destroy button failed with `session 'X' is not active in this window`.

**Root cause:** `SandboxPanel` lists sessions from disk
(`sandbox_list_sessions`, always reads `.tracelean/sessions/`), but
`sandbox_destroy_session` required an in-memory `ActiveSession` entry —
which only exists for sessions created in the *current* process. Any
session from before a restart was listed but undestroyable.

**Fix:** `sandbox_destroy_session` now falls back to `session::load_session`
+ `session::destroy_session` (pure disk cleanup, no watcher to stop) when
the id isn't in the active map, factored into a shared `destroy_one(...)`
helper. Added `sandbox_destroy_all_sessions` — best-effort bulk cleanup
over every persisted session for the project (active or not), collecting
per-session failures instead of aborting on the first one, returned to the
frontend as a list. `SandboxPanel.tsx` gained a "🗑 Destroy all (N)" button
next to "+ New sandbox session" (only shown when sessions exist), behind a
`window.confirm` since it deletes real working-copy directories.

Note: this only cleans up the session *directory* — it does not (and
cannot, no PID is tracked) kill a `bwrap`/`claude` process still running in
an open terminal for that session. The terminal itself needs to be closed
separately; deleting its bind-mounted work dir out from under a live shell
is an edge case left as-is (bind mounts keep the underlying dentries alive,
so it degrades rather than crashing, but isn't a clean state to leave a
sandbox in — worth a "close your terminals first" hint if this comes up
again).

Verified: full rebuild clean, `tsc --noEmit` clean, same 289/291 core +
116/116 unit + 1/1 integration + 1/1 gui_backend consistency (2 pre-existing
unrelated model_catalog failures, unchanged).

## SECURITY FIX — sandbox exposed the whole home directory read-only

User caught this live: inside a sandbox for `example/`, `claude` ran
`ls -la /home/ricostynha/Desktop/trace_code_ide` (the project's *parent*)
and could see everything there — sibling projects, other personal files.
Rightly called this dangerous.

**Root cause:** `bwrap_argv` (`core/src/sandbox/session.rs`) started from
`--ro-bind / /` — the entire host filesystem, read-only, visible inside
every sandbox — then punched writable holes for the project and a handful
of allowlisted dirs. This mirrors the pre-existing convention in
`ai::shell_sandbox::run_overlay` (built for the built-in agent's own
ephemeral shell commands, where "contain writes, don't worry about reads"
was an intentional, documented tradeoff — see that file's header comment).
For a long-lived session hosting an external, less-trusted agent, read
access to the user's *entire home directory* is a real exposure, not a
documented tradeoff anyone signed off on.

**Fix:** before anything else is bound, `--tmpfs $HOME` blanks the whole
home directory (bind order matters in bwrap — later operations win at a
given path, and this now runs before the project bind, so binds *inside*
$HOME that follow — the project, `.cargo`/`.rustup`/`.cache`/`.npm`,
`.claude`/`$CLAUDE_CONFIG_DIR`/`.claude.json` — still land correctly on top
of it). System paths (`/usr`, `/etc`, `/bin`, toolchains) are untouched —
this is specifically about personal files under $HOME, which is what got
demonstrated as exposed. Also added `.gitconfig` (read-only) to the
allowlist, since `git commit` needs an author identity and this is a
low-risk, no-write addition.

**Deliberately not restored:** shell rc files (`.bashrc`/`.zshrc`/...) —
an interactive shell in the sandbox now gets a default environment, not the
user's customized one — and `.ssh` (private keys) — SSH-authenticated git
operations aren't supported from inside a sandbox by default. Both are
easy to add back later if that tradeoff turns out wrong; flagged rather
than silently deciding for the user.

**New regression test** (the important one — a real bwrap run, not just an
argv-shape assertion): `sandbox::session::tests::
sandbox_hides_home_directory_siblings_but_keeps_the_project_visible` —
creates a fake $HOME with a sibling secret file and a nested project,
proves the sibling is unreadable from inside the sandbox while the
project's own file still is.

Verified: 31/31 sandbox tests (was 30, +1), full workspace rebuild clean,
integration test still passes, same 2 pre-existing unrelated model_catalog
failures.

**This does not fix:** siblings of directories *above* $HOME (e.g. if the
project lived outside $HOME, other paths at that level are still visible
via the original `--ro-bind / /`) — the demonstrated leak was specifically
about the home directory tree, which is what this closes. A fully
deny-by-default sandbox (bind nothing, allowlist exactly what's needed
system-wide too) would be more thorough but higher-risk to toolchain
compatibility and wasn't what was demonstrated as broken — worth flagging
if the user wants to go further.

## Bug fix — undo tree "file mode" couldn't get back to a file's original state

User report (unrelated to sandbox work — pre-existing editor bug):
"clicking undo tree on file mode, the initial state of the file is not
registered, only the first operation, so I can never go back to the
beginning."

**Root cause (two compounding issues, found via investigation before
fixing):**
1. `AppState::record_file_open()` (`core/src/state.rs`) only ever pushed a
   base-state node when the *whole tree* was empty (once per session, for
   whichever file happened to be opened first) — every other file got no
   base node at all. `load_file()` seeds the buffer directly, by design
   bypassing the command system ("initialization, not a command").
2. Even in the one case a base node existed, it used `Command::Batch {
   commands: vec![] }` (`UndoTree::push_initial`) — untargeted, no file
   association. The per-file filter in `get_undo_tree`
   (`gui_backend/src/ipc/editor.rs`) keeps a node only if
   `command_affects_file(&node.command, f)` is true, and `Batch{vec![]}`
   matches *no* filter string. So the one base node that did exist was
   unconditionally stripped from every per-file ("file mode") view — file
   mode's earliest visible node was always the file's first real edit.

**Fix — matches the codebase's "everything is a command" philosophy**
(structural, not a display-only patch):
- `UndoTree::push_file_base(command, inverse)` (`core/src/undo_tree.rs`) —
  new primitive alongside `push`/`push_initial`. Unlike `push`, it does
  **not** parent under `current` and does **not** move `current` — opening
  a file isn't "doing something at the current position," and other
  files' in-progress edit chains must be unaffected by it. Parents at the
  tree's true root (or becomes an independent root itself if the tree is
  genuinely empty).
- `AppState::record_file_base(path, content)` (`core/src/state.rs`) —
  builds a same-old/same-new no-op `Command::Replace { file: path, at: 0,
  old: content, new: content }` and pushes it via `push_file_base`. Being a
  real `Replace` with the file's own path, `command_file`/
  `command_affects_file` correctly recognize it as *this file's* history —
  no change needed to the per-file filter in `editor.rs`, it just starts
  working.
- `service::open_file()` calls it right after `load_file()`, only on a
  genuinely fresh load (the function already early-returns via
  `get_content` when the buffer already exists, so no duplicate-node risk
  on reopening a file already in this session, and none across a restored
  checkpoint either, since `save_checkpoint` persists the whole `AppState`
  — buffers and `undo_tree` together — directly, no command-log replay
  involved).
- `command_summary` (`core/src/lib.rs`) broadened its no-op branch from
  `old.is_empty() && new.is_empty()` to `old == new`, labeling any such
  node (including this new one) `"Initial @path:at"` — the undo-tree
  panel's `cmdMarker` already special-cases anything starting with
  `"Initial"` (the tree-wide root's own label), so this renders correctly
  with **zero frontend changes**.

**Why jumping to it is correct, not just cosmetically present:** `jump_to`
(`undo_tree.rs`) computes an LCA-based undo/redo path via the tree's real
parent chain — this is unchanged, whole-tree, cross-file-affecting
behavior for *any* historical jump today, not something new introduced
here. Traced through by hand and confirmed with a real test: undoing back
through a file's real edits (whatever they're parented under) always lands
exactly on that file's pre-edit content, which is by construction identical
to what the base node's `old`/`new` already hold — so applying the (no-op)
base command on top changes nothing, and the net result is correct
regardless of the base node's exact position in the tree relative to those
edits.

**New tests** — `tests/unit/test_undo_tree.rs`:
`push_file_base_is_reachable_but_does_not_move_current` (pure tree-level:
doesn't move `current`, node carries the right file). `tests/unit/
test_state.rs`: `record_file_base_lets_undo_tree_jump_back_to_the_pre_edit_state`
— the real acceptance test: load a file, record its base, make two real
edits, `jump_to_node` the base id, assert content is back to the original.
This is the exact user-reported operation, not just an internal-shape
assertion.

Verified: 118/118 in the tests crate (was 116, +2), 290/292 core (same 2
pre-existing unrelated model_catalog failures), full workspace rebuild
clean, gui_backend consistency check 1/1, integration test 1/1.

**Follow-up — the exact gap flagged above bit immediately:** user reported
the identical symptom for a file they never opened, that an agent edited
directly. Confirmed: `ai::tool_executor`'s edit tools and
`sandbox::mirror::apply_mutations` both call `load_file` to seed a buffer
with pre-edit content *before* applying the real edit, same as
`service::open_file` — but only `open_file` had been given the explicit
`record_file_base` call. Made this systemic instead of per-call-site:

- **`AppState::load_file` itself** now creates the base node (guarded by a
  new persisted `files_with_base_node: HashSet<PathBuf>` field so re-loading
  an already-tracked path — reopening a tab, a checkpoint restore — doesn't
  push a duplicate). `record_file_base` as a separate public method was
  removed; `service::open_file`'s explicit call was removed too, now
  redundant. Every current and future `load_file` caller — the built-in
  agent's edit tools, the sandbox mirror, the ACP server, the editor's own
  open — gets this automatically, with nothing to remember to call.
- `clear_history()` now also clears `files_with_base_node` — a cleared
  tree has no base nodes either, or a file loaded again after a clear
  would never get a new one.

**A real bug this surfaced in `UndoTree::redo()` itself** (pre-existing,
not introduced by this fix, but only reachable once the fix's first draft
started pushing base nodes as the *first* thing in a tree): its
`current == None` fallback finds "the first node with no parent" by
insertion order — an implicit single-root assumption. `push_file_base`'s
first version parented at the true root but *never* touched `current`,
so if a fresh session's first-ever node was a base node, the next real
edit (also with `current == None` at that point) created a **second**
independent parentless node — and `redo()`'s naive lookup would silently
redo to the wrong one instead of failing loudly. Caught by 11 test
failures across coalescing, redo, and property-based round-trip tests —
not a subtle one.

**Fix:** `push_file_base` now claims `current` when (and only when)
`current` is already `None` — nothing else is "in progress," so this base
node becomes the one true root, and the next real edit (to *any* file)
correctly parents under it instead of becoming a second orphan. If
`current` is already `Some` (another file's edit chain is active),
loading/opening a new file still leaves it alone, exactly as designed —
proven with a dedicated test (`agent_editing_a_second_file_does_not_steal_current_from_the_first`).

**Existing tests updated, not just left broken:** three tests
(`rapid_undo_redo_stability`, `test_interleaved_user_and_agent_edits_undo_all`,
plus the two new ones from the first pass) hardcoded exact undo/redo
step counts from before a base node existed — updated to account for the
one extra real step (undoing/redoing the base node itself), each with a
comment explaining why. This is a real, if minor, UX-visible change: on a
freshly-loaded file with disabled/no coalescing, there is now one
additional no-op-but-real undo step at the very beginning (landing you
*on* the base node) before undo truly does nothing — that's the fix
working as intended, not an artifact.

Verified: 120/120 in the tests crate (was 116 at the start of this bug,
+4 net new tests), 290/292 core (same 2 pre-existing unrelated
model_catalog failures), full workspace rebuild clean, gui_backend
consistency check 1/1, integration test 1/1, `tsc --noEmit` clean.

## Bug fix — "3 initial states" after opening one file and editing it

User report, immediately after the previous round: "when clicking on a
file and doing an edit, like 3 initial states happen on the graph." Traced
this to an interaction I hadn't accounted for between the new
`push_file_base` mechanism and **pre-existing** logic in
`service::open_project`.

**Root cause:** `open_project` calls `record_file_open()` (→
`push_initial()`) on *every* fresh project, unconditionally, *before* any
file is ever opened — an existing "Bug 4: baseline commit point" feature.
`push_initial` sets `current` to its own untargeted empty-`Batch` node.
`push_file_base`'s condition from the previous fix — "claim `current` only
when it's `None`" — never fired for the *first file opened in any real
session*, because `current` was already `Some` (the project's placeholder,
not the file's own base). So that file's first real edit parented under
the *placeholder* too, not under its own base node — becoming a **sibling**
of it instead of a child. In file mode, a node whose real parent is outside
the filtered set renders as its own disconnected root — so both the base
node and the first edit showed up as separate "initial-looking" circles for
one file after one edit. (My test suite didn't catch this because every
test used `AppState::new()` + `load_file` directly, with `current` genuinely
`None` — none of them reproduced `open_project`'s exact call sequence.)

**Fix:** `push_file_base` now claims `current` whenever it's `None` **or**
pointing at a node with no file of its own (`command_file` returns `None`
— i.e. nothing file-specific has happened yet, which is exactly what the
project-open placeholder is). Once `current` legitimately belongs to some
file — its own base, or a real edit to it or another file — it's never
stolen, preserving the *other* half of the design (opening/editing a
second file must not disturb a first file's in-progress chain, still
covered by `agent_editing_a_second_file_does_not_steal_current_from_the_first`).

**New test, reproducing the exact real-app sequence** (not just
`AppState::new()` + `load_file` in isolation):
`test_state::first_file_in_a_real_session_chains_under_its_own_base_not_the_project_placeholder`
— replays `open_project`'s baseline step (`record_file_open` +
`mark_commit_point`) then `open_file`'s sequence, and asserts the first
real edit's `parent` is the file's own base node id, not the placeholder.

Verified: 121/121 in the tests crate (was 120, +1), 290/292 core (same 2
pre-existing unrelated failures), full workspace rebuild clean, gui_backend
consistency check 1/1, integration test 1/1, `tsc --noEmit` clean.

## Investigated — file content duplication report (NOT confirmed fixed, be honest about this)

User report: after Claude Code appended a line to `hello.py` inside a
sandbox, a later `cat` of the same file (read from *inside* the sandbox,
i.e. `work_dir` via the bind — nothing to do with the real tree) showed
the entire file content duplicated back-to-back. This is a data-corruption
report, more serious than the undo-tree display bugs, and warrants extra
honesty about what's actually been verified versus guessed.

**What I tried and could NOT reproduce:** a clean single-session test —
create a session, run the user's exact `printf >> hello.py` inside it via
real `bwrap`, then let several more idle debounce cycles pass with nothing
further changing — content stayed correct and stable on both `work_dir`
and the real tree throughout
(`repeated_watcher_ticks_on_a_stable_file_do_not_duplicate_content` in
`tests/integration_sandbox_session.rs`). Single-session mirroring, even
under redundant repeated ticks, is solid.

**A real, related gap I did find and fix, but have NOT confirmed is THE
cause:** nothing stopped multiple sandbox sessions for the *same* project
from being simultaneously active with independent watchers —
`sandbox_create_session` unconditionally spawned a new one every call.
The user's own session history (many distinct session IDs created against
`example/` across this conversation, several never destroyed) shows this
has actually been happening. A first hypothesis — that a second, stale
session's watcher would "fight" an edit made through a live one by
reverting the real tree — was tested directly and **did not occur**:
`SandboxWatcher` is purely event-driven on its *own* `work_dir` (via
`notify`), so a session whose own `work_dir` never changes never ticks at
all, regardless of what happens to the shared real tree. That specific
race doesn't exist. (Test written to check this was removed after it
disproved the hypothesis rather than kept around asserting something
false.)

Multiple simultaneous sessions for one project remains a real problem
independent of whatever caused the duplication: they all mirror into the
same `AppState` (`state.apply()` calls from independent watcher threads,
interleaved with no coordination), and `AppState.sandbox_link` (which
`service::save_file` mirrors IDE saves *into*) is a single field last-writer-
wins — whichever session was created most recently silently becomes "the"
one, while older sessions' watchers keep independently applying their own
mirrored edits to the same shared state regardless. That's a real
correctness hazard even without a confirmed reproduction of this exact
symptom.

**Fix:** `sandbox_create_session` now destroys every *other* active
session for the same project before creating the new one — exactly one
watched session per project, always. Multiple *terminals* into that one
session remain fine (`tracelean-sandbox shell` can be re-run any number of
times against the same session id).

**Action needed, not just a code fix:** this only prevents *future*
pileup — it does not retroactively stop watcher threads already running
in the currently-live `cargo tauri dev` process from sessions created
earlier in this conversation. **A full app restart is needed** to kill
those before the guard takes effect; hot-reload alone won't do it.

**Honest bottom line:** the multi-session guard is a legitimate, real fix
worth keeping regardless. Whether it's *the* fix for the reported
duplication is unconfirmed — if it recurs after a clean restart with
exactly one active session, the next things to check are: was `hello.py`
also open in tracelean's own editor tab at the time (a second write path
via `service::save_file`'s mirror-out, not exercised by any test here),
and the exact session id / terminal in use when it happened.

Verified (the parts that are verified): 122/125 in `tests crate` +
core combined... concretely: `tests/integration_sandbox_session.rs` 2/2,
`cargo test -p tracelean-core --lib` 289/292 (same 2 pre-existing
model_catalog failures + 1 known-flaky bwrap tempdir test, confirmed
passing again in isolation), gui_backend consistency check 1/1, full
workspace rebuild clean, `tsc --noEmit` clean.

## Not done / deliberately deferred

- **Re-attaching a watcher to a session from a previous app run.**
  `sandbox_list_sessions` lists persisted specs from disk, but only sessions
  created in the *current* process have a live `SandboxWatcher` in
  `SandboxSessionWrapper` — old ones show up for visibility/destroy only, not live
  mirroring. Would need a `sandbox_attach_session` command that re-spawns a
  watcher for an existing `SessionSpec`.
- **`bwrap_argv` has no `--setenv`.** No env vars are threaded into the sandboxed
  process (network config, `CLAUDE_CONFIG_DIR` for per-session transcript
  isolation, `ANTHROPIC_API_KEY` fallback if keyring auth doesn't survive
  `--ro-bind / /`). Only tested so far with plain `sh -c` commands, not an actual
  `claude` invocation authenticating and talking to the API — that end-to-end run
  is still the real remaining risk, flagged in the plan doc's "Gotchas" section
  too.
- **IDE-side deletes aren't mirrored into the session work dir.** Only
  `service::save_file` (create/modify direction) mirrors out; deleting a file in
  tracelean while a session is active won't remove it from the sandbox.
- **`collect_tree_mutations` has no `skipped`/`blocked` reporting** (unlike the
  overlay path's `SandboxRun`) — binary/oversized files are silently dropped from
  the mirror, by design (documented as a simplification in `diff.rs`'s doc
  comment).

## Gotchas hit / worth knowing before continuing

- `SessionSpec.project_root` is **canonicalized** in `create_session` — pass the
  same canonicalized value everywhere or path comparisons (e.g. self-write lookups
  keyed by relative path) can silently miss.
- `bwrap_argv` currently has **no `--setenv`** — env vars for the launched process
  (network config for `claude`, `CLAUDE_CONFIG_DIR`, `ANTHROPIC_API_KEY` fallback if
  keyring auth doesn't survive `--ro-bind / /`) are not yet threaded through. This
  is the most likely blocker for a real end-to-end "run `claude` in the sandbox and
  it authenticates" test — untested so far, only `sh -c 'echo/rm'` has been run
  inside a session.
- `collect_tree_mutations` has **no `skipped`/`blocked` reporting** (unlike the
  overlay path's `SandboxRun`) — binary/oversized files are silently dropped from
  the mirror. Fine for now; flagged as a known simplification, not a bug.
- IDE-side **deletes are not mirrored** into the session work dir yet — only
  `service::save_file` (Create/Modify direction) mirrors out. If the user deletes a
  file in tracelean while a session is active, the sandbox won't see it disappear
  until... never, currently. Low priority (edits are the common case) but worth a
  line in Phase 6/7 if there's budget.
- `--reflink=auto` requires **GNU coreutils' `cp`**; fine on this Linux dev machine,
  worth a `cp --help | grep reflink` sanity check if this ever runs somewhere else.
- Scratch files from manual testing (`/tmp/.../scratchpad/sbtest`,
  `/tmp/.../scratchpad/probe.rs`) were cleaned up — nothing left outside the repo
  tests.

## Key paths

- `tracelean/core/src/sandbox/{mod,diff,mirror,session,watch,transcript}.rs`
- `tracelean/core/src/bin/tracelean-sandbox.rs`
- `tracelean/core/Cargo.toml` — `notify = "6"` added under the linux target block,
  new `[[bin]]` for `tracelean-sandbox`
- `tracelean/core/src/ai/shell_sandbox.rs` — visibility loosened (`pub(crate)`) on
  `PROTECTED`, `HOME_ALLOWLIST`, `MAX_CAPTURE_BYTES`, `is_protected`,
  `is_allowlisted`, `read_capture`
- `tracelean/core/src/state.rs` — `sandbox_link` field + accessors, near
  `project_root` (`:341`)
- `tracelean/core/src/service.rs` — mirror-on-save, in `save_file()` (`:123`)
- `tracelean/tests/integration_sandbox_session.rs` + `tests/Cargo.toml` registration
- Plan doc (phase-by-phase design + verification steps):
  `/home/ricostynha/.claude-personal/plans/you-task-is-to-sunny-summit.md`
