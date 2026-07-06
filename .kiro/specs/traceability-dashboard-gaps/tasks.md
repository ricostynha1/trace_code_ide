# Tasks — Traceability Dashboard Gaps

## Call-graph view

- [ ] 1. Build call-resolution pass: tree-sitter query for call expressions, resolve callee against symbol table (same-file then project-wide).
- [ ] 2. Emit `CallEdge` graph (petgraph) sharing `CodeElement` node IDs with existing trace graph.
- [ ] 3. Mark unresolved calls as `ExternalLeaf` nodes (grey, non-navigable).
- [ ] 4. Add view switcher (`Trace View` / `Call Graph View`) to TraceabilityDashboard.tsx.
- [ ] 5. Feed call-graph nodes/edges into existing D3 renderer; reuse filter/search/click-to-navigate.
- [ ] 6. Tests: sample project with known call chain → expected edges.

## Agent activity feedback

- [ ] 7. Emit `agent-activity` Tauri event from tool executor on each tool call (agent_id, tool_name, target, timestamp).
- [ ] 8. AiChatPanel.tsx: subscribe to event, render ephemeral status line, persist to interaction log.
- [ ] 9. TraceabilityDashboard.tsx: subscribe to event, highlight matching graph node briefly.
- [ ] 10. Debounce/batch rapid tool-call events to avoid UI flicker.
- [ ] 11. Tests: mock agent emits tool calls → chat panel and dashboard both reflect activity.
- [ ] 12. Update docs/IMPLEMENTATION.md: mark 5.5 and 5.8 done.
