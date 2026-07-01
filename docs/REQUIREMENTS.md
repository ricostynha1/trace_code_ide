# Requirements – TraceLean IDE

## 0. Core Architectural Requirement: Command-Sourced State

| ID | Requirement |
|----|-------------|
| REQ-00 | **Every mutation to application state is a reversible command. No exceptions.** |
| REQ-00a | All user actions (keystrokes, clicks, selections, menu actions) produce Command objects. |
| REQ-00b | All agent/AI actions produce Commands — AI never writes files directly, only through the command pipeline. |
| REQ-00c | Every Command must return its inverse. If an inverse cannot be computed, the action cannot mutate state. |
| REQ-00d | External side effects (compile, test, AI API call) execute outside the command system. Only their *results* enter state as reversible commands (e.g., `SetTestResults{new, previous}`). |
| REQ-00e | The undo-tree, remote sync, and persistence are all projections of the same command log. |
| REQ-00f | `AppState` has exactly one mutation path: `apply(Command) -> Inverse`. No other state mutation exists. The type system enforces this. |
| REQ-00g | Plugins must produce Commands. Direct state mutation by plugins is forbidden. |
| REQ-00h | AI edits flow: AI → shadow buffer → diff → Commands applied through normal pipeline. Fully reversible. |
| REQ-00i | Clipboard paste from external = `Insert{text}` command. Reversible by `Delete`. |
| REQ-00j | 100% reversibility guaranteed by construction. No fallbacks, no escape hatches. |

This is the foundational design constraint. The entire application is built around this invariant. Every other feature (undo-tree, remote execution, traceability, agent actions) depends on it.

## 1. Requirement Management

| ID | Requirement |
|----|-------------|
| REQ-01 | Users and AI agents collaboratively author natural-language requirements. |
| REQ-02 | Each requirement has a unique ID and version history. |
| REQ-03 | Requirements support hierarchy (parent/child). |
| REQ-04 | Requirements have status: draft, approved, linked to formal spec. |
| REQ-05 | Any requirement change triggers background agent re-evaluation of impacted specs and code. |

## 2. Formal Specification (Lean 4)

| ID | Requirement |
|----|-------------|
| REQ-06 | Each requirement has a corresponding Lean 4 formal specification. |
| REQ-07 | Bi-directional mapping between requirement and Lean spec. |
| REQ-08 | Spec edits are tracked and linked back to originating requirement(s). |
| REQ-09 | Lean specs must be type-correct and syntactically valid (Lean compiler check). |
| REQ-10 | Multiple Lean specs per requirement allowed for decomposition. |
| REQ-11 | Lean spec file location is deducible from the natural-language requirement it formalises (predictable path for easy search). |

## 3. Code Implementation & Editing

| ID | Requirement |
|----|-------------|
| REQ-12 | Agent implements formal spec in one or more target languages (initial: Python, Rust, C++). |
| REQ-13 | Syntax highlighting, navigation, and completion via tree-sitter for all supported languages. |
| REQ-14 | On file edit, background agent checks code against associated Lean spec. |
| REQ-15 | On violation: (a) reject edit with error, or (b) prompt user to update spec and propagate to NL requirements. |
| REQ-16 | User setting to choose strict (reject) vs suggestion (prompt) mode. |
| REQ-17 | Multi-file projects with dependency and call graph computed on the fly. |

## 4. Traceability View

| ID | Requirement |
|----|-------------|
| REQ-18 | Traceability Dashboard shows all requirements, specs, and code artifacts. |
| REQ-19 | Click requirement → navigate to Lean spec → highlight relevant code and corresponding tests. |
| REQ-20 | Dashboard shows call-graph / dependency graph of entire project linked to requirements. |
| REQ-21 | Live link: changes in code/spec reflect instantly in traceability view. |

## 5. Testing & Coverage

| ID | Requirement |
|----|-------------|
| REQ-22 | Each requirement has at least one integration test. |
| REQ-23 | 100% integration test coverage of all requirement-implementing code (enforced). Test path maps directly to requirement ID. |
| REQ-24 | Unit tests mirror source structure: `tests/unit/<same_path>/test_<filename>.<ext>`. |
| REQ-25 | Unit tests are documentation-first; displayed alongside code in IDE. |
| REQ-26 | Tests run automatically on save/commit; coverage reported. |
| REQ-27 | Coverage tools expose a common interface trait regardless of language. |

## 6. AI Integration (OpenRouter)

| ID | Requirement |
|----|-------------|
| REQ-28 | Integrate with OpenRouter for LLM access. |
| REQ-29 | Users select/configure model via settings. |
| REQ-30 | AI assists: requirement generation, Lean formalisation, code implementation, violation repair. |
| REQ-31 | All AI interactions logged and undoable. |
| REQ-32 | Code harness manages API calls, prompt engineering, context, response parsing. |
| REQ-32a | Agents are provided tool definitions (read_file, write_file, list_files, emit_command, query_trace_graph, search_files, run_shell, get_symbols) in their system prompt so they can interact with the project environment. |
| REQ-32b | A tool executor runs tool calls from agent responses against the project, respecting permissions. |
| REQ-32c | MCP tool host exposes TraceLean internals via JSON-RPC 2.0 so external agents/plugins can interact. |
| REQ-32d | MCP client connects to user-configured external MCP servers, making their tools available to agents. |
| REQ-32e | Agent permission model controls which files/commands each agent can touch, configurable per-agent. |
| REQ-32f | Streaming response support: tokens emitted to UI as they arrive for real-time display. |

## 7. Background Agent

| ID | Requirement |
|----|-------------|
| REQ-33 | Persistent background agent monitors file changes, runs validation async. |
| REQ-34 | Agent maintains in-memory graph of requirements↔specs↔code↔tests (rebuilt from source on startup). |
| REQ-35 | On violation in strict mode: block save until resolved. |
| REQ-36 | Agent can call AI to propose fixes. |

## 8. Tree-sitter & LSP

| ID | Requirement |
|----|-------------|
| REQ-37 | Tree-sitter (WASM) used for all parsing, highlighting, structural queries in the editor. |
| REQ-38 | Tree-sitter (native Rust) used in backend for code model extraction and graph building. |
| REQ-39 | LSP support for go-to-definition, find-references, rename (later phase). |
| REQ-40 | Dynamic addition of new language grammars supported. |

## 9. Undo-Tree (Multi-file)

| ID | Requirement |
|----|-------------|
| REQ-41 | The system maintains a tree-structured undo history (not linear) per file, like Emacs undo-tree. |
| REQ-42 | Undo history is multi-file aware: a single logical change spanning multiple files is one undo node. |
| REQ-43 | Users can visually browse and navigate the undo-tree (branch, jump to any past state). |
| REQ-44 | Commit points: named snapshots saved explicitly by user or automatically by agent after validation passes. |
| REQ-45 | Commit points store metadata: test coverage, spec conformance status, timestamp. |
| REQ-46 | Agent actions (AI-generated edits) create distinct undo nodes so they can be reverted atomically. |
| REQ-47 | Undo-tree persists across sessions (survives app restart). |
| REQ-48 | Only commands (operations) are stored, not full file snapshots. State reconstructed by replaying commands. |

## 10. Remote Execution

| ID | Requirement |
|----|-------------|
| REQ-49 | Users edit locally but can offload agent work (compilation, spec-checking, test execution) to remote servers. |
| REQ-50 | Remote servers are additional Docker hosts running the same container topology. |
| REQ-51 | The local IDE maintains full editing capability; remote is for compute-heavy tasks only. |
| REQ-52 | File sync between local and remote is automatic and transparent (rsync-like or filesystem mount). |
| REQ-53 | Multiple remote backends can be configured (e.g., one for Lean compilation, one for test execution). |
| REQ-54 | Connection loss to remote degrades gracefully: local agent takes over (slower but functional). |
| REQ-55 | Remote execution results (test output, coverage, spec-check) stream back to the local IDE in real-time. |
| REQ-56 | Authentication and encryption for all remote connections (SSH/TLS). |

## 11. Collaborative Editing

| ID | Requirement |
|----|-------------|
| REQ-57 | Multiple users can edit the same project simultaneously — enabled by the command-sourced architecture (each user streams commands to all peers). |
| REQ-58 | No special collaboration layer needed: the same command stream used for remote execution also syncs state between users. |
| REQ-59 | Each user has a full local AppState. Commands from other users are applied in order → all peers converge to identical state. |
| REQ-60 | Presence awareness: see other users' cursors and selections (cursor/selection commands are part of the stream). |
| REQ-61 | Per-user undo: each user can undo their own commands without affecting others' work. |
| REQ-62 | Conflict resolution: concurrent edits to the same region are ordered deterministically (by timestamp + user ID). |
| REQ-63 | Works offline: commands buffer locally, sync on reconnect. |

## 12. Docker-Based Execution

| ID | Requirement |
|----|-------------|
| REQ-64 | The entire system runs inside Docker containers. |
| REQ-65 | A single `docker compose up` brings up a fully functional system on any host with Docker. |
| REQ-66 | Agent processes run in isolated containers (sandboxed, no cross-contamination). |
| REQ-67 | User projects mounted as volumes; no data loss on container restart. |
| REQ-68 | Container images are reproducible (pinned deps, deterministic builds). |
| REQ-69 | Multiple agent containers can run in parallel for concurrent validation. |
| REQ-70 | Container networking isolates agents from host network unless explicitly allowed. |

## 13. Non-functional Requirements

| ID | Requirement |
|----|-------------|
| NFR-01 | Traceability updates and spec-check < 2s for projects < 100 files. |
| NFR-02 | Handle projects up to 10k files with < 4 GB memory. |
| NFR-03 | API keys stored encrypted; no code sent without user consent. |
| NFR-04 | Intuitive UI with clear errors and guided workflows. |
| NFR-05 | Extensible: new languages, AI providers, verification backends. |
| NFR-06 | Graceful AI timeout handling; manual overrides always available. |
| NFR-07 | Offline mode: manual editing and local spec-check work without AI. |
| NFR-08 | Cross-platform: runs on Linux, macOS, Windows (via Docker). |
