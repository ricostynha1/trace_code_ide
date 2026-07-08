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

Tools available: `read_file`, `write_file`, `str_replace`, `insert_lines`, `list_files`, `emit_command`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `run_shell`, `search_files`.

### MCP Integration

Two sides:
1. **MCP Host** (`mcp_host.rs`): Exposes our tools via JSON-RPC so external MCP clients can call them
2. **MCP Client** (`mcp_client.rs`): Connects to user-configured external MCP servers, discovers their tools, makes them available to our agents

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

### Remaining (Phase 4)
- Wire built-in agent to real AI providers (OpenRouter/Bedrock)
- Agent process binary (so it can be spawned as subprocess)
- UI components for multi-agent management
- Agent-to-agent delegation in practice (agent A spawns agent B)
- Persistent agent sessions across IDE restarts
