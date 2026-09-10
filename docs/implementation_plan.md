# Implementation plan — Bugs -1/0/1, OpenRouter, ACP/openCode, LSP

Companion to [`bug_investigation_research.md`](./bug_investigation_research.md), which is
the root-cause research this plan builds on. That doc says *what's broken and why*
(with file:line evidence); this doc says *what to build, in what order, and for whom*.
Everything here is still a plan — nothing implemented.

Each item carries:
- **User stories** — who wants this and what "done" looks like from their side (these
  were missing from the research doc; added here so acceptance is testable, not vibes).
- **Approach** — condensed from the research doc's root cause + fix direction.
- **Suggested implementation priority** — the concrete order of sub-steps *within* the
  item and why. A single cross-cutting **Master priority** ordering all items against
  each other is at the very end.

Story format: *As a `<role>`, I want `<capability>` so that `<benefit>`.* Roles used:
**Dev** (the human editing code in the IDE), **Agent-user** (the human driving the AI
chat), **Agent** (the model itself — a real stakeholder, since half these bugs are the
model being misled by bad signals).

---

## Bug 0 — three bundled symptoms (checkpoint / stale diff / retry loop)

### User stories
- As an **Agent-user**, I want "Save checkpoint" to persist every open dirty buffer to
  disk, so that when the agent then reads/edits those files via `run_shell` it sees the
  same content I see on screen.
- As an **Agent-user**, I want to actually be able to *accept* an agent's shell-driven
  edit, so that review mode isn't a dead end that only ever shows "this diff no longer
  matches the file."
- As an **Agent**, I want an unambiguous, information-rich result after a mutating
  `run_shell` (what changed, and whether it's applied or still staged), so that I don't
  re-issue the same command in a loop thinking it didn't take.
- As an **Agent-user**, I want a runaway identical-tool-call loop to self-arrest, so that
  a confused model doesn't burn my budget echoing the same command dozens of times.

### Approach (per symptom, from research doc)
- **A (checkpoint):** in `save_checkpoint`, before persisting, iterate open files and
  write each buffer's current content to the real path (reuse `save_file`'s write body,
  `service.rs:123-140`), then persist the checkpoint as today.
- **B (stale diff):** the same `run_shell` restages a diff against a fresh overlay's
  pre-image every call, so once one is accepted the others' baked-in originals are stale.
  Fix: either (a) refuse/queue a *new* pending diff for a path that already has an
  unresolved one, or (b) re-diff pending hunks against the current buffer at accept time
  and auto-rebase instead of hard-failing. Prefer (a) — it also structurally prevents the
  duplicate-staging that symptom C produces.
- **C (retry loop):** two parts —
  1. A same-turn duplicate-tool-call guard in `runtime.rs`: track recent `(name, args)`
     pairs; after N identical calls inject a system-authored note ("this exact call already
     ran; its result is above — do not repeat it") and skip re-execution.
  2. Replace the terse `"{path} (modified)"` note. Per the user's own edit to the research
     doc, the message should state the *outcome and magnitude*, e.g.
     `"{path} — staged for review (≈134 chars added, 20 removed); (note the agent will never be given the message waiting for user input, the part of the program responsible for giving the agent a new reponse will just block, and when user aproves rejects everything than the answer is geenrated for the agent) and, once resolved, report the accept/reject result with the same char-delta detail —
     reusing the `ResolvedDiffOutcome` plumbing already built for Bug 2 last session
     (`applied_message` / accepted-vs-total hunks) rather than inventing a new channel.

### Suggested implementation priority
1. **C.2 (the message) first.** It's the cheapest change and, per the research, the most
   *likely* actual trigger of the loop the user saw — the model misreading "(modified)" as
   "not confirmed." Fixing the signal may resolve the observed loop on its own.
2. **C.1 (the guard) second**, as the safety net for when a model loops for *other*
   reasons — belt-and-suspenders, but it needs the guard to be conservative (only fire on
   *identical* args, several times) so it never blocks legitimate repeated commands.
3. **B (stale diff) third.** Higher effort (touches the sandbox/overlay staging path) and
   option (a) partly depends on the same "one pending diff per path" bookkeeping the guard
   in C.1 makes natural. Doing C first shrinks how often B even triggers.
4. **A (checkpoint sync) last** of this group — it's independent and simple, but it's the
   least urgent of the four since Ctrl+S already gives the user a manual workaround today;
   it's a papercut, not a dead end like B.
   
   So impleemnt only C.2 and C.1

---

## Bug 1 — the `find` tool (embeddings + regex/glob confusion)

### User stories
- As an **Agent-user**, I want semantic/embedding search to work as soon as I open a
  project, so that the agent can find things by meaning without me hunting for a "rebuild
  index" button (which doesn't even exist anymore).
- As an **Agent**, I want a search tool simple enough that I don't confuse its parameters,
  so that I stop passing `*.py` into a regex field and getting a cryptic parse error.
- As an **Agent-user**, I don't want search indexing to silently spend my API budget, so
  that "it just works in the background" never means "it just charged me."

### Approach
- **A (embeddings never built):** the pipeline is fully implemented but never called —
  `execute_tool` hardcodes `&None` for the index. Building is **local and free** (fastembed
  `AllMiniLML6V2`, cached), so it's safe to auto-run. Hook `EmbeddingsIndex::build` into
  `service.rs::open_project` on a background task, store into a `SharedIndex` on
  `SharedApp`/`AiService` (mirror the `pending_diffs`/`resolved_diffs` pattern), thread it
  into `execute_tool_with_index`. Fix the stale "rebuild context button" error text
  regardless.
- **B (glob-in-regex):** minimal fix is a glob-shape detection hint in `regex_find`'s
  error branch. But the user's stronger hypothesis — *the tool is too complex to call
  correctly* — is well-founded and gets its own section below.

### `find` is too complex — redesign alternatives
`find` currently exposes **9 parameters** (`query`, `mode`∈{auto,regex,semantic},
`case_sensitive`, `max_results`, `offset`, `path`, `file_filter`, `max_depth`, `context`).
That's a lot of surface for a model to hold correctly, and two of them overlap in ways
that invite exactly the observed mistake: `mode`/`case_sensitive` both bear on match
semantics, and `query` (content pattern) vs `file_filter` (filename glob) are easy to
swap. Three ways forward, recommended first:

- **Alternative 1 (recommended) — split into tools that match model training priors.**
  Replace the one mega-`find` with the shapes frontier coding models already know from
  their training (ripgrep/fd, Claude Code's own `Grep`/`Glob`):
  - `grep` — content search: `query`, optional `path`, optional `file_filter`, optional
    `context`. Regex with a case-insensitive fallback, no `mode` param.
  - `glob` — filename search: `pattern`, optional `path`. This is where `*.py` was always
    meant to go; a glob in `grep`'s `query` can now be *detected* and redirected with a
    one-line hint ("did you mean the `glob` tool?").
  - `search_semantic` — meaning-based: `query`, optional `path`. Backed by the now-auto-built
    embeddings index.
  Each tool is 2–4 params instead of 9, and the *tool name itself* disambiguates intent,
  which is exactly what a confused model needs. Cost: three schema entries + dispatch
  instead of one, and updating any prompt/docs that name `find`. This is the option most
  likely to actually stop the mix-ups, because it removes the ambiguous choice rather than
  documenting around it.
- **Alternative 2 — keep one `find`, auto-detect the mode.** Drop `mode` and
  `case_sensitive`; infer regex-vs-glob-vs-semantic from the pattern shape and result
  count (bare words → literal/case-insensitive; `*.ext`/`**` → glob on filenames;
  natural-language phrase or zero regex hits → semantic). Fewer params, but it *blurs*
  `query` vs `file_filter` semantics and makes behavior implicit/surprising — the research
  doc flags this as risky, and I agree it's second choice.
- **Alternative 3 — keep the schema, fix only the error/description.** The minimal patch:
  better `mode` descriptions + the glob-detection hint. Cheapest, lowest-risk, but it only
  *documents* the trap instead of removing it — the 9-param surface still invites other
  slips. Fine as a stopgap shipped *today* while Alternative 1 is built.

### Suggested implementation priority
1. **Alternative 3's error-message hint first** (an hour of work; stops the specific
   cryptic-error footgun immediately, independent of everything else).
2. **Embeddings auto-build (A) second** — it's self-contained, free, and unblocks
   semantic search being useful at all; it also has to land *before* a `search_semantic`
   tool would have anything to query.
3. **Alternative 1 (the tool split) third**, as the real fix — but only after (2), since
   `search_semantic` depends on the index existing, and after confirming with the user
   that renaming/splitting `find` is acceptable (it's a visible tool-surface change).

Impelment 2 and 3 (better to split find I belive it is complex, also Remove pagination but when output of the tool is too big the backend simply writes the output to a file, and then says to the model it should read it. Doing that only the read must be pagination protected that it is !!!) (so remove pagination from all commands except read, that is a essensial thing as well). (as it simplify the commands and the model only must now how to use the read well)
---

## 6.5 — OpenRouter live testing

### User stories
- As a **Dev**, I want a real (non-mock) provider test I can run for free, so that I can
  validate the OpenRouter path end-to-end without spending money like the Bedrock tests do.
- As a **Dev**, I want that live test off by default, so that `cargo test --workspace` and
  CI stay hermetic and offline.

### Approach
Provider code already exists (`openrouter.rs`, `model_catalog.rs`, `streaming.rs`) — this
is a *test* gap. Gate a live test behind a `live_openrouter` Cargo feature, mirroring the
existing `live_bedrock` convention in `tests/Cargo.toml`. Free tier (July 2026): 27+
`:free` models, **20 req/min** cap, 50/day (or 1,000/day after a one-time $10 credit). Pin
whichever `:free` id is live when the test is written; the roster rotates.

### Suggested implementation priority
1. One gated smoke test (connect, single non-streaming turn, assert a response) first —
   proves the path works at all.

2. Another test (2 prompts only): Only need the smoketest I believe and maybe one easy prompt like: "what is 1+1, answer only with the answer, I want only to see 2, and nothing more on the answer", and then check if I see 2. Just to see if frameork is working.  And then do another quesiton asling 1+2,, but checking if logs caching etc make sense.

---

## 6.8 — ACP / openCode: make ACP the single agent loop

**Where we are today (answering the original question): no, the AI chat agent is not
wired through ACP — it's a separate, native loop, and the ACP module is a second,
parallel implementation.** Traced in code:

- The chat panel's two paths both call the **native runtime**:
  `ai_chat_session` → `AiService::chat_turn` → `run_agent_turn_session`
  (`core/src/agent/runtime.rs`), and `ai_chat_stream` → `run_agent_turn_session` directly
  (`mcp_commands.rs:259`). Grepping `core/src/agent/` for `acp`/`Acp` returns **nothing** —
  the chat loop has zero ACP involvement.
- The ACP module (`core/src/acp/`) is used by only two things, *neither of which is the
  chat panel*: the standalone `tracelean-agent` binary (exposes an agent over ACP stdio
  for external editors), and `AcpClientManager` (lets TraceLean connect *out* to external
  agents — Tauri commands exist, but no frontend calls them).
- Critically, `core/src/acp/builtin_agent.rs` is a **separate reimplementation** of the
  turn loop — it does *not* call `run_agent_turn_session`; it has its own compaction
  constants and its own message handling. So today there are **two independent agent-loop
  implementations** that can drift apart.

### Decision (made): do both (i) and (ii), with (ii) = option (b)

- **(i) Connect *to* openCode (TraceLean as ACP client).** Fully unblocked by the existing
  backend — openCode already speaks ACP (`zed.dev/acp/agent/opencode`), and
  `acp_connect_agent` already accepts `{name, command, args}`. This is a frontend
  `AcpPanel.tsx` + a manual interop smoke test against a real `opencode` binary. No new
  protocol code.
- **(ii) ACP becomes the *only* agent loop.** We collapse to a single loop and standardize
  on ACP as the internal communication protocol even for our own agent, so there is exactly
  one loop to maintain and no drift.

### "Will that be possible without losing features?" — the key feasibility question

Yes — **but only if (ii) is done in the correct direction.** The subtlety: option (b) must
mean *"keep `runtime.rs` as the one real loop and put an ACP adapter on top of it, then
delete `builtin_agent.rs`'s parallel loop"* — **not** *"port the chat over onto
`builtin_agent.rs`'s loop."* Direction matters enormously, because the two loops are not
peers — `builtin_agent.rs` is dramatically less capable, verified in code:

| Capability | `runtime.rs` (chat) | `builtin_agent.rs` (ACP) |
|---|---|---|
| Tool set | full `data/tools.json` via `tool_executor.rs` | a hand-written **5-tool** subset (`read_file`/`write_file`/`list_directory`/`search`/`run_command`) with *different names* |
| Tool-call parsing | real provider `tool_use` blocks | text scraping (`parse_llm_response`, with a `TODO: replace with proper structured output`) |
| Review mode / diff staging | full `ReviewSink` + `wait_for_review` + `ResolvedDiffOutcome` | none (`review_edits: false` hardcoded) |
| Command approval / permissions | real `PauseHandler::approve_command` | stubbed ("for now, we log that permission would be requested") |
| Cost-aware compaction | `cost_aware_compact` + retention engine + cache-breakpoint planning | a simplified duplicate (`compact_context`, self-described as simpler than the GUI path) |
| Live context bar, spend-cap enforcement, cancellation race | all present | partial / absent |

So porting *onto* the ACP loop would lose almost everything built over the last sessions.
Porting the ACP loop *onto* `runtime.rs` loses **nothing**, because the features live in the
*loop*, not the transport. That reframes the concern: **ACP is a transport/protocol, not an
agent loop.** The question "does ACP support review mode / compaction / the context bar?"
mostly dissolves:

- **Review-mode block, command approval → ACP `session/request_permission`.** ACP has a
  first-class permission-request round-trip; our `PauseHandler::approve_command` and
  `wait_for_review` map onto it directly (an approval request the client answers). This is
  the same shape `acp_permission_respond` already stubs.
- **Streaming output, tool-call visibility → ACP `session/update` notifications**
  (`agent_message_chunk`, `tool_call`, `tool_call_update`) — already emitted by
  `builtin_agent.rs`, so the surface exists.
- **Cancellation → ACP `session/cancel`**, which maps onto the existing `CancelToken`.
- **The genuinely IDE-internal signals — the live context-usage bar, cost/cache telemetry —
  have no ACP equivalent, and that's fine:** they're editor-internal Tauri events, not
  agent-protocol concerns, and stay as the side-channel events they already are regardless
  of transport. They don't need to travel over ACP to keep working.

**Net:** feature parity is preserved *by construction* if we keep `run_agent_turn_session`
as the single loop and treat ACP as an adapter/transport layer wrapping it. The real cost
of (ii) is engineering effort (writing that adapter so `runtime.rs`'s `PauseHandler`,
tool-executor, and event sink drive ACP's permission/update/cancel messages) plus deleting
the `builtin_agent.rs` loop — not lost capability. The one thing to prove out during
implementation: that ACP's permission-request round-trip is expressive enough for the
*per-hunk diff review* flow (accept some hunks, reject others), which is richer than a plain
yes/no approve. If it isn't, that specific flow may stay a side-channel Tauri event too —
worth confirming against the `agent-client-protocol` v1.2 permission schema early.

### User stories
- As an **Agent-user**, I want to drive an external agent like openCode from inside
  TraceLean, so that I have a fallback if the built-in agent isn't good enough yet.
- As a **Dev**, I want exactly one agent turn loop, so that the chat path and the ACP path
  can never diverge in behavior or bugs and I only maintain one.
- As an **Agent-user**, I want the switch to ACP-as-transport to be invisible, so that
  review mode, approvals, streaming, and the context bar all keep working exactly as today.

### Suggested implementation priority
1. **Ship (i) — the `AcpPanel.tsx` + openCode smoke test — first.** High user value (an
   escape hatch to a strong external agent), fully unblocked, low risk since it only *adds*
   a UI over working commands. It also exercises the ACP *client* path end-to-end against a
   real external agent, which de-risks (ii) by proving the transport interops before we
   depend on it internally.
2. **Then (ii), in the parity-preserving direction:** build an ACP adapter over
   `run_agent_turn_session` (map `PauseHandler`/tool-executor/event-sink onto ACP
   permission/update/cancel messages), switch the `tracelean-agent` binary to run *that*
   instead of its own loop, verify feature parity (especially the per-hunk review flow),
   then **delete `builtin_agent.rs`'s parallel loop.** Do not start the deletion until the
   adapter demonstrably matches today's chat behavior on review mode + compaction.
3. **Optionally, last:** route the internal chat panel through the same ACP adapter too, so
   even the in-app agent speaks ACP internally — the full "ACP is the only loop" end state.
   Gate this behind (2) being proven, since it's the change with the most user-visible blast
   radius if the adapter has gaps.

---

## 7 — LSP integration, and Myth × LSP synergies

### User stories
- As a **Dev**, I want real diagnostics, go-to-definition, hover, and symbol search on my
  code, so that TraceLean is a real IDE and not just an editor with a tree.
- As a **Dev**, I want LSP-driven actions (rename, code actions) to be undoable exactly
  like every other edit, so that the undo tree stays the single history of everything.
- As a **Dev**, I want to *discover* what's available at the cursor via the keyboard, so
  that IDE power isn't hidden behind menus I have to hunt through with a mouse.

### Crate choice (corrected from the existing plan)
`tower-lsp` is for building language *servers*; TraceLean needs to be the **client**.
Use **`async-lsp`** — it supports the client direction, composes via `tower::Layer`, and
handles notifications *synchronously in order* (tower-lsp dispatches them async, which its
own docs call "semantically incorrect" — a real problem for tracking live diagnostics). No
LSP crate is in `Cargo.toml` yet; this is greenfield.

### Myth × LSP synergies (the part worth being concrete about)
Myth's model is **content → parser → nodes with captures → a binding map attaching named
actions to captures**, rendered by the frontend with which-key discoverability
(`core/src/myth/surface.rs`, `actions.rs`, `keymap.rs`). LSP is, structurally, *another
producer of captured nodes and location-scoped actions* — so the fit is real, not
hand-wavy:

1. **Code actions ↔ Myth actions — the strongest synergy.** LSP `textDocument/codeAction`
   returns "named operations valid at this location." That is *exactly* Myth's
   capture→actions→which-key shape. An LSP code-action list can populate a Myth binding
   map at the cursor's capture, giving keyboard-discoverable "quick fixes here" for free —
   arguably a *better* use of the ActionRegistry than the file tree's fixed
   open/rename/delete set it powers today.
- 2. **Diagnostics ↔ SurfaceNode captures.** `SurfaceNode` already carries capture + range +
     JSON meta and its doc comment already anticipates "highlight captures for code." A
     diagnostic is just a node with an `error`/`warning` capture and a message in its meta —
     it renders through the *same* surface pipeline as syntax highlighting, no parallel UI.
3. **Symbol search / outline / references ↔ FileTreeSurface's list-nav.** These are the
   same list-of-nodes-you-navigate-and-act-on pattern `FileTreeSurface` already implements;
   swap paths for symbols and the navigation + which-key bindings carry over.
4. **Rename / code-action edits ↔ Command/undo-tree.** LSP `WorkspaceEdit` results should
   be lowered into the existing invertible `Command`s and pushed through the undo tree — so
   an LSP rename is undoable identically to a hand edit. This is general architecture reuse
   (available to any feature), not Myth-specific, but it's what keeps "one history of
   everything" true.

**The one place the synergy does *not* hold (scope this separately):** `FileTreeSurface`
re-parses static content once per view build. Live diagnostics must update on *every
keystroke* in an editing buffer — that's incremental re-parse, a capability the surface
model doesn't have yet. Treat live squiggles as a distinct, harder sub-problem gated on
extending `Surface`; don't let it block go-to-def / symbol search / code actions, which
need no new capability. (Note the overlap with Bug -1's layer 3: both want incremental
tree-sitter re-parse — doing Bug -1 layer 3 first would de-risk this.)

### Suggested implementation priority
1. **Transport + a single server (rust-analyzer) first** — spawn, initialize, capability
   negotiation. Pure infrastructure, shares subprocess/JSON-RPC patterns with the ACP
   client. Nothing user-visible yet.
2. **Go-to-definition + hover + symbol search next** — the request/response features that
   need *no* new surface capability, so they land on the existing Myth pipeline directly
   and prove the integration.
3. **Code actions third** — the highest-synergy feature, wiring LSP actions into the Myth
   ActionRegistry + which-key.
4. **Live diagnostics last**, gated on incremental re-parse (and ideally after Bug -1's
   layer 3 has already introduced incremental tree-sitter). Don't block 1–3 on it.

---

## Master implementation priority (all items, in order, with why)

Ordered by *(user pain now) × (how unblocked it is) ÷ (risk/effort)* — ship correctness
and cheap wins before large greenfield subsystems.

1. **Bug -1 layer 1 (cursor teleport correctness).** A data-loss-adjacent bug hitting the
   Dev on every fast-typed large file, right now. Correctness, self-contained, testable.
   Nothing else matters if the editor itself scatters your keystrokes.
2. **Bug 0-C.2 + C.1 (shell result message + duplicate-call guard).** Cheap, and directly
   stops the budget-burning agent loop the user actually hit. C.2 is nearly free and may
   fix it alone.
3. **Bug 1-B minimal hint + Bug 1-A embeddings auto-build.** Both small/self-contained;
   the hint kills a daily footgun today, the auto-build (free, local) makes semantic
   search real and unblocks the later tool split.
4. **Bug 0-B (stale diff) + Bug 0-A (checkpoint sync).** Higher-effort review-mode
   correctness; B is partly de-risked by having done the C guard first. A is a simple
   papercut with a Ctrl+S workaround, so it rides along last of this group.
5. **`find` tool split (Bug 1 Alternative 1).** The real fix for tool complexity, but it's
   a visible surface change needing user sign-off and depends on embeddings (step 3) being
   in place first.
6. **OpenRouter gated live test (6.5).** Small, unblocks cheap end-to-end provider
   validation that every later agent change benefits from. Slots in whenever there's a gap.
7. **ACP `AcpPanel.tsx` + openCode smoke test (6.8-i).** High value, fully unblocked, low
   risk — pure UI over working backend. Then make the two-loops decision (6.8-ii) with the
   user before any refactor.
8. **Bug -1 layers 2–3 (latency + incremental markdown parse).** Perf polish; do once the
   correctness fix (step 1) is proven, and pair layer 3 with the LSP work that also wants
   incremental re-parse.
9. **LSP (item 7).** Last — the largest greenfield subsystem, best built once Myth's
   surface/action model is exercised and incremental re-parse (step 8) exists to lean on.

Rationale for the shape: steps 1–4 are correctness and cheap-signal fixes to things
users hit *today*; 5–7 are self-contained features with clear value and low risk; 8–9 are
the big/uncertain investments that benefit from everything above being stable first —
mirroring how the previous batch (Bugs 2–4 last session) sequenced correctness before
new subsystems.
