# TraceLean — Implementation Spec for todo.md (P1–P9) + Roadmap

Date: 2026-07-17
Status: PROPOSED (awaiting review)

This spec is grounded in a code investigation. Where a todo item described a symptom,
the actual root cause was located and is cited with `file:line`.

---

## P1 — Tool calls render as blank messages in the AI chat

### Symptom
During an agent turn, tool-call activity shows up as empty bubbles in the chat.
Expected: compact chips like `read_file ×3`.

### Root cause (found)
`TauriEventSink::emit` (`tracelean/gui_backend/src/lib.rs:74-79`) forwards the payload
**as a `&str`**:

```rust
fn emit(&self, event: &str, payload: &str) {
    let _ = self.app_handle.emit(event, payload);   // payload serialized as a JSON *string*
}
```

The agent runtime builds a JSON object and stringifies it
(`tracelean/core/src/agent/runtime.rs:306` — `emit("ai-chat-message", &payload.to_string())`).
Tauri then serializes that string *again*, so the frontend receives
`event.payload === "{\"role\":\"system\",...}"` — a string. `AiChatPanel.tsx:179-186`
does `const { role, content } = event.payload;` → both `undefined` → blank bubble.

Secondary issue: even when parsed, the chat panel reconstructs tool names by
regex-matching free text (`AiChatPanel.tsx:391-399` matches `"call X"` / `"rsp X"`),
while a **structured** `tool-call` event (`ToolCallEvent { tool_name, status, duration_ms, … }`,
emitted at `runtime.rs:435-445`) already exists and is simply never consumed by the panel.

### Design decisions
- **D1.1 — Fix the boundary once, not every consumer.** `TauriEventSink::emit` parses the
  payload string into `serde_json::Value` and emits the *object* (falls back to raw string if
  not JSON). Frontend listeners also get a small `normalizePayload()` helper (parse if string)
  for defense in depth. Rejected alternative: `JSON.parse` in each listener — every future
  listener would have to remember it.
- **D1.2 — Chat consumes structured `tool-call` events, not prose.** `AiChatPanel` subscribes to
  `tool-call` and maintains a list of tool-call records per turn keyed by
  `(tool_name, sequence)`; `Running → Completed/Failed` transitions update in place
  (spinner → ✓/✗). The `"call X"` / `"rsp X"` system messages emitted at
  `runtime.rs:430-434` and `486-490` are **removed** (they exist only to feed the old
  string-parsing UI).
- **D1.3 — Grouping/rendering.** Consecutive tool calls between two assistant text messages
  render as one batch row: `read_file ×3 · find ×1 (1.2s)`. Each chip expands (click) to show
  truncated args and result preview. To support this, `ToolCallEvent` gains
  `args_preview: Option<String>` and `result_preview: Option<String>` (both capped at 200 chars,
  built in `runtime.rs` where args/result are in scope).
- **D1.4 — Failed calls are visible.** `Failed { error }` renders the chip in red with the error
  in the expansion — this is where the P3 message (below) surfaces to the user.

### Acceptance
- A Bedrock/MiniMax agent turn with 3 `read_file` calls shows `read_file ×3`, no blank bubbles.
- Streaming and non-streaming paths both render tool chips.
- A malformed tool call renders a red chip with the P3-formatted error.

### Files touched
`gui_backend/src/lib.rs`, `core/src/agent/runtime.rs`, `react_frontend/components/AiChatPanel.tsx`.

---

## P2 — MiniMax native tool-calling format (kill the JSON-escaping failure class)

### Problem
Tools are embedded in the system prompt (Bedrock/Mantle won't cache the `tools` field for
MiniMax), and the model is asked to reply in Hermes format: JSON inside `<tool_call>` tags
(`bedrock.rs:61-84`). For large string arguments (e.g. `replace_str.old_str` containing quotes,
newlines, braces), the model reliably produces broken JSON → `invalid JSON: EOF while parsing`.
This is a *format-choice* problem, not a parser bug: MiniMax-M2.5 was trained on its own XML
invoke format, where parameter values are raw text needing **no escaping**.

### Design decisions
- **D2.1 — Speak the model's trained dialect.** For MiniMax models, the system prompt presents
  tools exactly as the MiniMax chat template does, and asks for `<minimax:tool_call>` output:

  ```
  # Tools
  You may call one or more tools to assist with the user query.
  Here are the tools available in JSONSchema format:

  <tools>
  <tool>{"name": "...", "description": "...", "parameters": {...}}</tool>
  ...
  </tools>

  When making tool calls, use XML format to invoke tools and pass parameters:

  <minimax:tool_call>
  <invoke name="tool-name">
  <parameter name="param-key">param-value</parameter>
  ...
  </invoke>
  </minimax:tool_call>
  ```

  `render_tools_as_text` (`bedrock.rs:61`) is rewritten to this template (full JSON schema per
  tool inside `<tool>`, replacing today's `- **name**: desc | Parameters: …` bullet list).
- **D2.2 — Per-model-family format selection.** New enum in the model catalog:

  ```rust
  pub enum ToolCallFormat { MiniMaxXml, HermesJson, MistralBrackets }
  ```

  `model_catalog::enrich` assigns it from the model id (`minimax*` → `MiniMaxXml`; default
  `HermesJson`). `BedrockProvider` renders the tool block *and* parses the response according
  to the format. The existing three-strategy cascade in `parse_tool_blocks` (`bedrock.rs:98`)
  stays as fallback, but the primary strategy is the model's declared format.
- **D2.3 — Schema-driven type coercion.** The current XML parser guesses parameter types
  (`bedrock.rs:262-276`: `"true"` → bool even if the schema says string). The parser gets access
  to the request's `ToolSchema`s and coerces each `<parameter>` value using the declared
  JSON-schema type; only unknown parameters fall back to the guessing heuristic.
- **D2.4 — History round-trip in the same dialect.** `build_messages` (`bedrock.rs:727-744`)
  currently re-renders past assistant tool calls as Hermes `<tool_call>` JSON. For MiniMax it
  must re-render them as `<minimax:tool_call>` XML — otherwise the model sees a dialect in
  history different from the one it's told to emit, and the cached prefix churns. Tool results
  keep the `[tool_result id=…]` user-message flattening.
- **D2.5 — Multiline/edge parsing.** Parameter values capture everything (including newlines,
  quotes, braces) up to the next `</parameter>`. Documented limitation: a value containing the
  literal string `</parameter>` cannot be represented (same limitation as vLLM's reference
  parser). Add regression tests: multi-line `old_str` with quotes/braces (the exact failing
  payload from todo.md), CJK/emoji values, nested JSON-looking values, two invokes in one block.
- **D2.6 — Thinking tags.** Strip `<think>…</think>` spans from content before tool parsing and
  before display (MiniMax emits them in this dialect).

### Acceptance
- The exact `replace_str` call from todo.md (2 279-char `old_str` with code) round-trips:
  rendered prompt → simulated model XML reply → parsed call → executed.
- Cache hit rate does not regress (tool block content is byte-stable across turns).
- Non-MiniMax Bedrock models keep working via `HermesJson`.

### Files touched
`core/src/ai/bedrock.rs`, `core/src/ai/model_catalog.rs`, `core/src/ai/provider.rs` (format field).

---

## P3 — Actionable tool-call error messages

### Problem
On a malformed call the model gets a generic `Expected format: <tool_call>{"name": …}` hint
(`bedrock.rs:569-570`) with no tool-specific guidance, so it often repeats the same mistake.

### Design decisions
- **D3.1 — One canonical example per tool, stored in `data/tools.json`.** `ToolJsonEntry`
  (`tool_registry.rs:35`) gains an optional `example` field: a complete, valid arguments object
  as JSON (e.g. `{"path":"src/main.rs","old_str":"foo","new_str":"bar"}`). The registry exposes
  `example_for(name) -> Option<&str>`.
- **D3.2 — Single error-message builder, used everywhere.** New function in `core/src/ai`:

  ```rust
  fn format_tool_call_error(tool_name: Option<&str>, raw: &str, parse_err: &str,
                            registry: &ToolRegistry, format: ToolCallFormat) -> String
  ```

  Output includes: (1) which tool failed (we almost always know the name — extract it even from
  broken JSON with a lenient `"name"\s*:\s*"(\w+)"` scan); (2) the parse error; (3) the exact
  invocation example rendered *in the active dialect* (MiniMax XML example for MiniMax, Hermes
  JSON for others). Call sites: `bedrock.rs` parse-error feedback (`:564-572`) and the agent
  runtime invalid-arguments paths (`runtime.rs:315-362` and `:388-427`).
- **D3.3 — Pre-execution schema validation.** Before executing, arguments are validated against
  the tool's JSON schema (required fields present, no unknown fields, primitive types match —
  a small hand-rolled checker, no new dependency). Validation failure produces the same
  formatted message instead of a deep executor error.

### Acceptance
- Feeding the broken payload from todo.md produces a message naming `replace_str` and showing
  its example invocation in the active dialect.
- All static + dynamic tools in `tools.json` have an `example`; a unit test asserts each
  example validates against its own schema.

---

## P34 — Editing/undo reliability: the `Replace` primitive

### Root causes (found — three independent bugs)
1. **Frontend sends a command variant that does not exist.** `Editor.tsx:399-407` sends
   `{ Replace: { file, offset, old_text, new_text } }` for any select-and-type / paste-over /
   autocomplete edit. `Command` (`core/src/commands.rs:26`) has **no `Replace` variant**, so
   serde deserialization fails, the Tauri call errors, and the edit is *silently dropped*
   (only `console.error`). Frontend and backend buffers now diverge; every subsequent
   offset-based command lands in the wrong place. This alone explains "backspace deletes the
   first character" and the general editing nightmare.
2. **Offset unit mismatch.** CodeMirror offsets are UTF-16 code units; the backend interprets
   `offset` as **bytes** (`state.rs:69-101` with char-boundary clamping). Any non-ASCII
   character shifts all later edits.
3. **No divergence detection.** `Delete` trusts `offset/len` blindly; corruption is silent and
   compounding. Cursor position is never recorded, so undo/redo cannot restore it.

### Design decisions
- **D34.1 — Single text primitive** (adopting the todo proposal, with one refinement):

  ```rust
  Replace {
      file: PathBuf,
      at: usize,        // Unicode-scalar (char) index — NOT bytes, NOT UTF-16 units
      old: String,      // text currently at `at` (may be "")
      new: String,      // replacement (may be "")
  }
  ```

  - `insert` = `Replace { at, old: "", new: text }`; `delete` = `Replace { at, old: text, new: "" }`;
    backspace at 0 = no-op (never emitted).
  - **Inverse is trivial and total:** swap `old`/`new`. The undo tree stores only `Replace`
    (plus file ops and `Batch`).
  - **Refinement vs the todo:** no `find(old)` search. The todo's "locate first occurrence of
    `old` after start" makes execution context-dependent (the same command means different
    things on different buffer states), which breaks replay/jump determinism. Instead `at` is
    exact, and `old` becomes a **verification witness**: `execute` asserts the buffer content at
    `at..at+old.chars().count()` equals `old`. On mismatch it refuses the edit, emits a
    `state-integrity-error` event, and the frontend resyncs — divergence is *detected at the
    first bad command* instead of corrupting the file.
  - Byte conversion is internal (`char_indices().nth()` as in the todo's
    `byte_index_at_symbol`), so all public positions are char indices.
- **D34.2 — Frontend offset conversion.** `Editor.tsx` converts CodeMirror UTF-16 offsets to
  char indices: `charIdx = countCodePoints(doc.sliceString(0, fromA))` (surrogate-pair aware,
  O(edit prefix); fine at editor scale — optimize later only if profiling demands). It sends
  only `Replace` commands; the `Insert`/`Delete`/legacy-`Replace` payload shapes are deleted
  from the frontend.
- **D34.3 — Migration.** `Command::{Insert, Delete}` are removed from the enum. The persisted
  command log gets a version field; logs without it (or with old variants / byte offsets) are
  **discarded on load** with a console warning and a fresh initial node. Rationale: replaying
  historic byte-offset logs as char-offset commands would itself corrupt; TraceLean is
  pre-release and history is not user data worth a converter. (`persistence.rs`,
  `surgical_edit/*` which construct Insert/Delete, `tool_executor.rs` edit paths, and
  `UndoTreePanel` markers all migrate to `Replace`.)
- **D34.4 — Cursor without cursor commands.** `SetCursor`/`SetSelection` are removed from the
  undo tree (they pollute history and are the todo's suspected clash). Cursor after
  undo/redo/jump is *derived*: place at `at + new_char_len` of the last applied sub-command.
  Backend `undo`/`redo`/`jump_to_node` IPC responses gain `{ file, char_pos }`; the editor
  moves the CM selection there and scrolls it into view.
- **D34.5 — Continuous divergence detection.** `apply_command` response includes
  `{ revision: u64, content_hash: u64 }` (FxHash of the buffer). The editor compares against a
  locally computed hash; mismatch → full `syncFromBackend()`. This turns any residual bug from
  silent corruption into a self-healing blip.
- **D34.6 — Typing-run compaction (phase 2, optional).** Consecutive `Replace` nodes with
  adjacent positions, same file, within 750 ms merge into one undo node
  (`Replace(1,"","A") + Replace(2,"","B") ⇒ Replace(1,"","AB")`, per the todo). Implemented as
  a coalescing rule in `UndoTree::push`; never merges across a commit point or branch.
- **D34.7 — File ops stay as-is for now.** `CreateFile`/`DeleteFile`/`RenameFile` are already
  invertible and are not implicated in the bug reports. The todo's `replace_file(from?, to?)`
  unification is deferred (noted as P34-ext) — collapsing them buys elegance but requires
  carrying overwritten-content snapshots for invertibility; not worth blocking the editing fix.

### Tests
- Property test (proptest is already a dev-dep): random edit scripts over random Unicode
  documents (ASCII, CJK, emoji, combining marks) — apply all, undo all ⇒ original;
  redo all ⇒ final; jump to random nodes and back ⇒ consistent.
- Regression: select-and-type over a multibyte char; backspace runs at document start/middle/end;
  the "backspace deletes first char" scenario.
- Witness-mismatch test: corrupt the buffer manually, apply `Replace`, assert integrity error
  (not a wrong edit).

### Acceptance
- Typing, deleting, selecting-and-replacing, undoing and redoing across branches works with a
  file containing emoji/CJK, with no divergence over a 1 000-op fuzz session.

---

## P5 — Undo-tree hover diff in editor (and file tree)

### Current state & diagnosis
The plumbing exists (`UndoTreePanel.handleHover` → `get_undo_node_diff` →
window event `undo-hover-diff` → `Editor.parseDiffToDecorations`) but the last stage
hand-parses unified-diff text with buggy line accounting (`Editor.tsx:128-187`: added lines
don't advance counters; removed-line positions drift after the first hunk; file-path matching
is heuristic `endsWith`). It fails open — no decorations. Also the diff text comes from a
custom LCS differ (`state.rs:332`) whose output the regex parser then re-parses: two lossy
steps where one structured step would do.

### Design decisions
- **D5.1 — Ship structure, not text.** New IPC `get_undo_node_diff_structured(nodeId)`:

  ```rust
  struct NodeDiff { files: Vec<FileDiff> }
  struct FileDiff { path: String, added: u32, removed: u32, hunks: Vec<Hunk> }
  struct Hunk { current_start_line: u32, removed_lines: Vec<String>, added_lines: Vec<String> }
  ```

  Produced directly from the existing LCS op-stream in `simple_unified_diff` (refactor it to
  return ops; keep the text renderer for the log view). The regex parser in `Editor.tsx` is
  deleted.
- **D5.2 — Editor rendering.** Hovering a node decorates the current doc: lines that would be
  removed get the red line style; insertion points get a green widget line showing the incoming
  text (CodeMirror block widget), so "what changes if I jump here" is fully visible without
  leaving the current state. (Full side-by-side via `@codemirror/merge` is deferred — widget
  overlay answers the hover use-case with far less machinery.)
- **D5.3 — File tree diff badges (new, requested).** On hover, `FileTree` receives the same
  `NodeDiff` (lifted via shared state in `App`, same `undo-hover-diff` custom event) and shows
  per-file badges `+a −r` with a colored dot on affected files; files not in the tree view
  (e.g. collapsed dirs) surface via a count on their ancestor directory. Clearing hover clears
  badges.
- **D5.4 — Debounce.** Hover diffs are computed at most every 120 ms and cancelled on
  mouse-out, so sweeping the tree doesn't hammer IPC (each call clones AppState today —
  acceptable with debounce; revisit if profiling says otherwise).

### Acceptance
- Hovering any node with the corresponding file open shows red/green line decorations at the
  correct lines (verified against a scripted 3-edit history).
- Hovering shows `+/-` badges in the file tree for every touched file; unhover clears both.

---

## P6 — Enforcing the requirement → spec → code/test chain in the UI

### Current state
Linkage is filename-convention-only (`reqs/X.md ↔ specs/X.lean`, integration tests by filename —
`trace_graph/scan.rs:130-186`). Nothing links *code* to requirements, and nothing reports gaps.

### Design decisions
- **D6.1 — Annotation convention.** A trace annotation is a comment whose first non-space
  character after the comment leader is `*`:

  ```
  //* Req: REQ-001            (Rust/TS/C…)      #* Req: REQ-001   (Python/shell)
  --* Req: REQ-001, REQ-007   (Lean — multiple IDs allowed)
  //* Spec: sorting           (link code directly to a spec file stem)
  ```

  Grammar: `<leader>* (Req|Spec): <ID> ("," <ID>)*`. The annotation binds to the **next symbol**
  whose `start_line ≥` the annotation's line (function/struct/class per the existing
  tree-sitter `SymbolTable`); a file-level annotation (before any symbol, or in a file with no
  symbols) binds to the file. Test functions in `tests/` bind the same way.
- **D6.2 — Scanner.** `trace_graph/scan.rs` gains an annotation pass (plain line scan — no new
  tree-sitter queries needed) producing edges `Code —implements→ Req`, `Test —verifies→ Req`,
  `Spec —specifies→ Req` (annotation-based spec links complement the filename rule; filename
  rule stays as fallback). Incremental update path (`service.rs:154`) re-runs the pass per file.
- **D6.3 — Coverage model & report.** New `trace_graph/coverage.rs`:
  - A Req is **covered** iff it has ≥1 spec, ≥1 code, and ≥1 test edge (each dimension reported
    separately).
  - **Disconnected sets**: reqs missing any dimension; code symbols with no Req (only symbols in
    configurable "traced roots", default `src/`, to avoid flagging generated/util code);
    orphan specs/tests (link to nonexistent Req IDs — these are *errors*, not warnings).
  - IPC `get_trace_coverage()` returns the structured report.
- **D6.4 — UI surfacing.**
  - `TraceabilityDashboard` gets a Coverage section: per-Req row with ✓/✗ per dimension
    (spec/code/test), overall %, and clickable disconnected lists that navigate to the file.
  - Editor: gutter dot on annotated symbols; hover shows the linked Req title(s) (extends the
    existing symbol hover tooltip); unknown Req ID renders the annotation with a warning
    underline.
- **D6.5 — Enforcement level.** Warning-only in the UI. A `tracelean check --trace` CLI mode
  (nonzero exit on orphan-spec/test errors or coverage below a threshold) is specced for CI but
  gated behind the CLI work (see roadmap P13).

### Acceptance
- In `example/`, annotating one function with `//* Req: REQ-001` shows Code ✓ for REQ-001 in the
  dashboard and a gutter dot; removing it flips coverage and lists the Req as code-uncovered.
- An annotation naming a nonexistent Req appears in the errors list and navigates to the line.

---

## P7 — Context utilization display + reset button

### Current state & diagnosis
The cost bar shows `total_input_tokens / selected_model.max_tokens` (`AiChatPanel.tsx:474-478`)
— wrong on both axes: `total_input_tokens` is the *cumulative session sum* across all requests,
and `max_tokens` is the **output** cap (4096), not the context window. A
`reset_chat_session` IPC already exists (`ai_commands.rs:310`) but no UI calls it.

### Design decisions
- **D7.1 — Model knows its window.** `ModelConfig` gains `context_window: u32`;
  `model_catalog::enrich` + `data/models.json` supply real values (MiniMax-M2.5 ≈ 192k, Nova
  etc. per catalog; fallback 128 000 with a `~` marker in the UI when defaulted).
- **D7.2 — Track actual context, per session.** `ChatSession` gains
  `last_prompt_tokens: u32, last_completion_tokens: u32`, updated from each response's usage.
  Estimated next-turn context = `last_prompt_tokens + last_completion_tokens` (exact for the
  prefix; new user input adds on top). New IPC `get_chat_session_info(sessionId)`:

  ```rust
  { estimated_context_tokens, context_window, turns, compaction_count, model_view_len }
  ```

  Also included in the `ai-stats-updated` event payload so the bar updates live mid-turn.
- **D7.3 — UI.** The cost bar's `ctx` cell becomes a mini progress bar:
  `▰▰▱ 34% · 43k/128k` — green < 60 %, amber < 85 %, red ≥ 85 %; tooltip breaks down
  system+tools vs history (from `model_view_len` and last usage). At red, a hint appears:
  "context nearly full — reset or continue with compaction".
- **D7.4 — Reset button.** `⟲` button beside the bar → confirm popover → `reset_chat_session`.
  The visible transcript is **kept** with a `— context reset —` divider inserted (the backend
  clears `model_view` only; `user_view` retains display history). Stats/cost totals are not
  reset (they are money already spent).

### Acceptance
- After a 3-turn Bedrock chat, the bar shows the last request's real prompt tokens against the
  model's real window; pressing reset drops the estimate to ~system+tools size and the next
  request's `prompt_tokens` confirms it.

## P8 — Resurrect the TUI on a shared application core

### Current state & diagnosis
The TUI crate **compiles cleanly** (`cargo check -p tracelean-tui` passes) but is a 603-line
skeleton: file browser, requirements list, read-only file open via `service::` — no editing,
no undo, no AI chat, no agent. The reason it "completely doesn't work" is architectural, not a
build break: the application's AI/agent orchestration lives in the **Tauri layer** —
`gui_backend/src/ipc/ai_commands.rs` (636 lines) owns provider construction, AI settings
load/save, chat-session store, session stats + spend cap, the tool-loop entry point, and event
emission. None of that is reachable from the TUI, so every feature added since the TUI was
written landed GUI-only. `SharedApp` (`core/src/lib.rs:149`) was created exactly for
multi-frontend sharing and the TUI already holds one — it just doesn't contain the AI state.

### Design decisions
- **D8.1 — Core owns the application; frontends own pixels.** Everything in
  `gui_backend/src/ipc/*` that is not literally Tauri glue moves into core. Concretely,
  `SharedApp` grows the AI-side state that today lives in Tauri `State<>` wrappers:

  ```rust
  pub struct SharedApp {
      // existing: state, symbols, graph, …
      pub ai: AiService,          // NEW — in core
  }
  pub struct AiService {
      settings: Mutex<AiSettings>,          // load/save ~/.tracelean/ai_settings.json
      sessions: Mutex<HashMap<String, ChatSession>>,
      stats: Mutex<SessionStats>,           // + spend-cap check
      interaction_log: Mutex<InteractionLog>,
      registry: ToolRegistry, cache_registry: ProviderCacheRegistry,
      event_sink: Arc<dyn EventSink>,
  }
  ```

  `AiService` exposes the operations the IPC layer currently implements inline:
  `chat_turn(session_id, user_msg) -> AgentTurnResult`, `reset_session`, `get_models`,
  `update_settings`, `session_info` (P7), `provider()` factory. Tauri commands in
  `ai_commands.rs` become one-line dispatchers (the pattern `service.rs` already established
  for editor operations). **Definition of done: `gui_backend` contains no `ai::` logic beyond
  argument marshalling**, and the `EventSink` trait remains the only outbound channel — the
  TUI's sink renders events into its own widgets instead of Tauri events.
- **D8.2 — TUI recovery scope (parity tiers).** Rebuilt on `AiService` + `service::`:
  - *Tier 1 (this spec):* runtime hardening (panic hook restoring the terminal, error toasts
    instead of silent `Result` drops); working file open/edit/save with undo/redo through
    `AppState::apply` (same `Replace` primitive from P34 — the TUI gets divergence-free editing
    for free since it renders straight from backend buffers); AI chat pane with streaming text,
    tool-call chips (consuming the same structured events as P1), cost/context bar (P7 data),
    provider/model picker from settings.
  - *Tier 2 (later):* undo-tree visualization, traceability views, diff review (P10).
- **D8.3 — Regression guard.** A CI-runnable smoke test drives the TUI headless (ratatui
  `TestBackend`): open project → open file → type → undo → run a mock-provider chat turn with
  one tool call → assert rendered buffer contains the tool chip. This prevents the TUI from
  silently rotting again — it exercises the shared layer end-to-end with zero Tauri.

### Acceptance
- `tracelean-tui` in the example project: edit + save a file, run an agent turn on the mock
  provider, see tool chips and cost update — with `gui_backend` compiled out entirely.
- Diff of `gui_backend/src/ipc/ai_commands.rs` shows only thin dispatchers remain.

### Files touched
`core/src/lib.rs` (SharedApp), new `core/src/ai/service.rs` (AiService),
`gui_backend/src/ipc/ai_commands.rs` (gutted), `tui/src/*` (rebuilt views), `.tracelean`/config
path handling moved to core.

---

# Post-P9 Roadmap — proposed high-impact features
## P9 — Provider-native tool passing + cache-write-aware cost model

Two related provider-layer gaps. The system-prompt tool embedding was a *workaround for
Bedrock+MiniMax specifically* (Mantle didn't cache — and didn't reliably return — native tool
calls for MiniMax); it must not remain the default for everyone else.

### P9a — Native `tools` parameter for every provider that honors it

**Current state:** OpenRouter already sends the native `tools` field (`openrouter.rs:53,161`).
Bedrock embeds tools in the system prompt **unconditionally** (`bedrock.rs:706-772`) and its
response types don't even deserialize `message.tool_calls` (`ChatChoiceMessage`,
`bedrock.rs:423-427`) — so any Bedrock model that *does* return native tool calls appears
tool-less today (matches the observed "bedrock models seem disconnected from tools").

**Design decisions**
- **D9a.1 — Per-model tool-passing strategy**, alongside P2's `ToolCallFormat`:

  ```rust
  pub enum ToolPassing { NativeParam, SystemPromptEmbed }
  ```

  Assigned in `model_catalog::enrich` (+ overridable in `data/models.json`):
  Bedrock+MiniMax → `SystemPromptEmbed` (the P2 dialect); all other models → `NativeParam`
  until observed otherwise. The provider consults the strategy when building the request.
- **D9a.2 — Bedrock native path.** `ChatRequest` gains `tools: Option<Vec<ToolSchema>>` and
  `ChatChoiceMessage` gains `tool_calls: Option<Vec<ToolCallResponse>>`; when
  `NativeParam`, tools go in the request field, history keeps real `tool_calls` /
  `role: "tool"` messages (no text flattening), and parsed native tool calls take precedence
  over text-parsing (which stays as fallback so a model answering in Hermes text still works).
- **D9a.3 — Truth table stays observable.** The interaction log records which strategy was used
  (`tool_passing: "native" | "embedded"`) per request, so a model that silently drops native
  tool calls is diagnosable from the Log tab instead of by vibes. If a model proves broken
  natively, flipping one field in `models.json` reverts it to embedding — no code change.
- **D9a.4 — Cache interaction.** For `NativeParam` models the tools field participates in the
  provider's prefix cache (OpenAI-compatible providers cache it fine; the MiniMax/Mantle
  defect is exactly why *that* combination stays embedded). `provider_cache.json` notes record
  per-provider quirks.

**Acceptance:** a Bedrock non-MiniMax tool-supporting model (e.g. Nova/Claude via Mantle)
completes a 2-tool agent turn using native `tools` + structured `tool_calls`; MiniMax path
unchanged; regression fixtures cover both.

### P9b — Cost-model-driven explicit cache writes

**Current state:** the machinery exists but is unwired. `ProviderCacheConfig` knows
`CacheMode::Explicit`, `cache_write_multiplier` (e.g. Anthropic's 1.25×), `ttl_seconds`,
`requires_markers` (`provider_cache.rs:14-56`); `CacheMarkerPlanner::plan_markers`
(`ttl_tracking.rs:169-…`) and `TurnTimingTracker` idle statistics exist — but **no provider
ever emits a cache marker** (`grep cache_control` → zero hits in request building). For
explicit-cache models (Claude on Bedrock/OpenRouter) we currently pay full input price every
turn while *believing* caching is configured.

**Design decisions**
- **D9b.1 — Write-if-worth-it rule.** Before each request on a `requires_markers` model, the
  planner decides per candidate breakpoint (after tools/system, after conversation prefix):

  ```
  expected_savings = P(reuse within TTL) × prefix_tokens × (1 − read_discount) × input_price
  write_cost       = prefix_tokens × (write_multiplier − 1) × input_price
  place marker  ⟺  expected_savings > write_cost
  ```

  `P(reuse within TTL)` comes from `TurnTimingTracker`: fraction of recent inter-turn idle
  times below the provider TTL (`cold_turn_ratio` complement), floored at 0.5 mid-conversation
  (an active agent loop virtually guarantees a next request). This slots into the existing
  `CostDecision` vocabulary as `CacheWrite { breakpoint, expected_savings }` so decisions land
  in the same `DecisionLog` the pruning engine uses.
- **D9b.2 — Marker emission per provider dialect.** The request builders translate planned
  breakpoints into the wire format: OpenRouter/Anthropic-style `cache_control:
  {"type":"ephemeral"}` on the last content block of the chosen message; Bedrock Converse
  `cachePoint` where applicable. Max 4 breakpoints (Anthropic limit) — planner ranks by
  savings. Automatic-cache providers (`CacheMode::Automatic`) are untouched.
- **D9b.3 — Verify, don't trust.** `CachePredictionTracker` (already in `cost_model.rs:366`)
  records predicted vs actual `cached_tokens` per turn; ≥2 consecutive anomalies (paid write,
  got no cached reads) surface a warning in the Log tab and disable markers for the session —
  protecting against exactly the class of provider misbehavior that caused the MiniMax
  workaround.
- **D9b.4 — Stable prefixes are a precondition.** Marker placement assumes the prefix is
  byte-stable across turns; the P2 (dialect-stable history) and D9a.2 (native history) work
  guarantees this. The batching reminder currently appended to the *last user message*
  (`bedrock.rs:765-769`) is prefix-safe and stays.

**Acceptance:** on an explicit-cache model, a 5-turn agent session shows markers sent from
turn 1, `cached_tokens > 0` from turn 2, and the interaction log's cost breakdown showing
write surcharge on turn 1 and net savings by turn 3; with the mock provider simulating
cache-refusal, markers auto-disable after 2 anomalies.

### Files touched
`core/src/ai/{bedrock.rs, openrouter.rs, provider.rs, model_catalog.rs, ttl_tracking.rs,
cost_model.rs}`, `data/{models.json, provider_cache.json}`.

---
---

# Roadmap — proposed high-impact features (post-P10 other possible features to discuss)
---


Ordered by (impact ÷ effort), grounded in what the codebase already half-supports.
Numbering continues from the todo (P10+).

### P10 — Agent edit review: diff-first apply (accept/reject hunks)
**Why:** The single biggest trust gap. Agent edits currently land directly in buffers; the user
discovers changes after the fact via the undo tree. `ai/diff_pipeline.rs` already implements
shadow-buffer → hunks → accept/reject — it is simply not wired to the tool executor or any UI.
**What:** A "review mode" permission level in `AgentPermissions`: edit tools write to the shadow
buffer; the editor shows pending hunks inline (green/red, per-hunk ✓/✗, "accept all"); accepted
hunks become one `Batch` of `Replace` commands (one undo node per agent action, labeled with the
tool call). Rejected hunks are fed back to the agent as a tool result. Builds directly on P34's
primitive and P5's decoration work.
**Effort:** M (backend mostly exists; UI is the work).

### P11 — Persistent chat sessions
**Why:** Sessions and the interaction log die with the process; `ChatSwitcher` instances are
frontend-only state, so a crash or restart loses agent context mid-task — expensive with cached
prefixes (re-priming costs real money).
**What:** Persist `ChatSession` (user_view + model_view + usage counters) to
`.tracelean/sessions/{id}.json` on every turn; restore the switcher list at startup; label
sessions with first-message summary; per-session cumulative cost shown in the switcher.
**Effort:** S–M.

### P12 — Integrated terminal + test runner with agent feedback loop
**Why:** The agent has `run_shell`, but the *user* has no terminal, and test failures aren't
first-class. The regression harness (exercism tasks, benchmark bin) shows the repair loop is a
core goal — surfacing it in the IDE closes the loop: run tests → failures become structured
context → one click sends them to the agent.
**What:** Bottom terminal panel (xterm.js + Tauri pty), a "Run tests" action per project
(configurable command), failure parser (cargo test / pytest / vitest) producing clickable
diagnostics, and a "Fix with AI" button that opens a chat pre-seeded with the failing output +
relevant files.
**Effort:** M.

### P17 — Session replay regression harness for the agent stack
**Why:** P2-class bugs (parser vs model-format drift) recur with every provider/model change.
The mock provider + interaction log already capture full request/response pairs — replaying
recorded real sessions against the parser/runtime as unit fixtures would have caught P1, P2 and
P3 automatically.
**What:** "Export session as fixture" button in the Log tab → JSON fixture under
`tests/fixtures/sessions/`; a test runner that replays fixtures through `parse_tool_blocks`,
`build_messages`, and the runtime loop with the mock provider, asserting parsed calls/messages
match the recorded ones bit-for-bit.
**Effort:** S.

### Explicitly deferred (considered, not proposed now)
- **LSP integration** (rust-analyzer etc.): highest raw impact but very large scope; tree-sitter
  symbols cover current flows. Revisit after P10–P12.

---

# Suggested implementation order

| Phase | Items | Rationale |
|-------|-------|-----------|
| 1 | **P34** | Everything touches the editor; divergence bugs poison all testing of other features. |
| 2 | **P1 + P3** | Small, same files, immediately restores agent observability. |
| 3 | **P2** | Parser/dialect switch, protected by P17-style fixtures written as part of it. |
| 4 | **P7 + P5** | UI polish on now-stable foundations. |
| 6 | Roadmap P8, P9, P10, P11, P12, P17 … | In listed order. |
Deferred features for next time:
| 7 | **P6** | New capability; independent. |
| 8 | **LSP integration*** | LSP integation : independent |


