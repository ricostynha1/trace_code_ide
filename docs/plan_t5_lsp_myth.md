# T5 — LSP client and Myth integration: implementation plan

Design reference: `docs/formal_traceability_and_lsp_plan.md` §11, §12.
Independent of T2/T3; depends on T1 only for the `Trace` mode actions.

## 1. Client transport (`core/src/lsp/`)

Crate: `async-lsp` — TraceLean is the *client*; `tower-lsp` builds servers and dispatches
notifications out of order, which breaks live diagnostics.

| File | Contents |
|---|---|
| `mod.rs` | `LspRegistry`: language → running server, lifecycle, capability negotiation |
| `server.rs` | spawn, initialize, shutdown; restart with backoff on crash |
| `document.rs` | `didOpen`/`didChange`/`didClose` synced from the command stream |
| `query.rs` | request wrappers: hover, definition, references, symbols, code actions |
| `lean.rs` | Lean-specific extensions: `$/lean/plainGoal`, `$/lean/plainTermGoal`, `textDocument/waitForDiagnostics` |
| `edits.rs` | `WorkspaceEdit` → invertible `Command`s |

Server table, resolved by file extension: `rust` → `rust-analyzer`; `lean` → `lake serve`
(cwd = the nearest directory with a `lakefile.lean`/`lakefile.toml`, else `lean --server`);
`py` → `pyright-langserver --stdio`, falling back to `ruff server`.

A missing server is a first-class state — "rust-analyzer not found on PATH" with an install
hint — never a silent absence of features.

## 2. Lean specifics that a generic client gets wrong
- Diagnostics arrive incrementally from elaboration. Use `textDocument/waitForDiagnostics`
  before reporting a file clean; treating "nothing yet" as "fine" shows green on an
  unchecked model, which is exactly the lie this project exists to prevent.
- **Implement `$/lean/plainGoal` first**, before hover and symbols: goal-state-at-cursor is
  what makes writing a model and its proofs bearable.
- Set `LEAN_SERVER_LOG_DIR` when a debug flag is on.

## 3. `lsp_query` — the agent-facing tool

One tool, modes `hover | definition | references | diagnostics | symbols | code_actions |
goal`. Registered in `data/tools.json` and the executor, like every other agent tool.

Why it matters more than the editor features: an agent writing Lean without goal state is
guessing, and a goal costs ~200 tokens against thousands for re-reading the file. This is the
single highest-leverage item in T5.

## 4. Myth changes

1. **Dynamic action providers.** `bindings.json` maps a capture to a *static* list; LSP code
   actions and trace actions are computed at the cursor. Introduce
   `trait ActionProvider { fn actions_at(&self, cx: &CursorCx) -> Vec<Action>; }` with three
   implementations: the existing static map, an LSP provider, a trace provider. This is the
   change everything else hangs off.
2. **`Action` gains `title`, `group`, `priority`** — which-key showing nine raw LSP action
   names is unusable; LSP's `kind` (`quickfix`, `refactor.extract`) supplies the grouping.
3. **Mode stack.** Entering a mode pushes; `Escape` pops one level. Today `Escape` in `File`
   jumps to `Main` while the README says "back to Options" — fix the behaviour and generate
   the README table from `keymap.json` so they cannot drift again.
4. **New modes** `Goto`, `Verify`, `Trace` exactly as listed in design §12.

## 5. `WorkspaceEdit` → `Command` (blocking)
Rename and code-action edits must become invertible commands in the undo tree. Two reasons,
both load-bearing: "one history of everything" is the product's core claim, and T1's anchors
follow renames only if the rename is a command. Without this, every LSP rename silently
breaks traceability links.

## 6. Order of work
1 transport + registry → 2 Lean goal state → 3 `lsp_query` agent tool → 4 `WorkspaceEdit` →
5 dynamic providers → 6 mode stack + new modes → 7 hover/definition/symbols in the editor →
8 live diagnostics (needs incremental re-parse; do last).

## 7. Risks
- No Lean toolchain on this machine, and rust-analyzer may be absent too: every server must
  degrade to "not configured", and tests must not require a server to be installed.
- Live diagnostics need incremental re-parse in the surface model — explicitly last, and not
  allowed to block items 1–7.

---

# Revision — the transport is hand-rolled, not `async-lsp`

The plan named `async-lsp`. On implementation I went with a small client written against the
protocol directly. The reasoning, recorded so it can be reversed on evidence rather than
re-argued:

1. **The surface actually needed is small.** Initialize and capability negotiation,
   `didOpen`/`didChange`/`didClose`, a handful of request/response methods, and Lean's
   `$/lean/plainGoal`. That is framing plus id correlation — a few hundred lines, against a
   tower-based async stack pulled into `core`, which today has no `tower` and no `lsp-types`.
2. **The codebase already has this shape twice.** `ai/mcp_client.rs` speaks NDJSON over a
   long-lived child process, and `drt/protocol.rs` (T2) now does the same with timeouts,
   restarts and stderr capture. A third instance with LSP's `Content-Length` framing reuses
   patterns the project already maintains rather than importing a new paradigm.
3. **Ordering was the whole reason `tower-lsp` was rejected.** Owning the dispatch loop makes
   "notifications are handled synchronously, in order" a property of our code rather than a
   property we have to verify in someone else's.
4. **Server-initiated requests must be answered** (`workspace/configuration`,
   `client/registerCapability`) or some servers stall. That is explicit in a hand-rolled loop
   and implicit in a framework.

What would reverse this: needing the long tail of LSP (semantic tokens, inlay hints, call
hierarchy, workspace symbols with partial results), at which point `lsp-types` alone —
without the tower layer — is the cheaper half of the dependency.

---

# Revision — as built

Items 1–7 of §6 are done. Item 8, live diagnostics, is **not**, exactly as the plan
required: it needs incremental tree-sitter re-parse in the surface model, and §7 forbids
blocking 1–7 on it.

Three things came out differently from the plan and are worth recording:

1. **The `CodeActions` mode is dynamic, and its keys are resolved in the frontend.** A mode
   in `keymap.json` maps fixed keys to fixed actions; a code-action list is computed at the
   cursor. So `CodeActions` defines only `Escape`, the editor asks `myth_actions_at` for
   what is actually available, and `keymap::dynamic_bindings` hands out home-row keys. A
   menu that has to truncate reports how many entries it hid rather than silently dropping
   them.
2. **`→Main` is a reset, not a push.** Making the stack honest meant deciding what the old
   spelling means; `Main` is the floor of the stack, so a transition *to* it can only be a
   reset. `pop` and `reset` are now first-class keymap verbs.
3. **`goto_provenance` is answered from the undo tree, not from git** (`core/src/provenance.rs`).
   Walking one `Replace` backwards maps a post-edit char position to its pre-edit one
   exactly, so the answer needs no replay and no heuristics. Text that arrived with the file
   reports as the origin rather than being attributed to whoever touched the file last —
   the honest answer rather than a plausible one.
