# Agent Architecture Decoupling — Implementation Plan

## Goal

Extract the agent runtime from `gui_backend/` into `core/` so that:
1. The agent loop runs headlessly (benchmarks, CLI, tests)
2. Tauri GUI and TUI share the exact same agent code
3. Agent can run as ACP subprocess (replaceable by Claude Code, Codex, etc.)
4. All state mutations still go through `Command → state.execute()` — invariant preserved

## Current State (Problems)

The agent loop lives in `gui_backend/src/ipc/ai_commands.rs`:
- Takes 12 Tauri `State<>` parameters
- Directly coupled to `AppHandle` for event emission
- ~600 lines of loop logic that cannot run without Tauri
- Duplicates what `acp/builtin_agent.rs` should be doing (but that has `call_llm` stubbed)

```
gui_backend/src/ipc/ai_commands.rs   ← agent loop (Tauri-locked)
core/src/acp/builtin_agent.rs        ← ACP agent shell (LLM stubbed)
core/src/ai/tool_executor.rs         ← tool execution (already decoupled ✓)
core/src/ai/provider.rs              ← provider trait (already decoupled ✓)
core/src/ai/tools.rs                 ← tool definitions (already decoupled ✓)
core/src/state.rs                    ← AppState + Command execution (already decoupled ✓)
```

## Target Architecture

```
core/src/
├── agent/                    ← NEW: headless agent runtime
│   ├── mod.rs               (pub exports)
│   ├── context.rs           (AgentContext — all deps without Tauri)
│   ├── runtime.rs           (the LLM tool loop, extracted from ai_commands.rs)
│   └── error.rs             (AgentError type)
├── ai/                       (unchanged — providers, tools, executor)
├── acp/builtin_agent.rs      (wired to use agent::runtime instead of stub)
├── state.rs                  (unchanged — Command execution)
└── lib.rs                    (re-exports agent module)

core/src/bin/
├── tracelean-agent.rs        ← NEW: standalone ACP agent binary
└── tracelean-bench.rs        ← NEW: benchmark runner binary

gui_backend/src/ipc/
└── ai_commands.rs            ← THIN: builds AgentContext from Tauri State, delegates
```

## Key Abstraction: AgentContext

```rust
// core/src/agent/context.rs

/// Everything the agent loop needs — no Tauri, no UI framework.
pub struct AgentContext {
    pub state: Arc<Mutex<AppState>>,
    pub symbols: Arc<Mutex<SymbolTable>>,
    pub graph: Arc<Mutex<TraceGraph>>,
    pub settings: Arc<Mutex<AiSettings>>,
    pub stats: Arc<Mutex<SessionStats>>,
    pub log: Arc<Mutex<InteractionLog>>,
    pub mcp_client: Arc<tokio::sync::Mutex<McpClientManager>>,
    pub permissions: AgentPermissions,
    pub project_root: PathBuf,
    pub event_sink: Arc<dyn EventSink>,  // abstracts UI notifications
    pub spend_cap_usd: f64,
}

impl AgentContext {
    /// Build from SharedApp (used by both Tauri and TUI).
    pub fn from_shared_app(app: &SharedApp, project_root: PathBuf) -> Self { ... }

    /// Build from environment (standalone binary / benchmarks).
    pub fn from_env() -> Self { ... }
}
```

## Key Abstraction: Runtime

```rust
// core/src/agent/runtime.rs

/// Run one agent turn: prompt → [tool loop] → final response.
/// This is the full loop currently in ai_commands.rs lines 176-580.
pub async fn run_agent_turn(
    ctx: &AgentContext,
    messages: Vec<ChatMessage>,
) -> Result<AgentTurnResult, AgentError> { ... }

/// Result of an agent turn.
pub struct AgentTurnResult {
    pub response: AiResponse,
    pub tool_calls_executed: Vec<ToolCallRecord>,
    pub iterations: u32,
}
```

## Invariant Preservation

All paths through the system maintain: **Agent → tool_call → execute_tool() → Command → state.execute()**

| Mode | Tool execution location | State ownership |
|------|------------------------|-----------------|
| In-process (Tauri/TUI) | `agent::runtime` calls `tool_executor::execute_tool()` | Same process, shared via `Arc<Mutex<AppState>>` |
| ACP subprocess | IDE handles `WriteTextFile` req → calls `execute_tool()` | IDE owns state, agent only sees results |
| Headless (bench) | `agent::runtime` calls `execute_tool()` directly | Bench owns state locally |

The agent **never** bypasses the Command layer regardless of execution mode.

---

## Implementation Subtasks

### Task 1: Create `core/src/agent/` module

Create the module structure with `AgentContext` and `AgentError`.

**Files to create:**
- `core/src/agent/mod.rs`
- `core/src/agent/context.rs`
- `core/src/agent/error.rs`

**References:**
- `core/src/lib.rs` — add `pub mod agent;`
- `core/src/lib.rs:SharedApp` (lines ~140-180) — model `AgentContext` from this

---

### Task 2: Extract loop from `ai_commands.rs` → `agent/runtime.rs`

Move the LLM tool loop (lines 176-580 of `gui_backend/src/ipc/ai_commands.rs`) into `core/src/agent/runtime.rs`.

**Key logic to extract:**
- Tool schema assembly (builtin + MCP)
- Tool index / selection
- System prompt construction
- The `loop {}` with provider dispatch, tool execution, pause logic
- Spend cap enforcement
- Stats recording

**Files:**
- Create: `core/src/agent/runtime.rs`
- Modify: `gui_backend/src/ipc/ai_commands.rs` — replace loop body with `agent::runtime::run_agent_turn(ctx, messages).await`

**References:**
- `gui_backend/src/ipc/ai_commands.rs` lines 176-580 — the loop to extract
- `core/src/ai/tool_executor.rs:execute_tool()` (line 117) — called inside loop
- `core/src/ai/tool_selector.rs:ToolIndex` — used for tool selection
- `core/src/ai/tools.rs:builtin_tool_schemas()` — tool definitions
- `core/src/ai/provider.rs:AiProvider` trait — provider dispatch
- `core/src/ai/tracking.rs:SessionStats` — stats recording
- `core/src/ai/log.rs:InteractionLog` — interaction logging

---

### Task 3: Make `gui_backend/ai_commands.rs` a thin bridge

Reduce to: build `AgentContext` from Tauri State params → call `run_agent_turn()` → emit events.

**Files to modify:**
- `gui_backend/src/ipc/ai_commands.rs` — `ai_chat()` becomes ~30 lines
- `gui_backend/src/ipc/ai_commands.rs` — `ai_chat_stream()` same treatment
- `gui_backend/src/ipc/ai_commands.rs` — `resume_tool_loop()` delegates

**The EventSink bridge:**
```rust
// gui_backend/src/lib.rs — already has TauriEventSink
// It already implements EventSink trait — just pass it to AgentContext
```

---

### Task 4: Wire `acp/builtin_agent.rs` to use real runtime

Replace the stubbed `call_llm()` (line ~420) with actual provider calls via `AgentContext`.

**Files to modify:**
- `core/src/acp/builtin_agent.rs` — wire `handle_prompt_loop()` to use `agent::runtime`
- The ACP agent's tool execution stays IDE-side (ACP protocol handles this)

**Key change:** For ACP mode, tools execute on the CLIENT side (IDE), not agent side. The agent just receives results. So the ACP binary doesn't need `AppState` — it sends tool requests over the protocol and the IDE handles execution.

**References:**
- `core/src/acp/builtin_agent.rs:handle_prompt_loop()` (line ~240)
- `core/src/acp/builtin_agent.rs:call_llm()` (line ~420) — currently stubbed
- `core/src/acp/transport.rs` — IDE-side ACP client that handles tool requests

---

### Task 5: Add standalone agent binary

A binary that speaks ACP over stdio. Can be launched by the IDE, by Zed, or by any ACP client.

**Files to create:**
- `core/src/bin/tracelean-agent.rs`
- Update `core/Cargo.toml` — add `[[bin]]` section

**The binary:**
```rust
// Reads env: API key, model, region
// Speaks ACP over stdio
// Uses agent::runtime for LLM calls
// Tool calls go over ACP (IDE handles execution)
```

---

### Task 6: Add benchmark binary

Runs exercism tasks headlessly using `AgentContext::from_env()`.

**Files to create:**
- `core/src/bin/tracelean-bench.rs`
- Update `core/Cargo.toml` — add `[[bin]]` section

**The binary:**
- Creates headless `AgentContext` (AppState, SymbolTable, TraceGraph — all local)
- For each task: builds prompt, calls `run_agent_turn()`, runs pytest, records metrics
- Outputs JSON (compatible with existing `BenchmarkRun` struct in `ai/benchmark.rs`)
- Enforces cost cap

**References:**
- `core/src/ai/benchmark.rs` — existing structs (BenchmarkRun, BenchmarkTask)
- `core/src/lib.rs:SharedApp::new_headless()` — pattern to follow
- `scripts/benchmark_exercism.py` — keep as wrapper/alternative, but real logic in Rust

---

### Task 7: Update TUI to use same AgentContext

The TUI (`tui/src/main.rs`) should also be able to run the agent via the shared runtime.

**Files to modify:**
- `tui/src/app.rs` — add agent support using `AgentContext::from_shared_app()`

---

## Dependency Graph

```
Task 1 (module structure)
  ↓
Task 2 (extract loop)
  ↓
Task 3 (thin bridge)       Task 4 (wire ACP)       Task 7 (TUI)
                              ↓
                           Task 5 (agent binary)
                              ↓
                           Task 6 (bench binary)
```

Tasks 3, 4, 7 can be done in parallel after Task 2.
Tasks 5, 6 depend on Task 4.

## Files Quick Reference

| File | Role | Action |
|------|------|--------|
| `core/src/agent/mod.rs` | Module root | CREATE |
| `core/src/agent/context.rs` | AgentContext struct | CREATE |
| `core/src/agent/runtime.rs` | The LLM tool loop | CREATE (extract from ai_commands.rs) |
| `core/src/agent/error.rs` | Error types | CREATE |
| `core/src/bin/tracelean-agent.rs` | Standalone ACP binary | CREATE |
| `core/src/bin/tracelean-bench.rs` | Benchmark binary | CREATE |
| `core/src/lib.rs` | Re-exports | MODIFY (add `pub mod agent`) |
| `core/Cargo.toml` | Binary targets | MODIFY (add `[[bin]]` entries) |
| `gui_backend/src/ipc/ai_commands.rs` | Tauri bridge | MODIFY (thin out to delegate) |
| `core/src/acp/builtin_agent.rs` | ACP agent | MODIFY (wire real LLM) |
| `tui/src/app.rs` | TUI app | MODIFY (add agent support) |

## Success Criteria

- [ ] `cargo run --bin tracelean-bench -- hello-world` runs exercism task headlessly
- [ ] `cargo run --bin tracelean-agent` speaks ACP over stdio with real LLM
- [ ] `cargo tauri dev` still works identically (gui delegates to same runtime)
- [ ] All existing tests pass (`cargo test --workspace`)
- [ ] Agent never imports/uses Tauri types
- [ ] All state mutations go through Command → state.execute()
