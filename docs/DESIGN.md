# Design – TraceLean IDE

## 1. Architecture

Standalone desktop application built with Tauri: Rust backend + system webview rendering the UI. Small footprint, native tree-sitter bindings, full control over agent lifecycle. The webview is a rendering technology (not internet-based)—the app installs and runs locally.

## 2. Technology Stack

| Component | Choice | Why |
|-----------|--------|-----|
| Editor UI | React + TypeScript + CodeMirror 6 | Production-grade code editor. No equivalent in native Rust GUI. |
| Rendering | Tauri webview (system WebKit/WebView2) | Standalone desktop app. Lighter than Electron (no bundled Chromium). |
| Parsing (editor) | tree-sitter compiled to WASM | Runs in webview for instant keystroke-level highlighting. No IPC latency. |
| Parsing (backend) | tree-sitter native Rust | Code model extraction, graph building, deep analysis. |
| Backend | Rust (Tokio async runtime) | Performance, memory safety, native tree-sitter/petgraph integration. |
| Formal Verification | Lean 4 (subprocess / container service) | Validates specs. Models side effects via IO monad. |
| AI | OpenRouter HTTP API | Multi-model access, single integration point. |
| Persistence | Filesystem only | Source files = single source of truth. No database. |
| Graph | petgraph (Rust, in-memory) | Rebuilt from source on startup. Fast traversal for traceability. |
| Test Runners | pytest, cargo test, gcov (per-language) | Reuse existing tooling. Plugin mechanism for more. |
| Traceability UI | D3.js (in webview) | Interactive graph rendering. |

## 3. Traceability Data Model

### Graph Nodes

- **Requirement** – NL text, ID, status, version
- **Spec** – Lean 4 code, linked to ≥1 Requirement
- **CodeElement** – function/class/module identified by file:line
- **Test** – integration or unit, with coverage data

### Graph Edges

```
Requirement → Spec        (formalisation)
Spec → CodeElement        (implementation)
CodeElement → Test        (verification)
Requirement → Test        (direct integration test mapping)
```

### Rebuild on Startup

Graph is rebuilt from source files on every startup via tree-sitter. No persistence, no cache. Source files are the single source of truth.

**Performance**: Parallel parsing with Rayon (tree-sitter is per-file independent). Target < 5s cold start for 10k files. After startup, incremental updates via file watcher (only re-parse changed files).

## 4. Background Agent

Long-lived Tokio task. Listens to filesystem events (notify crate).

### On file save:

1. Parse changed file with tree-sitter → extract code model (symbols, deps).
2. Query trace graph → find associated Lean spec(s).
3. Validate conformance (property-based testing against spec).
4. Strict mode: block save on violation. Suggestion mode: notify + offer AI fix.

### Shadow State

Agent keeps last-known-good snapshot. Validation failure = file stays dirty until resolved.

## 5. Verification Strategy

### Lean Specs Model Side Effects

Lean 4's `IO` monad, `Task`, and `Mutex` primitives express:
- IO effects (file, network, database)
- Concurrency constraints (ordering, mutual exclusion, no data races)
- Liveness and deadlock-freedom properties
- Resource lifetimes (acquire/release invariants)

### Property-Based Testing (Phase 1)

- Convert Lean spec into executable properties (testable propositions).
- Run code against these properties (QuickCheck-style).
- Concurrency specs: randomised scheduling, stress tests, race detectors.
- Fast feedback. Catches most violations.

### Full Lean Verification (Phase 2 – Future)

- Generate Lean theorem: "code satisfies spec" (including effect-safety).
- Type-check with Lean compiler.
- Requires Lean-to-target-language extraction or translation-validation.

## 6. AI Workflow

Specialised agents with dedicated prompt templates:

1. **Elicitation Agent** – User goal → structured requirements.
2. **Formalisation Agent** – Requirement + context → Lean spec (iterative).
3. **Implementation Agent** – Lean spec + target language → code + unit tests.
4. **Repair Agent** – Violation detected → code fix or spec update suggestion.

Harness manages: token budgets, context window, retry logic, response parsing. User can override any AI suggestion.

## 7. Testing Architecture

```
project/
├── src/
│   ├── parser/
│   │   └── lexer.rs
│   ├── agent/
│   │   └── validator.rs
│   └── ...
├── tests/
│   ├── unit/                        ← mirrors src/ structure exactly
│   │   ├── parser/
│   │   │   └── test_lexer.rs
│   │   └── agent/
│   │       └── test_validator.rs
│   └── integration/                 ← maps to requirements by name
│       ├── req_01_requirement_mgmt.rs
│       ├── req_06_lean_spec.rs
│       ├── req_14_violation_check.rs
│       └── ...
```

- **Unit tests**: `tests/unit/` mirrors `src/` path exactly. File = `test_<source_filename>`. Documentation purpose, not coverage-counted. Findable by path alone.
- **Integration tests**: `tests/integration/` named by requirement ID. Finding tests for a requirement = look up by ID in filename.
- **Coverage tools**: Common interface trait across all languages:

```rust
trait CoverageProvider {
    fn run(&self, test_suite: &Path) -> CoverageReport;
    fn coverage_percent(&self, report: &CoverageReport) -> f64;
    fn uncovered_lines(&self, report: &CoverageReport) -> Vec<FileLine>;
}
```

Implementations: coverage.py (Python), tarpaulin (Rust), gcov (C++). All produce a uniform `CoverageReport` consumed by the traceability graph.

## 8. UI Layout

```
┌─────────────────────────────────────────────────────┐
│  Menu Bar  [File] [Edit] [View] [Traceability] [AI] │
├──────────┬──────────────────────┬───────────────────┤
│ File     │                      │ AI Chat           │
│ Tree     │   Editor Pane        │ Panel             │
│          │   (CodeMirror 6)     │ (toggleable)      │
│          │                      │                   │
│          │   Modes:             │                   │
│          │   - Code editing     │                   │
│          │   - Lean spec editing│                   │
│          │   - NL requirements  │                   │
└──────────┴──────────────────────┴───────────────────┘
```

- Left: File tree (project explorer)
- Center: Editor (CodeMirror 6 + tree-sitter WASM). Switches modes: code, Lean spec, natural-language requirements.
- Right: AI chat panel (toggle open/close)
- Traceability graph: Opened from menu bar → separate window. Interactive D3.js diagram.
- Agent feedback: While AI/agent works, progress and graph changes stream into the chat panel.
- Violations: Toast notifications with "Fix with AI" / "Edit Spec" buttons.

## 9. Command-Sourced Architecture (Core)

This is the foundational design constraint. Everything else builds on it.

### Principle

Every mutation to `AppState` is a reversible command. The application state is a function of the command history. No exceptions, no fallbacks.

```rust
impl AppState {
    /// THE ONLY WAY to change state. No other mutation path exists.
    fn apply(&mut self, cmd: Command) -> Inverse {
        let inverse = cmd.execute(self);
        self.undo_tree.push(cmd, inverse.clone());
        self.command_log.append(cmd);
        inverse
    }
}
```

### Command Types

```rust
enum Command {
    // Text editing
    Insert { file: PathBuf, offset: usize, text: String },
    Delete { file: PathBuf, offset: usize, len: usize, deleted_text: String },
    Replace { file: PathBuf, offset: usize, old_text: String, new_text: String },

    // Cursor / selection
    SetCursor { file: PathBuf, new_pos: Position, prev_pos: Position },
    SetSelection { file: PathBuf, new_range: Range, prev_range: Option<Range> },

    // File operations
    CreateFile { path: PathBuf },
    DeleteFile { path: PathBuf, content: String },  // stores content for undo
    RenameFile { from: PathBuf, to: PathBuf },

    // UI state
    OpenPanel { panel: PanelId, prev_state: PanelState },
    ClosePanel { panel: PanelId, prev_state: PanelState },
    SplitView { config: LayoutConfig, prev_config: LayoutConfig },

    // External results entering state (after side effect completes)
    SetTestResults { suite: String, new: TestResults, previous: Option<TestResults> },
    SetCompileOutput { file: PathBuf, new: CompileResult, previous: Option<CompileResult> },
    SetAIResponse { request_id: String, response: AIResult, previous: Option<AIResult> },
    SetCoverage { new: CoverageReport, previous: Option<CoverageReport> },

    // Multi-file atomic group
    Batch { commands: Vec<Command> },
}
```

Every variant carries enough data to compute its inverse. `Insert` undoes via `Delete`. `Delete` undoes via `Insert` (stored `deleted_text`). `Set*` undoes by restoring `previous`.

### Reversibility Guarantee

```rust
trait Reversible {
    fn execute(&self, state: &mut AppState) -> Inverse;
    fn inverse(&self) -> Command;  // always computable
}
```

If you can't implement `inverse()`, you can't implement the trait, you can't call `apply()`, you can't mutate state. The type system enforces 100% reversibility.

### External Side Effects

External actions (compile, test, AI call) happen **outside** the command system:

```rust
// Side effect executes in the real world — NOT through apply()
let results = run_tests_externally().await;

// Only the result recording goes through apply()
state.apply(Command::SetTestResults {
    suite: "integration".into(),
    new: results,
    previous: state.test_results.clone(),
});
```

We don't try to "un-compile" or "un-call the AI." We only undo the *recording of results* in our state.

### AI Edit Flow

AI never touches files directly:

```
AI produces edit
  → writes to shadow buffer (isolated, not part of AppState)
  → diff engine computes delta between shadow buffer and current file
  → delta converted to Command(s): Insert, Delete, Replace
  → Commands applied via state.apply() — normal pipeline
  → fully reversible, same as any user edit
  → grouped as Batch{} → single undo node
```

### Undo-Tree

The undo-tree is just a tree-shaped index over the command log:

```rust
struct UndoNode {
    id: NodeId,
    command: Command,           // the forward command
    inverse: Command,           // pre-computed inverse
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    metadata: Option<CommitPoint>,
    timestamp: Instant,
}
```

Undo = apply the inverse. Redo = apply the forward command again. Branch = new child on an old node.

### Commit Points

Named nodes with metadata:

```rust
struct CommitPoint {
    name: String,
    coverage: f64,
    spec_conformance: bool,
    test_results: TestSummary,
}
```

Created by user (explicit) or agent (after validation pass).

### Remote Sync

Remote execution = streaming the command log:

```
Local: user types → Command produced → apply locally → send Command to remote
Remote: receive Command → apply → identical state

Local: triggers "run tests" → external execution on remote
Remote: runs tests → produces result → sends result back
Local: state.apply(SetTestResults{...}) → command enters log → remote also gets it
```

Both machines replay the same command sequence → identical state. The command log IS the sync protocol.

### Persistence

Command log serialized to `.tracelean/commands/`. On startup: replay from last checkpoint to reconstruct state. Periodic full-state checkpoints every N commands for fast startup.

### Plugins

Plugins MUST produce Commands via a provided API. They receive a `CommandEmitter` handle, not a mutable `AppState` reference. Direct mutation is structurally impossible.

```rust
trait Plugin {
    fn on_event(&self, event: Event, emit: &CommandEmitter);
    // no &mut AppState — can't mutate directly
}
```

## 10. Remote Execution

Edit locally, compute remotely. Both machines share the same command log → identical state.

### Architecture

```
┌─────────────────────┐        Command stream         ┌─────────────────────────┐
│  Local Machine      │ ─────────────────────────────→ │  Remote Server(s)       │
│                     │                                │                         │
│  - Tauri IDE        │  Commands (user edits, state)  │  - Same AppState        │
│  - Full AppState    │ ←───────────────────────────── │  - lean4-server         │
│  - Local agent      │  Commands (external results)   │  - agent-pool           │
│  (fallback)         │                                │  - test-runner          │
└─────────────────────┘                                └─────────────────────────┘
```

### How it works

1. User edits locally → Command produced → applied locally → streamed to remote.
2. Remote receives Command → applies to its copy of AppState → state stays in sync.
3. Heavy tasks (Lean compile, tests) dispatched to remote for execution.
4. Remote completes external action → result packaged as Command → streamed back to local.
5. Local applies result Command → both machines identical.

### File Sync

Not needed as a separate mechanism. File content IS the AppState (reconstructed from commands). Both sides replay same commands → same files. Initial connection: send checkpoint + commands since checkpoint.

### Task Routing

Local agent decides:
- Fast/local: tree-sitter parse, graph update, cursor moves → local only.
- Heavy: Lean compilation, full test suite, coverage → dispatch to remote.
- Configurable per-task routing rules.

### Graceful Degradation

Remote unreachable → local agent runs everything (slower but functional). Command log buffers locally. On reconnect: sync buffered commands. No data loss.

### Multi-Remote

Multiple remote backends:
- Dedicated Lean server (beefy CPU).
- Dedicated test cluster.
- Commands routed by type.

## 11. Collaborative Editing

Comes for free from the command-sourced architecture. The same command stream used for remote execution also syncs state between multiple users.

### How It Works

```
┌──────────┐    command stream    ┌──────────┐
│  User A  │ ←──────────────────→ │  User B  │
│  (local  │                      │  (local  │
│  AppState)│ ←──→ hub (opt) ←──→ │  AppState)|
└──────────┘                      └──────────┘
```

1. User A types → Command produced → applied to A's AppState → streamed to B.
2. User B receives Command → applies to B's AppState → both identical.
3. Same in reverse. Bidirectional command stream.

No special collaboration protocol. No CRDTs. The command log is already the sync mechanism.

### Conflict Resolution

Concurrent edits to the same region: ordered deterministically by `(timestamp, user_id)`. All peers apply the same ordering rule → same result. If two users type at the same offset simultaneously, one goes first (by ID). Both peers compute this identically.

### Per-User Undo

Each command carries a `user_id`. Undo for User A only reverts A's commands, skipping B's. The undo-tree branches per user naturally.

### Presence

Cursor and selection commands are part of the stream. User A's `SetCursor` commands arrive at User B → displayed as a colored indicator. Zero extra work — it's just another command type.

### Offline

Commands buffer locally. On reconnect: exchange buffered commands. Apply in deterministic order. States converge.

### Connection Topology

- **Peer-to-peer**: Direct WebSocket between users (small teams).
- **Hub**: Relay server (one of the Docker containers) for larger teams or NAT traversal. Hub just rebroadcasts commands — no authority needed.

## 12. Docker Architecture

### Container Topology

```
docker compose up
├── tracelean-ui          (Tauri/web frontend)
├── tracelean-backend     (Rust API server, graph engine)
├── tracelean-agent-pool  (N isolated agent workers)
├── lean4-server          (Lean compiler service)
└── test-runner           (sandboxed test execution)
```

- Each agent in its own container (isolation, sandboxed code execution).
- Lean compiler as shared service (avoids N installations).
- User project mounted as volume (host filesystem = source of truth).
- Network isolation by default (AI API calls routed through backend only).
- Single `docker compose up` = cross-platform portability.
- Pinned base images + lockfiles = reproducible builds.
- Agent pool scales horizontally for parallel validation.
- Test runner is ephemeral: spun up per suite, torn down after.

## 13. LSP Integration (Future)

Custom LSP server exposing:
- `textDocument/traceToRequirement` – navigate from code to requirement
- `textDocument/traceToSpec` – navigate from code to Lean spec
- Standard LSP features delegated to language servers

Enables other editors (VS Code, Neovim) to use traceability features.
