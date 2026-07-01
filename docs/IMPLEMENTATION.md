# Implementation Plan – TraceLean IDE

Incremental delivery. Each MVP is usable on its own. Later MVPs build on earlier ones.

---

## MVP 0 — Command-Sourced Editor Shell

**Goal**: A working editor where every action is a reversible command. The foundation everything else depends on.

### Tasks

- [x] 0.1 — Scaffold Tauri app (Rust backend + React frontend, builds and launches)
- [x] 0.2 — Define `Command` enum (Insert, Delete, Replace, SetCursor, SetSelection)
- [x] 0.3 — Implement `AppState` with `apply(Command) -> Inverse` as sole mutation path
- [x] 0.4 — Implement undo-tree data structure (tree of UndoNodes, branch on undo+edit)
- [x] 0.5 — Integrate CodeMirror 6 in webview, wire all keystrokes to produce Commands via Tauri IPC
- [x] 0.6 — Undo/redo working (Ctrl+Z walks tree, branching works)
- [x] 0.7 — Multi-file commands: `Batch{}` grouping, atomic undo across files
- [x] 0.8 — File tree panel (left side): open folder, list files, open file in editor
- [x] 0.9 — File operations as commands: CreateFile, DeleteFile, RenameFile
- [x] 0.10 — Command log persistence to `.tracelean/commands/` (serialize on change, replay on startup)
- [x] 0.11 — Periodic state checkpoints for fast startup
- [x] 0.12 — Docker compose: single container running the app (baseline)

**Deliverable**: A minimal code editor where you can open a project, edit files, undo/redo with full tree history, and restart without losing history.

---

## MVP 1 — Tree-sitter Integration

**Goal**: Syntax highlighting and code model extraction via tree-sitter.

### Tasks

- [x] 1.1 — Compile tree-sitter grammars to WASM (Python, Rust, C++, Lean 4)
- [x] 1.2 — Integrate tree-sitter WASM in CodeMirror 6 for real-time syntax highlighting
- [x] 1.3 — Integrate tree-sitter native (Rust) in backend for structural parsing
- [x] 1.4 — On file open/save: backend parses file → extracts symbols (functions, classes, modules)
- [x] 1.5 — Build in-memory symbol table per file (name, kind, location, dependencies)
- [x] 1.6 — Parallel parsing with Rayon on startup (scan all project files)
- [x] 1.7 — Incremental re-parse on file change (only changed file, triggered by file watcher)

- [ ] 1.8 — Adopt tree-sitter highlight queries (`.scm` files) for proper token-level highlighting instead of ad-hoc node-kind→color JSON approach. Current approach fails for markdown headings and other complex node structures where color must propagate through nested nodes.

**Deliverable**: Editor with fast syntax highlighting and a backend that understands code structure.

---

## MVP 2 — Traceability Graph (In-Memory)

**Goal**: Build and query the requirement→spec→code→test graph.

### Tasks

- [x] 2.1 — Define graph node types: Requirement, Spec, CodeElement, Test
- [x] 2.2 — Implement petgraph-based trace graph (directed edges, typed nodes)
- [x] 2.3 — Define file format for requirements (markdown with IDs, stored in `reqs/`)
- [x] 2.4 — Define file convention for Lean specs (path derived from requirement ID)
- [x] 2.5 — On startup: scan `reqs/`, `specs/`, `src/`, `tests/` → build graph
- [x] 2.6 — Link requirements → specs by file naming convention
- [x] 2.7 — Link specs → code elements by annotation comments or config mapping
- [x] 2.8 — Link code elements → tests by `tests/unit/` and `tests/integration/` path conventions
- [x] 2.9 — Query API: given a requirement ID, return linked specs, code, tests
- [x] 2.10 — Query API: given a code element, return linked requirement(s) and spec(s)
- [x] 2.11 — Graph update on file change (re-parse changed file, update affected edges)

**Deliverable**: Backend can answer "what code implements this requirement?" and "what tests cover this code?"

---

## MVP 3 — Requirements & Lean Spec Editing

**Goal**: Author and edit requirements and Lean specs within the IDE.

### Tasks

- [x] 3.1 — Editor mode switcher: Code / Lean Spec / Natural Language (different syntax highlighting per mode)
- [x] 3.2 — Requirements panel: list all requirements from `reqs/` folder, show status
- [x] 3.3 — Create/edit requirement (produces Commands like any edit)
- [x] 3.4 — Requirement status workflow: draft → approved → linked
- [x] 3.5 — Lean spec file auto-creation when requirement is approved (path from REQ ID)
- [x] 3.6 — Lean 4 syntax highlighting via tree-sitter WASM grammar
- [x] 3.7 — Lean compiler integration: on spec save, invoke `lean` subprocess to type-check
- [x] 3.8 — Display Lean compiler errors inline in editor
- [x] 3.9 — Bi-directional navigation: click requirement → opens spec file; click spec → shows requirement

**Deliverable**: You can write requirements, formalise them in Lean, and navigate between them.

---

## MVP 4 — AI Integration

**Goal**: AI assists in writing requirements, specs, and code. Custom harness tightly coupled to command system and trace graph. MCP for external tool extensibility.

### Tasks

- [x] 4.1 — Provider-agnostic AI client trait + OpenRouter, Bedrock, Mock implementations (async, retries, timeout, live model listing from provider APIs)
- [x] 4.2 — Transparency layer: token counts (input/thinking/output/cached), per-request and cumulative cost, raw request/response inspection
- [x] 4.3 — Model selection UI: settings panel, live provider query, parameter tuning per model
- [x] 4.4 — AI chat panel (right side): conversational interface with session history
- [x] 4.5 — Interaction log: all prompts/responses stored, browsable, inspectable (full context visible)
- [x] 4.6 — Context assembler: pulls from trace graph (requirement → spec → code → tests for a given target), respects token budget, ranks relevance
- [x] 4.7 — Prompt template engine: variable substitution, system/user separation, response format hints, composable partials
- [x] 4.8 — Response parser: extract code blocks by language, extract structured JSON, handle truncation/continuation
- [x] 4.9 — AI → shadow buffer → diff pipeline: AI output lands in temp buffer, user sees diff view, accept/reject per-hunk
- [x] 4.10 — Accepted hunks emitted as individual Commands (undoable in tree, attributed to AI agent)
- [x] 4.11 — Elicitation agent: user describes goal → AI suggests structured requirements (writes to `reqs/`) [button press]
- [x] 4.12 — Formalisation agent: requirement → Lean spec draft (data types + correctness theorems) [button press on Req → generate formal spec]
- [x] 4.13 — Implementation agent: Lean spec + language → code (respects existing conventions via context) [button press on spec → generate implementation]
- [x] 4.14 — Repair agent: given violation → suggests code fix or spec update, shows diff [button press, user-facing only]
- [x] 4.14a — Agent tool definitions: builtin tools (read_file, write_file, list_files, emit_command, query_trace_graph, query_code_element, list_requirements, get_symbols, run_shell, search_files) defined and injected into agent system prompts so agents can interact with the environment
- [x] 4.14b — Tool executor: executes tool calls from agents against the project (respects permission model)
- [x] 4.15 — MCP tool host: expose TraceLean internals (trace graph queries, file read/write, command emit) as MCP tools via JSON-RPC 2.0. External agents/plugins can interact via `initialize`, `tools/list`, `tools/call`.
- [x] 4.16 — MCP client: connect to user-configured external MCP servers (`.tracelean/mcp.json`). Discovers remote tools, makes them available to agents. stdio JSON-RPC transport.
- [x] 4.17 — Agent permission model: `AgentPermissions` struct controls allowed/denied tools, readable/writable path globs, shell access. Configurable per-agent via Tauri IPC.
- [x] 4.18 — Streaming response support: `ai-stream-start` and `ai-stream-token` Tauri events push partial tokens to frontend. `ai_chat_stream` command with chunked emission.

**Deliverable**: AI assists at every stage. All AI edits are reversible, diffable, and individually undoable. Full transparency on what context goes to the model and what it costs. MCP for extensibility.

---

## MVP 5 — Traceability Dashboard

**Goal**: Visual navigation of the full requirement→spec→code→test graph.

### Tasks

- [x] 5.1 — Traceability window: opens from menu bar as separate window
- [x] 5.2 — D3.js graph rendering: nodes = requirements/specs/code/tests, edges = relationships
- [x] 5.3 — Click node → navigate to file/line in editor
- [x] 5.4 — Filter by requirement, by status, by coverage
- [ ] 5.5 — Call-graph / dependency graph view (function-level dependencies)
- [x] 5.6 — Search: find requirement/spec/code by text
- [x] 5.7 — Live update: graph refreshes on file changes
- [ ] 5.8 — Agent activity feedback: show what agent is doing in chat panel with graph references
- [x] 5.9 — Color coding: green = passing, red = violation, grey = untested

**Deliverable**: Full visual traceability. Click through from requirements to code to tests.

---

## MVP 6 — Undo-Tree Visualization

**Goal**: Visual browsing and navigation of the full undo history.

### Tasks

- [x] 6.1 — Undo-tree panel: visual tree of all commands/branches
- [x] 6.2 — Click any node → jump to that state (replay commands)
- [x] 6.3 — Commit points highlighted and labelled
- [x] 6.4 — Branch labels: show where branches diverge
- [x] 6.5 — Filter: show only commit points, or only current branch
- [x] 6.6 — Per-file view: show undo-tree for a single file
- [x] 6.7 — Multi-file view: show cross-file Batch nodes
- [x] 6.8 — Diff preview: hover a node → see what changed

**Deliverable**: Time-travel through project history. Jump to any state. See all branches.

---

## MVP 8 — Remote Execution & Collaboration

**Goal**: Offload compute to remote servers. Multiple users edit simultaneously.

### Tasks

- [ ] 8.1 — Command stream serialization protocol (binary, compact)
- [ ] 8.2 — WebSocket transport: local ↔ remote command streaming
- [ ] 8.3 — Remote AppState: remote machine replays command stream → identical state
- [ ] 8.4 — Task routing: local agent dispatches heavy tasks to remote
- [ ] 8.5 — Remote Lean compilation: send commands, get SetCompileOutput back
- [ ] 8.6 — Remote test execution: send commands, get SetTestResults back
- [ ] 8.7 — Graceful degradation: buffer commands on disconnect, sync on reconnect
- [ ] 8.8 — Multi-user: bidirectional command stream between peers
- [ ] 8.9 — Conflict ordering: deterministic `(timestamp, user_id)` rule
- [ ] 8.10 — Per-user undo: undo only your own commands
- [ ] 8.11 — Presence: cursor/selection commands from other users displayed in editor
- [ ] 8.12 — Hub relay container: rebroadcast commands for NAT traversal / teams
- [ ] 8.13 — Authentication: SSH/TLS for remote, token-based for collaboration

**Deliverable**: Edit locally, compute remotely. Multiple users can work on same project in real-time.


## MVP 9 — Testing & Coverage

**Goal**: Test execution, coverage measurement, enforcement.

### Tasks

- [ ] 9.1 — Define `CoverageProvider` trait (common interface across languages)
- [ ] 9.2 — Implement CoverageProvider for Python (coverage.py)
- [ ] 9.3 — Implement CoverageProvider for Rust (tarpaulin)
- [ ] 9.5 — Integration test runner: discover tests in `tests/integration/`, run by requirement ID
- [ ] 9.6 — Unit test runner: discover tests in `tests/unit/`, run matching source structure
- [ ] 9.7 — Auto-run tests on save/commit
- [ ] 9.8 — Coverage report: which lines are covered, which are not
- [ ] 9.9 — Coverage enforcement: warn/block if integration test coverage < 100%
- [ ] 9.10 — Display coverage in editor (gutter annotations)
- [ ] 9.11 — Link coverage data into trace graph (CodeElement → coverage %)
- [ ] 9.12 — Commit points: store coverage and test results in metadata

**Deliverable**: Tests run automatically, coverage is measured and enforced, results visible in editor and graph.

---

## MVP 10 — Docker, LSP & Polish

**Goal**: Production-ready deployment, full LSP, performance.

### Tasks

- [ ] 10.1 — Docker compose: full topology (ui, backend, agent-pool, lean4-server, test-runner)
- [ ] 10.2 — Container isolation: each agent sandboxed, network restricted
- [ ] 10.3 — Volume mounts: user project persists across restarts
- [ ] 10.4 — Reproducible builds: pinned base images, lockfiles
- [ ] 10.5 — LSP server: custom methods (traceToRequirement, traceToSpec)
- [ ] 10.6 — LSP: delegate standard features to language servers (go-to-def, references, rename)
- [ ] 10.7 — Performance: profile and optimize startup scan for 10k files
- [ ] 10.8 — Performance: command log compaction (prune old branches beyond threshold)
- [ ] 10.9 — Offline mode: full editing + local spec-check when no AI / no remote
- [ ] 10.10 — Plugin system: `CommandEmitter` API for third-party plugins
- [ ] 10.11 — Encrypted keyring for API keys
- [ ] 10.12 — User onboarding: guided first-run wizard

**Deliverable**: Ship-ready product. Installs via Docker, works cross-platform, supports plugins.

