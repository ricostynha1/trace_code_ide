# Spec: AI Chat — Sub-agents & Tool Call Visibility

## Overview

Two enhancements to the AI chat interface:
1. **Sub-agent delegation** — main agent can spawn focused sub-agents
2. **Tool call transparency** — visible, distinct UI for tool invocations with reasoning

---

## Industry Research: Sub-agent Strategies

### What the players do

| Tool | Sub-agent approach | Key features |
|---|---|---|
| **Claude Code** | Native sub-agents via `Task` tool | Isolated context, own tool set, own model, resumable. Orchestrator = slash command. Sub-agents can't call other sub-agents. |
| **Cursor** | Background agents (up to 8 parallel) + Commands as workflow steps | Git worktree isolation, orchestrator commands reference step files. All in same session or separate. |
| **GitHub Copilot** | Custom agents + `#runSubagent` tool + handoffs | Each agent has own model/tools/prompt. Handoffs = button to switch context to next agent. Agent-to-agent delegation via experimental `runSubagent`. |
| **Windsurf** | Workflows (sequential agent steps) | Same concept as Cursor commands. No native sub-agent isolation. |

### Common patterns

1. **Sub-agent = tool call** — the orchestrator treats each sub-agent as a tool it can invoke
2. **Isolated context** — sub-agent gets its own context window, only returns a result summary
3. **Configurable per sub-agent**: model, temperature, allowed tools, permissions
4. **Use cases**: research/context gathering, code search, parallel file analysis, test generation, documentation
5. **Human-in-the-loop optional** — approval gates between steps improve success rate (59% → 86% for 5-step workflows)

### Our approach

Match Claude Code's model: sub-agents are invoked as a tool by the main agent, run in isolation, return structured result. Configurable model/tools per sub-agent profile. For content selection / context assembly tasks (our primary use case), sub-agents are ideal — they can search, filter, and summarize without polluting the main chat context.

---

## Feature 1: Sub-agent System

### Data Model

```rust
// In core/src/ai/subagent.rs (new module)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentProfile {
    pub id: String,
    pub name: String,
    pub description: String,
    pub model: Option<ModelConfig>,      // None = inherit from parent
    pub system_prompt: Option<String>,
    pub allowed_tools: Vec<String>,      // subset of available tools
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentInvocation {
    pub profile_id: String,
    pub task_prompt: String,             // what the main agent asks it to do
    pub parent_request_id: String,       // links back to main conversation
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    pub profile_id: String,
    pub summary: String,                 // returned to main agent context
    pub artifacts: Vec<String>,          // file paths, code blocks, etc.
    pub token_usage: TokenUsage,
}
```

### Settings (user-configurable)

Add to `AiSettings`:
```rust
pub subagent_profiles: Vec<SubAgentProfile>,
```

Default profiles shipped:
- **context-gatherer** — searches codebase, assembles relevant context
- **code-reviewer** — reviews diffs, returns findings
- **test-writer** — generates tests for given code

### Execution Flow

```
User message → Main Agent
  Main Agent decides to delegate → emits ToolCall("invoke_subagent", {profile, task})
    → Core spawns sub-agent with isolated AiRequest
    → Sub-agent runs (may call tools internally)
    → Returns SubAgentResult.summary to main agent
  Main Agent continues with enriched context
```

### IPC Commands (new)

```rust
// Settings
get_subagent_profiles() -> Vec<SubAgentProfile>
update_subagent_profile(profile: SubAgentProfile) -> Result
delete_subagent_profile(id: String) -> Result

// Execution (called by main agent internally, or manual trigger)
invoke_subagent(profile_id: String, task: String) -> SubAgentResult
```

---

## Feature 2: Tool Call Visibility in Chat

### Problem

Currently tool calls happen invisibly. User sees only final AI response. They can't tell what the agent is doing or why.

### Solution

Each tool call produces a **ToolCallMessage** displayed in the chat stream, visually distinct from user/assistant messages.

### Data Model

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallMessage {
    pub tool_name: String,
    pub reason: String,          // 1-sentence explanation from the AI
    pub status: ToolCallStatus,
    pub nested_calls: Vec<ToolCallMessage>,  // if tool triggers more tools
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolCallStatus {
    Running,
    Completed,
    Failed(String),
}
```

### Chat Message Types (extended)

Current chat has: `User`, `Assistant`. Add:

```rust
pub enum ChatMessageKind {
    User,
    Assistant,
    ToolCall(ToolCallMessage),       // NEW — visually distinct
    SubAgentCall {                   // NEW — for sub-agent invocations
        profile_name: String,
        task_summary: String,
        status: ToolCallStatus,
    },
}
```

### AI Agent Prompt Requirement

The system prompt must instruct the AI to include a `reason` field when calling any tool:
```
When calling a tool, always include a one-sentence "reason" explaining WHY you are calling it.
Format: {"tool": "...", "args": {...}, "reason": "..."}
```

The `reason` is extracted during response parsing and shown in the UI.

### UI Rendering (React + TUI)

**Visual treatment — distinct from normal chat:**
- Background: darker/muted (e.g., `#1a1a2e` vs normal `#0f0f23`)
- Left border accent: orange/amber for tool calls, purple for sub-agents
- Monospace font for tool name
- Collapsible: shows tool name + reason by default, expands to show nested calls
- Animated spinner while `status == Running`

**React (AiChatPanel.tsx):**
```tsx
<div className="tool-call-msg">
  <span className="tool-icon">⚙️</span>
  <span className="tool-name">read_file</span>
  <span className="tool-reason">Reading auth.rs to check current login implementation</span>
  <span className="tool-status">✓</span>
</div>
```

**TUI (ui/ai_chat.rs):**
```
 ┊ ⚙ read_file — Reading auth.rs to check current login implementation ✓
 ┊   ⚙ search_files — Finding all callers of login() ... 
```

### Nested Tool Calls

When a tool call triggers further tool calls (e.g., sub-agent internally calling tools):
- Display as indented children under parent
- Same visual style, increasing indent
- Collapsible in GUI, always-visible in TUI (space is cheap vertically)

---

## Implementation Plan

### Phase 1: Tool Call Visibility (simpler, immediate value)

1. Extend `response_parser.rs` to extract `reason` from tool calls
2. Add `ToolCallMessage` to chat message stream
3. Update `AiChatPanel.tsx` to render tool call messages distinctly
4. Update TUI `ui/` to render tool calls
5. Add CSS for distinct visual treatment

### Phase 2: Sub-agent System

1. Create `core/src/ai/subagent.rs` — profile storage, execution logic
2. Register `invoke_subagent` as a built-in tool in `tools.rs`
3. Add sub-agent settings UI (React + TUI)
4. Wire into `tool_executor.rs` for execution
5. Add sub-agent messages to chat stream (similar to tool calls, different accent)
6. Add IPC commands for profile CRUD

### Phase 3: Default Profiles & Polish

1. Ship default sub-agent profiles
2. Add approval gates (optional: ask user before spawning sub-agent)
3. Token budget tracking per sub-agent
4. Parallel sub-agent execution support

---

## Open Questions

1. **Should sub-agents have access to the full conversation history?** Recommendation: No — they get only the task prompt + project context. This matches Claude Code's isolation model.
2. **Can sub-agents call other sub-agents?** Recommendation: No (prevents runaway recursion). Single level only, matching Claude Code.
3. **Token budget per sub-agent?** Recommendation: Configurable per profile with a default cap (e.g., 4k output tokens).
