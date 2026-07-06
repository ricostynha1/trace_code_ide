# Requirements Document

## Introduction

Audit and fix correctness gaps in the agent tool-execution harness (`tool_executor.rs`, `commands.rs`, `state.rs`) plus defense-in-depth Docker hardening. Focus: `execute_write_file`'s disk write is currently an inline, ad-hoc `std::fs::write` call sitting outside `AppState::apply`, breaking the "apply() is the only mutation path" invariant documented in `state.rs`. Add test coverage for all 10 tools in `tool_executor.rs` (success / permission-denied / error paths) and prove AI-driven edits produce reversible `Command`s whose `undo()` restores prior state. Harden both Dockerfiles to run as non-root.

## Glossary

- **Tool_Executor**: The `execute_tool` function and its 10 per-tool handlers in `tool_executor.rs` (`read_file`, `write_file`, `list_files`, `emit_command`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `run_shell`, `search_files`).
- **AppState**: The struct in `state.rs` holding buffers, undo tree, and command log, whose only mutation path is `apply(Command)`.
- **Command**: The enum in `commands.rs` representing a state mutation and its inverse.
- **Disk_Write**: The act of persisting file content to the filesystem via `std::fs::write` or equivalent.
- **AgentPermissions**: The struct in `tool_executor.rs` controlling per-agent tool/path/shell access.
- **Mock_Provider**: A test double standing in for an AI provider, used to construct `ToolCall` sequences without a real network call.
- **Container_Runtime**: The `Dockerfile` and `Dockerfile.dev` images used to build/run TraceLean.

## Requirements

### Requirement 1: write_file disk persistence is a consequence of apply()

**User Story:** As a maintainer, I want `write_file`'s disk write to happen as a byproduct of `AppState::apply`, so that the "apply() is the only mutation path" invariant holds and undo/redo stay consistent with disk state.

#### Acceptance Criteria

1. THE Tool_Executor SHALL trigger the Disk_Write for `write_file` from within, or as a direct synchronous consequence of, the `AppState::apply` call rather than via a separate inline `std::fs::write` statement in `execute_write_file`.
2. WHEN `execute_write_file` runs on an existing file, THE Tool_Executor SHALL produce identical in-memory buffer content and on-disk file content after the call completes.
3. IF the Disk_Write fails after `apply` has recorded the command, THEN THE Tool_Executor SHALL return a `ToolResult` with `success: false` and SHALL leave the recorded `Command` available for a caller-initiated undo.
4. WHEN `state.undo()` is called after a successful `write_file` tool call, THE AppState SHALL restore the in-memory buffer to its pre-write content.

### Requirement 2: Full tool coverage for success paths

**User Story:** As a maintainer, I want a test for each of the 10 tools' success path, so that regressions in normal operation are caught.

#### Acceptance Criteria

1. THE Tool_Executor test suite SHALL contain at least one passing-case test for each of: `read_file`, `write_file`, `list_files`, `emit_command`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `run_shell`, `search_files`.
2. WHEN a success-path test invokes a tool with valid arguments and full `AgentPermissions`, THE test SHALL assert `ToolResult.success` is `true`.

### Requirement 3: Full tool coverage for permission-denied paths

**User Story:** As a maintainer, I want a test proving each permission-gated tool rejects disallowed agents, so that the permission model is enforced.

#### Acceptance Criteria

1. THE Tool_Executor test suite SHALL contain at least one permission-denied test for each of the tools gated by `AgentPermissions`: `read_file`, `write_file`, `emit_command`, `run_shell`.
2. WHEN a permission-denied test invokes a gated tool with `AgentPermissions` that deny it, THE test SHALL assert `ToolResult.success` is `false` and `ToolResult.content` mentions permission denial.
3. IF a tool has no permission gate (e.g. `list_files`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `search_files`), THEN THE Tool_Executor test suite SHALL contain an error-path test in place of a permission-denied test.

### Requirement 4: Full tool coverage for error paths

**User Story:** As a maintainer, I want a test proving each tool handles invalid input or environment errors gracefully, so that malformed AI tool calls do not panic the harness.

#### Acceptance Criteria

1. THE Tool_Executor test suite SHALL contain at least one error-path test for each of the 10 tools, covering missing required arguments or an underlying I/O/parse failure.
2. WHEN an error-path test invokes a tool with a missing required argument, THE test SHALL assert `ToolResult.success` is `false` without a panic.

### Requirement 5: AI edits via mock produce reversible commands

**User Story:** As a maintainer, I want tests proving that AI-issued tool calls (via a Mock_Provider ToolCall sequence, no real provider) result in Commands whose inverse restores prior state, so that undo is trustworthy for AI-driven edits.

#### Acceptance Criteria

1. THE test suite SHALL contain a test that issues a `write_file` `ToolCall` through Tool_Executor without any AI provider dependency, using only constructed `ToolCall` values.
2. WHEN a `write_file` or `emit_command` `ToolCall` is executed and then `state.undo()` is invoked, THE AppState SHALL restore the buffer content to the value present immediately before the tool call.
3. WHEN a sequence of two or more AI tool calls is executed and then `state.undo()` is invoked once per call in reverse order, THE AppState SHALL restore the buffer content to its state before the first tool call.

### Requirement 6: Container runtime hardening

**User Story:** As a maintainer, I want the Docker images to run as non-root, so that a container compromise or agent-triggered `run_shell` escape has reduced host/container privilege.

#### Acceptance Criteria

1. THE Container_Runtime `Dockerfile` runtime stage SHALL create a non-root user and SHALL set `USER` to that non-root user before the `ENTRYPOINT` instruction.
2. THE Container_Runtime `Dockerfile.dev` SHALL create a non-root user and SHALL set `USER` to that non-root user before the default `CMD` instruction.
3. WHERE the non-root user requires write access to application directories (e.g. project mount, elan toolchain install path), THE Container_Runtime SHALL grant ownership of those directories to the non-root user.
