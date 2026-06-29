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

**Deliverable**: Editor with fast syntax highlighting and a backend that understands code structure.

---

## MVP 2 — Traceability Graph (In-Memory)

**Goal**: Build and query the requirement→spec→code→test graph.

### Tasks

- [ ] 2.1 — Define graph node types: Requirement, Spec, CodeElement, Test
- [ ] 2.2 — Implement petgraph-based trace graph (directed edges, typed nodes)
- [ ] 2.3 — Define file format for requirements (markdown with IDs, stored in `reqs/`)
- [ ] 2.4 — Define file convention for Lean specs (path derived from requirement ID)
- [ ] 2.5 — On startup: scan `reqs/`, `specs/`, `src/`, `tests/` → build graph
- [ ] 2.6 — Link requirements → specs by file naming convention
- [ ] 2.7 — Link specs → code elements by annotation comments or config mapping
- [ ] 2.8 — Link code elements → tests by `tests/unit/` and `tests/integration/` path conventions
- [ ] 2.9 — Query API: given a requirement ID, return linked specs, code, tests
- [ ] 2.10 — Query API: given a code element, return linked requirement(s) and spec(s)
- [ ] 2.11 — Graph update on file change (re-parse changed file, update affected edges)

**Deliverable**: Backend can answer "what code implements this requirement?" and "what tests cover this code?"

---

## MVP 3 — Requirements & Lean Spec Editing

**Goal**: Author and edit requirements and Lean specs within the IDE.

### Tasks

- [ ] 3.1 — Editor mode switcher: Code / Lean Spec / Natural Language (different syntax highlighting per mode)
- [ ] 3.2 — Requirements panel: list all requirements from `reqs/` folder, show status
- [ ] 3.3 — Create/edit requirement (produces Commands like any edit)
- [ ] 3.4 — Requirement status workflow: draft → approved → linked
- [ ] 3.5 — Lean spec file auto-creation when requirement is approved (path from REQ ID)
- [ ] 3.6 — Lean 4 syntax highlighting via tree-sitter WASM grammar
- [ ] 3.7 — Lean compiler integration: on spec save, invoke `lean` subprocess to type-check
- [ ] 3.8 — Display Lean compiler errors inline in editor
- [ ] 3.9 — Bi-directional navigation: click requirement → opens spec file; click spec → shows requirement

**Deliverable**: You can write requirements, formalise them in Lean, and navigate between them.

---

## MVP 4 — Background Agent & Conformance Checking

**Goal**: Automatic validation of code against specs on every edit.

### Tasks

- [ ] 4.1 — Background agent: long-lived Tokio task, listens to file watcher events
- [ ] 4.2 — On file save: agent queries trace graph for associated Lean spec(s)
- [ ] 4.3 — Property extraction: parse Lean spec → extract testable properties (pre/post conditions)
- [ ] 4.4 — Property-based test generation: convert properties to executable tests
- [ ] 4.5 — Run generated property tests against saved code
- [ ] 4.6 — Report results: pass/fail per property, with counterexample on failure
- [ ] 4.7 — Strict mode: block save (reject Command) if validation fails
- [ ] 4.8 — Suggestion mode: allow save, show violation notification with options
- [ ] 4.9 — User setting toggle: strict vs suggestion mode
- [ ] 4.10 — Shadow state: agent keeps last-known-good version per file
- [ ] 4.11 — Commit points: auto-create after successful validation pass

**Deliverable**: Code is validated against its Lean spec on every save. Violations are caught.

---

## MVP 5 — Testing & Coverage

**Goal**: Test execution, coverage measurement, enforcement.

### Tasks

- [ ] 5.1 — Define `CoverageProvider` trait (common interface across languages)
- [ ] 5.2 — Implement CoverageProvider for Python (coverage.py)
- [ ] 5.3 — Implement CoverageProvider for Rust (tarpaulin)
- [ ] 5.4 — Implement CoverageProvider for C++ (gcov)
- [ ] 5.5 — Integration test runner: discover tests in `tests/integration/`, run by requirement ID
- [ ] 5.6 — Unit test runner: discover tests in `tests/unit/`, run matching source structure
- [ ] 5.7 — Auto-run tests on save/commit
- [ ] 5.8 — Coverage report: which lines are covered, which are not
- [ ] 5.9 — Coverage enforcement: warn/block if integration test coverage < 100%
- [ ] 5.10 — Display coverage in editor (gutter annotations)
- [ ] 5.11 — Link coverage data into trace graph (CodeElement → coverage %)
- [ ] 5.12 — Commit points: store coverage and test results in metadata

**Deliverable**: Tests run automatically, coverage is measured and enforced, results visible in editor and graph.

---

## MVP 6 — AI Integration

**Goal**: AI assists in writing requirements, specs, and code.

### Tasks

- [ ] 6.1 — OpenRouter HTTP client in Rust (async, retries, timeout handling)
- [ ] 6.2 — Model selection UI: settings panel to choose model
- [ ] 6.3 — Code harness: prompt template system, context assembly, response parsing
- [ ] 6.4 — AI chat panel (right side): conversational interface
- [ ] 6.5 — Elicitation agent: user describes goal → AI suggests structured requirements
- [ ] 6.6 — Formalisation agent: given requirement → AI produces Lean spec draft
- [ ] 6.7 — Implementation agent: given Lean spec + language → AI produces code
- [ ] 6.8 — Repair agent: given violation → AI suggests code fix or spec update
- [ ] 6.9 — AI → shadow buffer → diff → Commands pipeline (AI never writes directly)
- [ ] 6.10 — AI edits as Batch command (single undo node for entire AI action)
- [ ] 6.11 — AI interaction log: all prompts/responses stored, browsable
- [ ] 6.12 — Undo AI action: single Ctrl+Z reverts entire AI edit

**Deliverable**: AI assists at every stage. All AI edits are reversible. User stays in control.

---

## MVP 7 — Traceability Dashboard

**Goal**: Visual navigation of the full requirement→spec→code→test graph.

### Tasks

- [ ] 7.1 — Traceability window: opens from menu bar as separate window
- [ ] 7.2 — D3.js graph rendering: nodes = requirements/specs/code/tests, edges = relationships
- [ ] 7.3 — Click node → navigate to file/line in editor
- [ ] 7.4 — Filter by requirement, by status, by coverage
- [ ] 7.5 — Call-graph / dependency graph view (function-level dependencies)
- [ ] 7.6 — Search: find requirement/spec/code by text
- [ ] 7.7 — Live update: graph refreshes on file changes
- [ ] 7.8 — Agent activity feedback: show what agent is doing in chat panel with graph references
- [ ] 7.9 — Color coding: green = passing, red = violation, grey = untested

**Deliverable**: Full visual traceability. Click through from requirements to code to tests.

---

## MVP 8 — Undo-Tree Visualization

**Goal**: Visual browsing and navigation of the full undo history.

### Tasks

- [ ] 8.1 — Undo-tree panel: visual tree of all commands/branches
- [ ] 8.2 — Click any node → jump to that state (replay commands)
- [ ] 8.3 — Commit points highlighted and labelled
- [ ] 8.4 — Branch labels: show where branches diverge
- [ ] 8.5 — Filter: show only commit points, or only current branch
- [ ] 8.6 — Per-file view: show undo-tree for a single file
- [ ] 8.7 — Multi-file view: show cross-file Batch nodes
- [ ] 8.8 — Diff preview: hover a node → see what changed

**Deliverable**: Time-travel through project history. Jump to any state. See all branches.

---

## MVP 9 — Remote Execution & Collaboration

**Goal**: Offload compute to remote servers. Multiple users edit simultaneously.

### Tasks

- [ ] 9.1 — Command stream serialization protocol (binary, compact)
- [ ] 9.2 — WebSocket transport: local ↔ remote command streaming
- [ ] 9.3 — Remote AppState: remote machine replays command stream → identical state
- [ ] 9.4 — Task routing: local agent dispatches heavy tasks to remote
- [ ] 9.5 — Remote Lean compilation: send commands, get SetCompileOutput back
- [ ] 9.6 — Remote test execution: send commands, get SetTestResults back
- [ ] 9.7 — Graceful degradation: buffer commands on disconnect, sync on reconnect
- [ ] 9.8 — Multi-user: bidirectional command stream between peers
- [ ] 9.9 — Conflict ordering: deterministic `(timestamp, user_id)` rule
- [ ] 9.10 — Per-user undo: undo only your own commands
- [ ] 9.11 — Presence: cursor/selection commands from other users displayed in editor
- [ ] 9.12 — Hub relay container: rebroadcast commands for NAT traversal / teams
- [ ] 9.13 — Authentication: SSH/TLS for remote, token-based for collaboration

**Deliverable**: Edit locally, compute remotely. Multiple users can work on same project in real-time.

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

---

## Dependency Graph

```
MVP 0 (Command Shell)
  └→ MVP 1 (Tree-sitter)
       └→ MVP 2 (Trace Graph)
            ├→ MVP 3 (Requirements & Specs)
            │    └→ MVP 4 (Background Agent)
            │         └→ MVP 5 (Testing & Coverage)
            │              └→ MVP 7 (Traceability Dashboard)
            └→ MVP 6 (AI Integration)
  └→ MVP 8 (Undo-Tree Visualization)
  └→ MVP 9 (Remote & Collaboration)
  └→ MVP 10 (Docker, LSP, Polish)
```

MVP 0 is the hard prerequisite for everything. MVP 1 and 2 are sequential foundations. After that, MVPs 3-7 form the core feature chain. MVPs 8, 9, 10 can be parallelized once MVP 0 is solid.
