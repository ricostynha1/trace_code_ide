# Handoff: P17 — Session replay regression harness (last remaining task)

Everything else in the implementation order is **done and committed**: P34, P1+P3,
P2+P9a, P7+P5, MYTH rework, bugs.md fixes, P8 (TUI), which-key fix, P9, P10, P11, P12.
P6 and LSP are explicitly out of scope (user said do NOT implement P6 at all).

Spec reference: `tracelean/docs/SPEC-todo-p1-p7-and-roadmap.md` lines 590–599 (section
"P17 — Session replay regression harness for the agent stack"). Effort: S.

## Goal

Recorded real sessions become unit fixtures. Replaying a fixture through the tool-call
parser must reproduce the recorded parsed calls bit-for-bit — this would have caught
the P1/P2/P3 class of parser-vs-model-format drift automatically.

## What is already in place (committed or staged)

1. **`core/src/ai/bedrock.rs` — `parse_tool_calls_from_text(content, format, tools)`**
   (just added, compiles): public replay entry that runs the exact runtime parsing
   path — `parse_tool_blocks_with` with declared-dialect-first ordering and
   schema-driven coercion. Returns `(Vec<ToolCallResponse>, Vec<String /* errors */>)`.
2. **`InteractionEntry`** (`core/src/ai/log.rs`) already records everything a fixture
   needs per interaction: `request_messages`, `response_content`,
   `response_tool_calls`, `tool_schemas`, `tool_passing`
   (`NativeParam` | `SystemPromptEmbed`), `model_id`.
3. `ToolCallFormat` (MiniMaxXml | HermesJson | MistralBrackets) lives on
   `ModelConfig.tool_call_format` in `core/src/ai/provider.rs`.
4. Tests crate layout: `tracelean/tests/` is a tests-only crate (`tracelean-tests`)
   with explicit `[[test]]` targets in `tests/Cargo.toml` — add a new target there.

## Remaining work (in order)

### 1. `core/src/ai/replay.rs` (new module)

```rust
pub struct SessionFixture {
    pub name: String,
    pub model_id: String,
    pub tool_call_format: ToolCallFormat,
    pub interactions: Vec<FixtureInteraction>,
}
pub struct FixtureInteraction {
    pub request_messages: Vec<ChatMessage>,
    pub tool_schemas: Vec<ToolSchema>,
    pub response_content: Option<String>,
    pub expected_tool_calls: Vec<ToolCallResponse>,
    /// From InteractionEntry.tool_passing — replay parsing only makes sense
    /// for SystemPromptEmbed (text dialects); native structured calls never
    /// appear in response_content, skip parser assertion for those.
    pub native_tools: bool,
}
```

- `pub fn fixture_from_log(name, model, entries: &[InteractionEntry]) -> SessionFixture`
  (map fields; `native_tools = matches!(tool_passing, Some(ToolPassing::NativeParam))`;
  get `tool_call_format` from the model catalog via `model_id`, default HermesJson).
- `pub fn replay_fixture(f: &SessionFixture) -> Vec<String>` — for each interaction
  where `!native_tools && response_content.is_some()`: run
  `bedrock::parse_tool_calls_from_text(content, f.tool_call_format, Some(&i.tool_schemas))`
  and compare against `expected_tool_calls`: same length, same `function.name` in
  order, and same `function.arguments` **compared as parsed `serde_json::Value`**
  (string compare is too brittle — key order). Each mismatch pushes a readable
  string; empty vec = pass.
- Register `pub mod replay;` in `core/src/ai/mod.rs`. Unit tests in the module:
  build a synthetic fixture with a Hermes response
  (`<tool_call>{"name":"read_file","arguments":{"path":"a.txt"}}</tool_call>`) and a
  MiniMax XML one; assert replay passes, then corrupt an expected call and assert
  replay reports the mismatch.

### 2. Export from the Log tab

- IPC command `export_session_fixture(log_state, state, name: String)` in
  `gui_backend/src/ipc/ai_commands.rs`: take all current log entries
  (`InteractionLog::recent(usize::MAX)` or equivalent), build the fixture, write
  pretty JSON to `{project}/.tracelean/fixtures/sessions/{name}.json`
  (**everything TraceLean writes must go under the project's `.tracelean/` —
  explicit user requirement**). Register in `gui_backend/src/lib.rs` invoke_handler.
- Log tab UI (`react_frontend/components/AiChatPanel.tsx`, log tab header near the
  "Total cost" block): an "Export fixture" button → `invoke("export_session_fixture",
  { name: `session-${Date.now()}` })`, show the written path in a small status line.

### 3. Harness test target

- `tracelean/tests/replay_fixtures.rs` + `[[test]] name = "replay_fixtures"` in
  `tests/Cargo.toml`. It globs `tests/fixtures/sessions/*.json`
  (`std::fs::read_dir`, no new deps), deserializes `SessionFixture`, runs
  `replay_fixture`, asserts empty mismatch list per file (report file name in the
  panic message). Zero fixtures = pass (don't fail CI on empty dir).
- Seed `tests/fixtures/sessions/seed-hermes.json` and `seed-minimax.json` written by
  hand (small: one interaction each, mirroring the unit tests) so the harness
  actually executes in CI. Copying real exported fixtures from a project's
  `.tracelean/fixtures/sessions/` into `tests/fixtures/sessions/` is the intended
  developer workflow — mention it in a short comment at the top of the test.

### 4. Wrap up

- `cargo test --workspace` and `cd react_frontend && npx tsc --noEmit` must pass.
- Commit with trailer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`
  (user wants one commit per checkpoint).

## House rules (from the user, apply to all work)

- **Never run tests that hit paid APIs** — prepare commands, the user runs them.
  (`AWS_BEARER_TOKEN_BEDROCK` is exported in the shell; never read/print it.)
- Commit after each completed checkpoint so the user sees progress.
- All files TraceLean creates at runtime go to `{project}/.tracelean/`.
- Working dir for cargo: `/home/ricostynha/Desktop/trace_code_ide/tracelean`
  (repo root is one level up; commits happen from the repo root).
