



# Agent Architecture Decision

## Decision: Implement Zed ACP as our agent interface
Gathered all context on the internet to implement Zed ACP protocol)

**Why:**
1. Emerging standard for agent-to-editor communication (JSON-RPC 2.0, "LSP for agents")
2. Ships a default agent while remaining open to any ACP-compatible agent
3. Immediate compatibility with Claude Code, Codex CLI, Gemini CLI, etc.
4. Separates "IDE features" from "agent intelligence" — the right boundary
5. Other people can extend our IDE with their own agents without touching our code
6. Backed by Zed + JetB
rains, 60+ agents in registry, active development

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

