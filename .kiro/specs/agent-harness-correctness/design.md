# Design Document

## Overview

Fix the `write_file` invariant violation (disk write outside `apply`), add full tool test coverage (success/permission-denied-or-error/error paths for all 10 tools), add mock-driven AI-edit undo tests, and harden both Dockerfiles to run as non-root. No new public API surface — changes are internal to `tool_executor.rs`, `commands.rs`, `state.rs`, test files, and the two `Dockerfile*`.

## Architecture

Current flow for `write_file`:

```
execute_write_file
  -> state.apply(CreateFile)      [if new]
  -> state.load_file(...)
  -> state.apply(Replace)         [mutates buffer only]
  -> std::fs::write(&full_path, &content)  [disk write, OUTSIDE apply, ad-hoc,
                                             writes a locally-held `content` var,
                                             not what's actually in AppState]
```

Problem with reading disk-write content from a local var: `content` and the buffer written by `apply` are two separate copies that happen to be equal today. Nothing enforces they stay equal if `execute_write_file` is edited later (e.g. an extra transform applied to the buffer post-`apply` but not to `content`). Property 1 (buffer/disk parity) would then hold by coincidence, not by construction.

**Design choice: introduce `save_eff`, an explicitly-effectful function that reads its content FROM `AppState` (`state.get_content`), not from a caller-supplied string.** This makes parity structural: whatever is in the buffer after `apply` is exactly what gets written to disk, because there is only one read of the content, and it happens inside `save_eff` after `apply` has already run.

Naming: `_eff` suffix marks functions with a filesystem/process side effect, to visually separate them from the pure `ToolResult`-shaping code around them. Scope: only `save_eff` (new) uses this convention in this fix. The other 9 tool handlers are unchanged — no repo-wide rename.

Target flow:

```
execute_write_file
  -> state.apply(CreateFile)      [if new]
  -> state.load_file(...)
  -> state.apply(Replace)         [mutates buffer — the only mutation path]
  -> save_eff(state, project_root, &rel_path)   [reads state.get_content(&rel_path), writes it to disk]
```

```rust
/// Writes `rel_path`'s current buffer content (read from `state`, not a
/// caller-supplied copy) to disk under `project_root`. Side-effecting;
/// name carries `_eff` suffix by convention. Buffer is the sole source of
/// truth for what gets persisted, so buffer/disk parity holds by
/// construction, not by two variables coincidentally matching.
fn save_eff(state: &AppState, project_root: &Path, rel_path: &Path) -> std::io::Result<()> {
    let content = state.get_content(&rel_path.to_path_buf()).unwrap_or_default();
    std::fs::write(project_root.join(rel_path), content)
}

fn execute_write_file(...) -> ToolResult {
    // ... permission check, existing/new file setup ...

    // Apply to in-memory state — the only mutation path for buffers/undo.
    if !file_exists {
        state.apply(Command::CreateFile { path: rel_path.clone() });
    }
    state.load_file(rel_path.clone(), existing.clone());
    state.apply(Command::Replace { file: rel_path.clone(), offset: 0, old_text: existing, new_text: content.clone() });

    // Disk write is a direct synchronous consequence of the apply() above,
    // adjacent in the same function body. On failure the Command remains
    // recorded/undoable; only the tool call itself is reported as failed (Req 1.3).
    match save_eff(state, project_root, &rel_path) {
        Ok(_) => ToolResult { success: true, content: format!("Wrote {} bytes to '{}'.", content.len(), path), data: None },
        Err(e) => ToolResult { success: false, content: format!("Write error: {}", e), data: None },
    }
}
```

Rejected alternative (moving disk I/O into `AppState::apply`, e.g. `apply_and_persist`): `AppState` is deliberately I/O-free (used in tests without a filesystem, per `state.rs`'s doc comment). Coupling it to the filesystem would break that separation and every existing state test. Not pursued.

## Components and Interfaces

No new public API surface. Modified/added, scoped to `tool_executor.rs`:

- `tool_executor.rs::save_eff` (new, private) — `fn save_eff(state: &AppState, project_root: &Path, rel_path: &Path) -> std::io::Result<()>`. Reads content via `state.get_content`, writes to disk. Only caller is `execute_write_file`.
- `tool_executor.rs::execute_write_file` — reordered to call `save_eff` immediately after the `Replace` apply; no signature change.

**Shared-helper evaluation (point 4):** `service.rs::save_file` has a near-identical `state.get_content` → `std::fs::write` sequence, but wraps it with symbol re-parsing and command-log persistence — those stay separate regardless. The candidate for sharing is only the one-line read+write. Decision: **do not share `save_eff` with `service.rs`.** A shared helper would need to live outside `state.rs` (which stays I/O-free) and outside `tool_executor.rs` (private today), forcing a new shared module or a `pub(crate)` promotion for a single `std::fs::write` line — more indirection than the duplication it removes. If a third caller needs the same pattern later, revisit. `service.rs::save_file` is unchanged by this fix.

- Test modules (new): `tracelean/tests/unit/test_tool_executor.rs` (or extend existing test files if present) covering all 10 tools x {success, permission-denied-or-error, error}.
- Test module (new/extended): mock-provider-free `ToolCall` sequence tests for undo correctness, likely alongside `test_tool_executor.rs` or `test_commands.rs`/`test_state.rs`.
- `Dockerfile`, `Dockerfile.dev` — add non-root user creation, ownership grants, `USER` directive.

No changes to `commands.rs` or `state.rs` logic — both already satisfy the undo/inverse contract; only test coverage is added there indirectly through `tool_executor` tests.

## Data Models

No new data structures. Existing `ToolCall`, `ToolResult`, `Command`, `AgentPermissions`, `AppState` are used as-is.

## Docker Hardening Design

**`Dockerfile` (runtime stage):**
- After installing packages and before `ENTRYPOINT`, add:
  ```dockerfile
  RUN useradd -m -u 10001 tracelean \
      && mkdir -p /home/tracelean/.elan \
      && chown -R tracelean:tracelean /home/tracelean /usr/local/bin/tracelean /usr/local/bin/ui_settings
  ```
- Move elan install to the non-root user's home (`ENV PATH="/home/tracelean/.elan/bin:${PATH}"`) or `chown -R` the existing `/root/.elan` to the new user and adjust `PATH`/`HOME`. Simpler: create user first, then run elan install as that user (`USER tracelean` before the elan `RUN`, or `su - tracelean -c '...'`), avoiding a `/root`-owned toolchain entirely.
- Add `USER tracelean` immediately before `ENTRYPOINT ["tracelean"]`.

**`Dockerfile.dev`:**
- After installing packages/toolchain, add a non-root user, `chown -R` `/app` (the mounted project dir) and the elan install path to that user.
- Add `USER <user>` before the default `CMD ["bash"]`.
- Since this is a dev container likely bind-mounting a host directory, note (in a comment) that host UID/GID mismatches can cause permission issues; a fixed UID (e.g. 10001) is used for predictability, matching common devcontainer convention.

These are example-based/static checks (Requirement 6), verified by asserting the Dockerfile text contains `USER <name>` before `ENTRYPOINT`/`CMD`, and that a `chown` targeting the relevant directories exists — not property tests.

## Error Handling

- `execute_write_file` disk-write failure: `save_eff` returns `Err`, mapped to `ToolResult{success:false,...}` while leaving the already-applied `Command` in `state`'s command log/undo tree untouched (Req 1.3). No rollback of the in-memory `apply` is performed — undo remains the caller's explicit mechanism to revert, consistent with existing undo semantics.
- `save_eff` itself does no error handling beyond propagating `std::io::Result` — it's a thin, effectful read-then-write; all `ToolResult` shaping stays in `execute_write_file`.
- All 10 tool handlers already return `ToolResult{success:false,...}` on missing args / I/O errors rather than panicking (`unwrap`-free on user input paths, verified during test-writing). Any handler found to `unwrap()`/`expect()` on a fallible user-controlled value during test-writing will be patched to return an error `ToolResult` instead — this is a byproduct fix, not a new architecture.

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system-essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: write_file buffer/disk parity

For any project root, relative file path, and any string content, after `execute_write_file` returns with `success: true`, the in-memory buffer content for that path in `AppState` SHALL equal the on-disk file content read back from disk. (Holds by construction: `save_eff` writes `state.get_content(...)` directly, not a separately-held copy.)

**Validates: Requirements 1.2**

### Property 2: Permission denial for gated tools

For any of the gated tools (`read_file`, `write_file`, `emit_command`, `run_shell`) and any `AgentPermissions` configuration that denies that tool (via `denied_tools` or `allow_shell: false`), calling `execute_tool` with that permissions value SHALL return a `ToolResult` with `success: false` and `content` containing a permission-denial message, and SHALL NOT mutate `AppState`.

**Validates: Requirements 3.1, 3.2**

### Property 3: Missing required argument never panics

For any of the 10 tools and any `ToolCall` missing one of that tool's required arguments, `execute_tool` SHALL return without panicking and SHALL return a `ToolResult` with `success: false`.

**Validates: Requirements 4.1, 4.2**

### Property 4: Undo of a tool-call sequence restores prior state

For any sequence of one or more `write_file`/`emit_command` `ToolCall`s applied in order to an `AppState` starting from some buffer content C, invoking `state.undo()` once per call in reverse order SHALL restore the buffer content to C.

**Validates: Requirements 1.4, 5.2, 5.3**

## Testing Strategy

**Unit/example tests (per Requirements 2, 3.3, 6):**
- One success-path unit test per tool (10 tests): `read_file`, `write_file`, `list_files`, `emit_command`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `run_shell`, `search_files` — each asserts `ToolResult.success == true` with valid args + full permissions.
- One error-path unit test per ungated tool (6 tests: `list_files`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `search_files`) per Req 3.3, covering missing/invalid args.
- One example test for the disk-write-failure branch of `execute_write_file` (Req 1.3), e.g. writing to a path under a read-only or non-existent-and-uncreatable directory, asserting `success:false` and that the command log still contains the applied command.
- Docker: static assertions (string checks on file contents, or a small script/test) that each Dockerfile has a `useradd`/`adduser` line, a `chown` covering app/toolchain dirs, and `USER <name>` before `ENTRYPOINT`/`CMD`.

**Property tests (per Correctness Properties above, min. 100 iterations each):**
- Property 1: generate random file paths (within a temp project root) and random UTF-8-safe content; run `execute_write_file` (which calls `save_eff`); compare buffer vs disk.
- Property 2: generate arbitrary `AgentPermissions` that deny each gated tool in turn; assert denial and no state mutation (compare `state.command_log().len()` before/after).
- Property 3: for each tool, generate `ToolCall`s with each required argument removed one at a time (and with wrong-typed values); assert no panic (via `std::panic::catch_unwind` or simply that the call returns) and `success:false`.
- Property 4: generate random initial buffer content and a random sequence (length 1-5) of `write_file`/`emit_command` calls with random content/commands; apply all, undo all in reverse; compare final buffer to initial.

Tag format: **Feature: agent-harness-correctness, Property {number}: {property_text}**
