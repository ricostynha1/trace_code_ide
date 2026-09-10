
## 6.8 — ACP / openCode: make ACP the single agent loop

**Where we are today (answering the original question): no, the AI chat agent is not
wired through ACP — it's a separate, native loop, and the ACP module is a second,
parallel implementation.** Traced in code:

- The chat panel's two paths both call the **native runtime**:
  `ai_chat_session` → `AiService::chat_turn` → `run_agent_turn_session`
  (`core/src/agent/runtime.rs`), and `ai_chat_stream` → `run_agent_turn_session` directly
  (`mcp_commands.rs:259`). Grepping `core/src/agent/` for `acp`/`Acp` returns **nothing** —
  the chat loop has zero ACP involvement.
- The ACP module (`core/src/acp/`) is used by only two things, *neither of which is the
  chat panel*: the standalone `tracelean-agent` binary (exposes an agent over ACP stdio
  for external editors), and `AcpClientManager` (lets TraceLean connect *out* to external
  agents — Tauri commands exist, but no frontend calls them).
- Critically, `core/src/acp/builtin_agent.rs` is a **separate reimplementation** of the
  turn loop — it does *not* call `run_agent_turn_session`; it has its own compaction
  constants and its own message handling. So today there are **two independent agent-loop
  implementations** that can drift apart.

### Decision (made): do both (i) and (ii), with (ii) = option (b)

- **(i) Connect *to* openCode (TraceLean as ACP client).** Fully unblocked by the existing
  backend — openCode already speaks ACP (`zed.dev/acp/agent/opencode`), and
  `acp_connect_agent` already accepts `{name, command, args}`. This is a frontend
  `AcpPanel.tsx` + a manual interop smoke test against a real `opencode` binary. No new
  protocol code.
- **(ii) ACP becomes the *only* agent loop.** We collapse to a single loop and standardize
  on ACP as the internal communication protocol even for our own agent, so there is exactly
  one loop to maintain and no drift.

### "Will that be possible without losing features?" — the key feasibility question

Yes — **but only if (ii) is done in the correct direction.** The subtlety: option (b) must
mean *"keep `runtime.rs` as the one real loop and put an ACP adapter on top of it, then
delete `builtin_agent.rs`'s parallel loop"* — **not** *"port the chat over onto
`builtin_agent.rs`'s loop."* Direction matters enormously, because the two loops are not
peers — `builtin_agent.rs` is dramatically less capable, verified in code:

| Capability | `runtime.rs` (chat) | `builtin_agent.rs` (ACP) |
|---|---|---|
| Tool set | full `data/tools.json` via `tool_executor.rs` | a hand-written **5-tool** subset (`read_file`/`write_file`/`list_directory`/`search`/`run_command`) with *different names* |
| Tool-call parsing | real provider `tool_use` blocks | text scraping (`parse_llm_response`, with a `TODO: replace with proper structured output`) |
| Review mode / diff staging | full `ReviewSink` + `wait_for_review` + `ResolvedDiffOutcome` | none (`review_edits: false` hardcoded) |
| Command approval / permissions | real `PauseHandler::approve_command` | stubbed ("for now, we log that permission would be requested") |
| Cost-aware compaction | `cost_aware_compact` + retention engine + cache-breakpoint planning | a simplified duplicate (`compact_context`, self-described as simpler than the GUI path) |
| Live context bar, spend-cap enforcement, cancellation race | all present | partial / absent |

So porting *onto* the ACP loop would lose almost everything built over the last sessions.
Porting the ACP loop *onto* `runtime.rs` loses **nothing**, because the features live in the
*loop*, not the transport. That reframes the concern: **ACP is a transport/protocol, not an
agent loop.** The question "does ACP support review mode / compaction / the context bar?"
mostly dissolves:

- **Review-mode block, command approval → ACP `session/request_permission`.** ACP has a
  first-class permission-request round-trip; our `PauseHandler::approve_command` and
  `wait_for_review` map onto it directly (an approval request the client answers). This is
  the same shape `acp_permission_respond` already stubs.
- **Streaming output, tool-call visibility → ACP `session/update` notifications**
  (`agent_message_chunk`, `tool_call`, `tool_call_update`) — already emitted by
  `builtin_agent.rs`, so the surface exists.
- **Cancellation → ACP `session/cancel`**, which maps onto the existing `CancelToken`.
- **The genuinely IDE-internal signals — the live context-usage bar, cost/cache telemetry —
  have no ACP equivalent, and that's fine:** they're editor-internal Tauri events, not
  agent-protocol concerns, and stay as the side-channel events they already are regardless
  of transport. They don't need to travel over ACP to keep working.

**Net:** feature parity is preserved *by construction* if we keep `run_agent_turn_session`
as the single loop and treat ACP as an adapter/transport layer wrapping it. The real cost
of (ii) is engineering effort (writing that adapter so `runtime.rs`'s `PauseHandler`,
tool-executor, and event sink drive ACP's permission/update/cancel messages) plus deleting
the `builtin_agent.rs` loop — not lost capability. The one thing to prove out during
implementation: that ACP's permission-request round-trip is expressive enough for the
*per-hunk diff review* flow (accept some hunks, reject others), which is richer than a plain
yes/no approve. If it isn't, that specific flow may stay a side-channel Tauri event too —
worth confirming against the `agent-client-protocol` v1.2 permission schema early.

### User stories
- As an **Agent-user**, I want to drive an external agent like openCode from inside
  TraceLean, so that I have a fallback if the built-in agent isn't good enough yet.
- As a **Dev**, I want exactly one agent turn loop, so that the chat path and the ACP path
  can never diverge in behavior or bugs and I only maintain one.
- As an **Agent-user**, I want the switch to ACP-as-transport to be invisible, so that
  review mode, approvals, streaming, and the context bar all keep working exactly as today.

### Suggested implementation priority
1. **Ship (i) — the `AcpPanel.tsx` + openCode smoke test — first.** High user value (an
   escape hatch to a strong external agent), fully unblocked, low risk since it only *adds*
   a UI over working commands. It also exercises the ACP *client* path end-to-end against a
   real external agent, which de-risks (ii) by proving the transport interops before we
   depend on it internally.
2. **Then (ii), in the parity-preserving direction:** build an ACP adapter over
   `run_agent_turn_session` (map `PauseHandler`/tool-executor/event-sink onto ACP
   permission/update/cancel messages), switch the `tracelean-agent` binary to run *that*
   instead of its own loop, verify feature parity (especially the per-hunk review flow),
   then **delete `builtin_agent.rs`'s parallel loop.** Do not start the deletion until the
   adapter demonstrably matches today's chat behavior on review mode + compaction.
3. **Optionally, last:** route the internal chat panel through the same ACP adapter too, so
   even the in-app agent speaks ACP internally — the full "ACP is the only loop" end state.
   Gate this behind (2) being proven, since it's the change with the most user-visible blast
   radius if the adapter has gaps.
