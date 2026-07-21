# TraceLean Architecture

## System Overview

TraceLean is a traceability-first IDE that links requirements → specs → code → tests in real-time. It combines traditional editing with AI-agent integration through the Zed Agent Client Protocol (ACP).

```
┌─────────────────────────────────────────────────────────────┐
│                    TraceLean IDE                              │
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ React        │  │ TUI          │  │ Headless         │  │
│  │ Frontend     │  │ (ratatui)    │  │ (CI/testing)     │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────────┘  │
│         │                  │                  │              │
│         └──────────────────┼──────────────────┘              │
│                            │                                 │
│                   ┌────────▼────────┐                        │
│                   │   SharedApp     │                        │
│                   │  (Arc<...>)     │                        │
│                   └────────┬────────┘                        │
│                            │                                 │
│  ┌─────────────────────────▼─────────────────────────────┐  │
│  │              tracelean-core                            │  │
│  │                                                       │  │
│  │  AppState ← Command system (single mutation path)     │  │
│  │  UndoTree ← tree-structured history                   │  │
│  │  Parser   ← tree-sitter multi-language                │  │
│  │  TraceGraph ← requirement↔spec↔code↔test links        │  │
│  │  AI Module ← providers, agents, tool execution        │  │
│  │  ACP Module ← agent-client-protocol integration       │  │
│  │  MCP Host  ← tool server for external consumers       │  │
│  │  MCP Client ← connect to external tool servers        │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## Agent-Editor Architecture (ACP)

We implement the **Client** side of the [Agent Client Protocol (ACP)](https://agentclientprotocol.com). External agents run as subprocesses.

```
┌──────────────────────┐         stdio (JSON-RPC 2.0)         ┌────────────────────┐
│                      │  ─────────────────────────────────►  │                    │
│   TraceLean IDE      │         initialize                    │  External Agent    │
│   (ACP Client)      │  ◄─────────────────────────────────  │  (Claude Code,     │
│                      │         capabilities + agentInfo      │   Codex, Gemini,   │
│                      │  ─────────────────────────────────►  │   custom agents)   │
│                      │         session/new {cwd, mcpServers} │                    │
│                      │  ◄─────────────────────────────────  │                    │
│                      │         {sessionId}                   │                    │
│                      │  ─────────────────────────────────►  │                    │
│                      │         session/prompt {content}      │                    │
│                      │  ◄─────────────────────────────────  │                    │
│                      │         session/update (streaming)    │                    │
│                      │  ◄─────────────────────────────────  │                    │
│                      │         fs/read_text_file             │                    │
│                      │  ─────────────────────────────────►  │                    │
│                      │         {content}                     │                    │
│                      │  ◄─────────────────────────────────  │                    │
│                      │         fs/write_text_file            │                    │
│                      │  ─────────────────────────────────►  │                    │
│                      │         (ok)                          │                    │
└──────────────────────┘                                       └────────────────────┘
```

### Key Design Points

- **IDE as Client**: We launch agents, send prompts, handle their fs/permission requests
- **Agent as subprocess**: Communicates via stdin/stdout newline-delimited JSON-RPC 2.0
- **Buffer-aware reads**: `fs/read_text_file` returns unsaved editor content (not just disk)
- **Undo-preserving writes**: `fs/write_text_file` goes through Command system (full undo chain)
- **MCP forwarding**: IDE passes MCP server configs to agent via `session/new` → agent gets tools natively
- **Permission gates**: Agent requests permission via `session/request_permission` before destructive ops

### Protocol Stack

```
┌─────────────────────────────────────────┐
│  ACP v1   (agent ↔ IDE/editor)          │  ← agent-client-protocol crate
├─────────────────────────────────────────┤
│  MCP      (agent ↔ tools/data sources)  │  ← hand-rolled (Phase 1), rmcp (Phase 2)
└─────────────────────────────────────────┘
```

**ACP** = how agents talk to the editor (sessions, prompts, file ops, permissions).
**MCP** = how agents talk to external tools (databases, APIs, custom servers).

They're complementary. ACP sessions can carry MCP server configs so agents get tools automatically.

---

## Command System & Undo Tree

All state mutations go through a single path:

```
User/Agent action → Command → AppState.apply(cmd) → inverse stored → UndoTree.push(cmd, inverse)
```

### Command Types (exhaustive)

| Command | Effect |
|---------|--------|
| `Insert {file, offset, text}` | Insert text at byte offset |
| `Delete {file, offset, len, deleted_text}` | Delete text (stores deleted for inverse) |
| `SetCursor {file, new_pos, prev_pos}` | Move cursor |
| `SetSelection {file, new_range, prev_range}` | Set selection |
| `CreateFile {path}` | Create empty file |
| `DeleteFile {path, content}` | Delete file (stores content for inverse) |
| `RenameFile {from, to}` | Rename/move file |
| `Batch {commands}` | Atomic group (inverse = reversed inverses) |

### Undo Tree (not a stack)

```
        ○ root
       / \
      ○   ○ ← branch created on undo+edit
     /
    ○ ← current
   / \
  ○   ○ ← future branches possible
```

- Every command creates a child node
- Undo moves to parent, redo moves to latest child
- `jump_to(node_id)` finds LCA path and replays commands
- Commit points mark named snapshots (with coverage/conformance metadata)
- **Hover diff**: `node_content_diff(id)` clones state, replays to target, produces unified diff

---

## Trace Graph

Links requirements → specs → code → tests bidirectionally.

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│ REQ-01   │────►│ spec/    │────►│ src/     │────►│ tests/   │
│ (reqs/)  │     │ REQ-01   │     │ auth.rs  │     │ auth_    │
│          │◄────│ .lean    │◄────│          │◄────│ test.rs  │
└──────────┘     └──────────┘     └──────────┘     └──────────┘
```

- **Nodes**: Requirements (.md), Specs (.lean), Code elements (functions/structs), Tests
- **Edges**: Derived from naming conventions, `@trace` annotations, spec imports
- **Queries**: "What code implements REQ-01?", "What requirements does this function satisfy?"
- **Incremental**: File changes trigger targeted re-scan (not full rebuild)

---

## AI Module

### Provider Abstraction

```rust
trait AiProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError>;
    fn name(&self) -> &str;
    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError>;
}
```

Implementations: OpenRouter, AWS Bedrock, Mock (for testing/dev).

### Agent Pipeline

```
Prompt → Context Assembly → LLM Call → Response Parse → Tool Calls → Execute → Loop
```

Built-in agents: Elicitation, Formalisation, Implementation, Repair.

### Tool Execution

```
Agent emits ToolCall → permission check → execute_tool() → ToolResult back to agent
```

Tools available (canonical names, single source of truth `data/tools.json`): `read_file`, `edit_file`, `replace_str`, `list_directory`, `find` (regex/semantic/auto), `delete_file`, `run_shell`, `web_search`, `web_fetch`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `discover_tools`, `help_tool`. Argument validation (required/typed params) is schema-driven straight from `data/tools.json` (`ai/tool_errors.rs::validate_args`, gated in `ai/tool_executor.rs::execute_tool_reviewed`) — no per-tool hardcoded error text.

### MCP Integration

Two sides:
1. **MCP Host** (`mcp_host.rs`): Exposes our tools via JSON-RPC so external MCP clients can call them
2. **MCP Client** (`mcp_client.rs`): Connects to user-configured external MCP servers, discovers their tools, makes them available to our agents

### Agent Shell Sandboxing

`core/src/ai/shell_sandbox.rs`. Every `run_shell` tool call goes through one
mechanism that does containment *and* diffing at once: the project is bound
into a bubblewrap (`bwrap`) overlay mount namespace whose upper dir is
persisted after the command exits. The real project tree is physically
untouched during the run — walking the upper dir once yields every
create/modify/delete the command performed, fed back through the normal
`Command`/undo-tree machinery instead of a special-cased apply path.

- **Backends, in preference order**: overlay (full containment + exact diff) → strace fallback (no containment, but mutated paths are recovered from the syscall trace) → none (unsandboxed, visible notice; `detect` mode only).
- `PROJECT_ALLOWLIST` — project-relative dirs bound writable but *not* diffed (regenerable build/cache dirs: `target`, `node_modules`, `.venv`, `.tracelean/tmp`, …). `.tracelean/tmp` is the sanctioned place for an agent-written throwaway script (`SYSTEM_PROMPT` in `ai/mod.rs` tells the agent this directly).
- `PROTECTED` — paths (`.git`, `.tracelean`) whose upper-dir changes are discarded even where technically writable; never replayed onto the real tree.
- Mode is user-controlled via `AiSettings::shell_sandbox` (`"off"` / `"detect"` / `"strict"`) and network access via `shell_network` (`"deny"` / `"ask"` / `"allow"`).

### Cost Model & Context Compaction

`core/src/ai/cost_trimmed_summary_model.rs`, `ttl_tracking.rs`,
`trimmed_rules_table.rs`, driven from `agent/runtime.rs::cost_aware_compact`.
Every request runs a cut-point cost scan instead of a guessed
"context is getting big, truncate" threshold:

- **Rules table** (`trimmed_rules_table.rs`) answers a type-based question only: is this message ever allowed to be pruned at all? (user messages: never; stale tool results/assistant turns: eligible once they age out of the recent-turns window.)
- **Cost scan** (`batch_prune_decisions`) then decides *whether* and *how far* to cut among what the rules table allowed: `savings(x)` sums the expected future read-discount value of everything at/after cut `x`; `penalty(x)` is the cached-prefix invalidation that cutting there would cost, computed via `estimate_prefix_invalidation`. Only acts if net benefit is positive.
- **Cache markers** (`ttl_tracking::CacheMarkerPlanner`) plan explicit `cache_control`-style breakpoints for providers that require them (Anthropic-style); automatic-caching providers (Bedrock/MiniMax) instead get a passive prediction from the shared prefix with the previous request (`agent/runtime.rs::relative_cached_prefix_tokens`/`common_prefix_tokens`).
- Both paths account for tool-schema tokens explicitly (`estimate_tools_tokens`) — they're a separate `AiRequest` field, not a `ChatMessage`, but occupy real cached wire tokens and used to be silently treated as free.
- User-facing tuning knobs: `AiSettings::n_expected_rounds` (aggressiveness) and `disable_context_trimming` (full on/off switch for isolating caching behavior).

### Myth (Keyboard-Driven, Structured-Document UI)

`core/src/myth/` (`mod.rs`, `surface.rs`, `actions.rs`, `keymap.rs`,
`bindings.rs`). Every Myth-aware UI surface is content + a parser producing
nodes with captures, plus a binding map attaching actions to captures —
the same model whether it's rendered as rich rows in the GUI or styled text
in the TUI:

- **`Surface`/`SurfaceNode`** (`surface.rs`): content parsed into nodes (`capture`, line, char range, text, JSON meta). `FileTreeSurface` is the only current implementation (workspace → indented text → `@dir`/`@file` nodes); `capture` is documented to extend to code-highlight captures later.
- **`ActionRegistry`** (`actions.rs`): a static table of pure functions (`open_file`, `rename_file`, `delete_file`, `create_file`, `save_file`, `undo`, `redo`, `copy`, plus 4 semantic-nav actions). State-mutating actions return `Command`s — they never mutate `AppState` directly, so every action is undoable for free.
- **`Keymap`** (`keymap.rs`): a mode machine over `ui_settings/keymap.json` (`Main` → `Options` → `File`, transitions and dispatches). Which-key is a *query* over this data (`bindings_for_state`), not a separate feature. Validated at startup: an unknown action or a transition to an undefined state fails `keymap.rs`'s `shipped_keymap_is_valid` test.
- **Coverage today**: only `Editor.tsx` and `FileTree.tsx` call `myth_key_event`/render the which-key bar. `AiChatPanel`, `DiffReviewPanel`, `UndoTreePanel`, `TerminalPanel`, `MenuBar`, `EditorDiffBar` have no Myth wiring — they rely on plain `<button>`s (Tab/Enter-accessible, but no leader-key/which-key discoverability). Extending coverage there is unstarted work, not a regression.

---

## Frontend Architecture

### GUI (Tauri + React)

```
React components → invoke("command_name", args) → Tauri IPC → service layer → core
                                                                    ↑
                                                              events emitted back
```

- **Editor**: Monaco-like buffer with tree-sitter highlights
- **Undo Tree Panel**: Visual tree with hover-diff, jump-to-node, commit points
- **Trace Panel**: Requirement status workflow, spec navigation, coverage display
- **AI Panel**: Chat, streaming responses, diff acceptance/rejection

### TUI (ratatui)

Same `SharedApp` backend, different rendering. Terminal-native UI for SSH/remote usage.

---

## Persistence

```
.tracelean/
├── commands/
│   ├── log.bincode      ← command log (replay on open)
│   └── checkpoint.json  ← periodic full-state snapshot
└── mcp.json             ← MCP server configuration
```

- Commands are replayed on project open to restore state
- Checkpoints prevent long replay times (created every 100 commands)
- Both are self-contained — project is portable

---

## Surgical Edit System

For AI-generated edits, multiple strategies with automatic fallback:

```
AST Edit (precise) → Patch Edit (unified diff) → Search/Replace → Shadow Diff (full file)
```

- **AST Edit**: Tree-sitter-aware, edits by symbol name/path
- **Patch Edit**: Apply unified diffs directly
- **Search/Replace**: Line-based find+replace with fuzzy matching
- **Shadow Diff**: Buffer → diff → accept/reject hunks individually

---

## File Layout

```
tracelean/
├── core/              ← tracelean-core (no UI deps)
│   └── src/
│       ├── acp/       ← Agent Client Protocol (Zed ACP v1)
│       ├── ai/        ← AI providers, agents, tools, MCP
│       ├── surgical_edit/ ← edit strategies
│       ├── trace_graph/   ← requirement tracing
│       ├── state.rs       ← AppState + Command system
│       ├── undo_tree.rs   ← tree-structured history
│       ├── commands.rs    ← Command enum
│       ├── parser.rs      ← tree-sitter integration
│       ├── service.rs     ← domain orchestration
│       └── lib.rs         ← SharedApp + exports
├── gui_backend/       ← Tauri app (thin IPC dispatch)
│   └── src/ipc/       ← command modules by domain
├── tui/               ← Terminal UI
├── react_frontend/    ← React + TypeScript UI
└── tests/             ← integration tests
```

---

## Phase Roadmap

### Phase 1 ✓
- ACP Client infrastructure (types, server, transport)
- Can launch agent subprocesses and exchange messages
- fs/read and fs/write handle buffer state + undo chain
- Hand-rolled MCP for tool hosting and client connections

### Phase 2 ✓
- Real agent subprocess launch via `AcpAgent::from_args()` + `Client.builder()`
- Full prompt turn lifecycle (streaming session/update → AcpEvent → UI)
- Permission request flow (agent → UI → respond via channel)
- MCP server forwarding to agents via `NewSessionRequest.mcp_servers()`
- fs/read_text_file returns unsaved buffer content
- fs/write_text_file preserves undo chain via Command system

### Phase 3 ✓
- Built-in default agent (`builtin_agent.rs`) — ACP Agent over stdio
  - Turn-based loop: prompt → LLM → tool calls → results → loop
  - Tools: read_file, write_file, list_directory, search, run_command
  - Permission requests before write operations
  - Context compaction (truncate old tool results, summarize old messages)
  - Steering channel (abort/redirect via tokio::sync::watch)
  - TODO markers for wiring real AI provider (needs API keys from settings)
- Multi-agent orchestrator (`orchestrator.rs`)
  - Multiple simultaneous agent connections
  - Route prompts to specific agents by name
  - Sub-agent delegation
  - Conversation history tracking per agent
  - Unified event stream via EventSink

### Phase 4 ✓ (partially stale as of this section — verify against code before trusting fully)
- Built-in agent wired to real AI providers (OpenRouter, Bedrock) — see `ai::bedrock`/`ai::openrouter`, `AiSettings::active_provider`.
- Cost-aware context compaction (pruning + summarization) driven by real pricing, not guessed thresholds — `agent/runtime.rs::cost_aware_compact`, `ai::cost_trimmed_summary_model`.
- Shell sandboxing for agent commands (overlay+bwrap, strace fallback) — `ai::shell_sandbox`.

### Remaining
- Agent process binary (so the built-in agent can be spawned as a subprocess like external ACP agents).
- UI components for multi-agent management.
- Agent-to-agent delegation in practice (agent A spawns agent B).
- Persistent agent sessions across IDE restarts.
- MYTH keyboard coverage outside Editor/FileTree (see Myth section below).
