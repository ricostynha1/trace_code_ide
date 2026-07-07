## Current implementation priorities

- [ ] T1: Hover diff view on undo tree — compare current file state vs hovered node state

## T1 Implementation Write-up: Undo Tree Hover Diff

**Goal:** When hovering a node in the undo tree, show a real unified diff between the current editor state and the state at that node.

**Approach (clone + replay):**
1. Clone current `AppState` (it's already `Clone`)
2. Use `undo_tree.jump_to(target)` on the clone to get commands
3. Execute those commands on the clone via `execute_raw`
4. Extract the file content from the clone's buffer
5. Diff current content vs clone content using simple line-based diff
6. Return unified diff string

~15 lines in `state.rs`. Uses existing infrastructure. Add caching later if hover feels slow.

**Backend:** New method `pub fn node_content_diff(&self, node_id: NodeId) -> String` in state.rs.

**Frontend:** Already calls `get_undo_node_diff` and displays result. No changes needed.

---

# Agent Architecture Decision

## Decision: Implement Zed ACP as our agent interface

**Why:**
1. Emerging standard for agent-to-editor communication (JSON-RPC 2.0, "LSP for agents")
2. Ships a default agent while remaining open to any ACP-compatible agent
3. Immediate compatibility with Claude Code, Codex CLI, Gemini CLI, etc.
4. Separates "IDE features" from "agent intelligence" — the right boundary
5. Other people can extend our IDE with their own agents without touching our code
6. Backed by Zed + JetBrains, 60+ agents in registry, active development

## Protocol Stack (settled as of mid-2026)

```
├─────────────────────────────────────────┤
│  Zed ACP      (agent ↔ editor/IDE)      │  ← To implement
├─────────────────────────────────────────┤
│  MCP          (agent ↔ tools/data)      │  ← We already have this
└─────────────────────────────────────────┘
```

## Architecture: Message-Passing Only

Agent communicates with IDE ONLY via message passing:
```
Agent → IDE: tool calls (read_file, write_file, etc.) via MCP
IDE → Agent: tool results via MCP
Agent → UI: events (message_start, message_update, tool_execution_start, etc.)
UI → Agent: prompts, steering messages, abort signals
```

Benefits:
- Agent is testable in isolation
- Agent is replaceable (swap for any ACP agent)
- Other people can connect their own agents
- Can run agent in separate process/container for isolation

## Implementation Roadmap

### Phase 1: ACP Server (make IDE agent-ready)
- Implement ACP JSON-RPC server as a core feature
- Expose tools via ACP: read_file, write_file, str_replace, emit_command, query_trace_graph, etc.
- Handle sessions, capability negotiation, permission gates
- Any ACP agent can now connect

### Phase 2: Built-in Default Agent (Rust)
- Turn-based agent loop: prompt → LLM → tool calls → results → loop
- Steering queue (abort/redirect mid-execution)
- Context compaction (drop old tool results, truncate long content, respect context window)
- Streaming events to UI
- Connect to IDE via same ACP interface (dogfooding)

- For now simple agent after this step we will handle more complex delegations etc etc