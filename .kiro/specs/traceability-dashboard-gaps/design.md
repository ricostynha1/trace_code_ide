# Design — Traceability Dashboard Gaps

## 1. Call-graph view (TD-01..04)

### Data source

Reuse existing symbol table (per-file, built in MVP1) plus new call-resolution pass:

```
for each function symbol F:
  for each call expression inside F's body (tree-sitter query):
    resolve callee name -> symbol table lookup (same file, then project-wide)
    if resolved: edge F -> callee_symbol (CallEdge)
    else: edge F -> ExternalLeaf(name)  (unresolved / stdlib)
```

Call-resolution is a new petgraph subgraph, separate from the requirement/spec/code/test trace graph but sharing the same `CodeElement` node IDs so a click can jump between views.

### View switcher

Dashboard gets a mode toggle: `Trace View` | `Call Graph View`. Same D3.js canvas, different node/edge set fed in. Filter/search/click-to-navigate (existing 5.3, 5.6) reused as-is since both are D3 graphs with the same interaction layer.

### Rendering

- Local calls: solid edge, `CodeElement` node styling (existing color coding).
- External/unresolved calls: dashed edge to a grey "leaf" node labeled with the call name, no navigation target.

## 2. Agent activity feedback (TD-05..07)

### Event stream

Agent tool executor (existing, MVP4 4.14b) already invokes each tool call. Add a Tauri event emission at each tool invocation:

```rust
app.emit("agent-activity", AgentActivityEvent {
    agent_id,
    action: "tool_call",
    tool_name,
    target: describe_target(&tool_call), // e.g. file path, requirement ID
    timestamp,
});
```

### Chat panel

Subscribes to `agent-activity`, renders a small inline status line ("Reading `src/auth.rs`...") above/below the streaming response, ephemeral (replaced by next event, kept in history log per 4.5).

### Graph highlight

Dashboard subscribes to the same `agent-activity` event. If `target` maps to a known graph node ID, apply a temporary highlight class (pulsing border) for ~2s or until next event. No graph rebuild needed — pure UI overlay.

## Risks

- Call resolution across files needs the full symbol table built (startup scan); on very large projects this could be slow — reuse existing Rayon parallel parse, no new perf work planned here.
- Agent activity event volume: throttle/debounce UI updates if tool calls fire faster than render (e.g. batch within 100ms window).
