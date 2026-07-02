# Problems Spec – TraceLean IDE

Turns issues from [problemns.md](problemns.md) into concrete tasks.

---

## MVP 2 — Agent Harness Correctness

**Goal**: AI edits safe, buffered, command-driven.

**Already implemented** (from MVP 4):
- Shadow buffer → diff → hunk acceptance → Commands ✓ (`diff_pipeline.rs`)
- Agent permission model (`AgentPermissions`: tool allow/deny, path globs, shell control) ✓
- Tool executor respects permissions ✓
- `write_file` tool goes through command pipeline ✓
- AI edits are undoable (they become `Replace` commands) ✓

### Remaining Tasks

- [ ] 2.1 — Add tests for every harness command (tool calls produce expected Commands).
- [ ] 2.2 — Verify that no agent code path can mutate `AppState` without going through `apply()`. Audit `execute_write_file` and `execute_emit_command` — confirm shadow buffer is the only write path.
- [ ] 2.3 — In Docker mode: run agent process as unprivileged user that cannot write project files directly. Agent writes to buffer; application diffs and applies commands. (In fact in regular mode local for now also yes even though this system is only linux only !, docker is the path that will brig us multi platform suppoert)
- [ ] 2.5 — Tests: AI edits produce reversible commands, undo works. (For tests use mock agent!! )

**Deliverable**: AI edits traceable, permissioned, always enter system through reversible command pipeline. Proven by tests.

---
## MVP 1 — Undo Tree, Checkpoints, and Persistence

**Goal**: Make undo model trustworthy, compact, and replayable.

**Already implemented** (from MVP 0 + MVP 6):
- Undo tree with branching ✓
- Commit points with metadata ✓
- Persistence (command log + checkpoints) ✓
- Undo-tree visualization ✓
- Jump-to-node time travel ✓

### Remaining Tasks

- [ ] 1.1 — Remove `Batch` variant from `Command` enum. A batch is just a sequence of commands pushed atomically to the undo tree (single parent node with multiple children applied together). The current `Batch { commands: Vec<Command> }` is redundant abstraction over that.
- [ ] 1.2 — Differentiate ordinary undo nodes from explicit snapshot/commit nodes in the data model. Currently `commit_point: Option<CommitPoint>` on every node; Those nodes are equal to the rest, ahving a extra parameter bool true Snapshot with and extra paramters timesamp and other metadata. And are to be tranversable on undo tree menu (on where is today the commit ! (so change that instead of commit to snapshot))
- [ ] 1.3 — Snapshot trigger: button press only. Snapshot metadata: timestamp, label, touched files, test results (stub: `0 tests passed` until test runner exists in MVP 9).
- [ ] 1.4 — Snapshot creation calls a validation pipeline. For now: dummy fn returning `ValidationResult { tests_passed: 0, tests_total: 0, status: Pass }`. Leave hooks for: parsing check, compilation check, test execution.
- [ ] 1.5 — Compact repeated single-char inserts into word-level `Insert` when contiguous (same file, adjacent offset). Same for deletes.
- [ ] 1.6 — Persist undo tree (not just command log) in compact format. Current persistence saves flat command list + full state checkpoint. Need to have this written in a super compact form basically only snapshots are saved with all metadatast the rest of tommands are super compact like i5it to inser 5t, propose before implementing this serialization proptocol.
- [ ] 1.7 — Tests: insert/delete compaction correctness.
- [ ] 1.8 — Tests: branching behavior (undo + new edit creates branch).
- [ ] 1.9 — Tests: snapshot creation stores correct metadata, validation pipeline called.
- [ ] 1.10 — Tests: persistence round-trip (save tree → reload → identical state + branches).
- [ ] 1.11 — Tests: agent-generated edits (via mock agent) undoable deterministically.

**Deliverable**: Undo tree branches correctly, persists compactly (including branches), compacts edits, and validates on snapshot.

---

## MVP 3 — Trace Graph Fixes

**Goal**: Graph rendering works and is trustworthy.

**Already implemented** (from MVP 2 + MVP 5):
- petgraph-based trace graph with all node types ✓
- Scan project, build links by convention ✓
- D3.js rendering in TraceabilityDashboard ✓
- Query APIs: `query_requirement_trace`, `query_code_trace` ✓
- Live update on file changes ✓
- Filter, search, click-to-navigate ✓
- Forward traceability (req → spec → code → test) ✓
- Reverse traceability (code → spec → req) ✓

### Remaining Tasks

- [ ] 3.1 — Debug why graph renders empty in some cases. Likely: `build_trace_graph` not called on startup, or project structure doesn't match expected conventions. Add auto-build on project open.
- [ ] 3.2 — Add e2e tests: open example project → graph has expected nodes/edges.
- [ ] 3.3 — Ensure graph updates after edits (file watcher triggers re-scan of changed file).
- [ ] 3.4 — Ensure graph updates after checkpoint creation (snapshot should record trace state).

**Deliverable**: Trace graph shows current state reliably. Verified by tests.

---

## MVP 4 — Multi-Agent Collaboration

**Goal**: Multiple agents work independently, integrate into one shared linear history.

### Design

- Each agent session is anchored to a base state (a snapshot point).
- Agents work in isolation (own buffer/branch).
- Before integration: rebase agent commands against current main state using diff-based approach.
- Prefer diff-based rebasing over raw command concatenation.
- Preserve reverse operations so merged edits are rollback-safe.
- Agent branch must pass validation before integration to main.
- After each sequential integration: create a snapshot on main.

### Tasks

- [ ] 4.1 — Agent session model: each session records base state (snapshot ID) + sequence of commands.
- [ ] 4.2 — Agent isolation: agent applies commands to a fork of state, not shared state.
- [ ] 4.3 — Diff-based rebase: when integrating, compute diff between agent's result and current main, translate to commands against current main.
- [ ] 4.4 — Validation gate: block reintegration if agent branch fails validation pipeline.
- [ ] 4.5 — On successful rebase: append rebased commands to main history + create snapshot.
- [ ] 4.6 — Per-agent undo: within an agent session, undo only that agent's commands. (Guarantee as agent will work on its own global undo tree in isolation from the main project or the other agents. Only then in merging will he encounter the global merge tree)
- [ ] 4.7 — Tests: two agents edit same file concurrently → rebase produces correct merge.
- [ ] 4.8 — Tests: validation failure blocks integration.

**Deliverable**: Multiple agents work independently without corrupting shared history. Integration is gated by validation.

---