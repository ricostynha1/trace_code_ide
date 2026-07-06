# Tasks — Agent Harness Correctness

- [x] 1. Add `save_eff` fn (private, `tool_executor.rs`): reads content via `state.get_content`, writes to disk. (Req 1.1)
- [x] 2. Refactor `execute_write_file`: call `save_eff` right after `Replace` apply; remove old inline `std::fs::write` on local `content` var. On write err: `success:false`, keep applied Command in log. (Req 1.1, 1.2, 1.3, 1.4)
- [x] 3. Success-path unit test per tool, all 10: `read_file`, `write_file`, `list_files`, `emit_command`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `run_shell`, `search_files`. (Req 2)
- [x] 4. Permission-denied test per gated tool: `read_file`, `write_file`, `emit_command`, `run_shell`. Assert `success:false`, content mentions denial, no state mutation. (Req 3.1, 3.2)
- [x] 5. Error-path test per ungated tool: `list_files`, `query_trace_graph`, `query_code_element`, `list_requirements`, `get_symbols`, `search_files`. (Req 3.3)
- [x] 6. Error-path test per tool (all 10), missing required arg → `success:false`, no panic. (Req 4)
- [x] 7. Mock-provider-free `ToolCall` sequence tests: single `write_file`/`emit_command` call + `undo()` restores prior buffer. (Req 5.1, 5.2)
- [X] 8. Multi-call undo test: 2+ AI tool calls, `undo()` per call in reverse order, buffer matches pre-first-call state. (Req 5.3)
- [X] 9. Property test 1 (write_file buffer/disk parity): random paths + UTF-8 content, ≥100 iterations.
- [X] 10. Property test 2 (permission denial for gated tools): random `AgentPermissions` denying each gated tool, ≥100 iterations.
- [X] 11. Property test 3 (missing-arg never panics): per tool, drop each required arg one at a time, ≥100 iterations.
- [X] 12. Property test 4 (undo restores prior state): random initial content + random 1-5 call sequence, undo all reverse, ≥100 iterations.
- [X] 13. Harden `Dockerfile` runtime stage: create non-root user, `chown` app/toolchain dirs, `USER <name>` before `ENTRYPOINT`. (Req 6.1, 6.3)
- [X] 14. Harden `Dockerfile.dev`: create non-root user, `chown` `/app` + elan path, `USER <name>` before default `CMD`. (Req 6.2, 6.3)
- [X] 15. Static tests asserting each Dockerfile has `useradd`/`adduser`, `chown` on relevant dirs, `USER` before `ENTRYPOINT`/`CMD`.
