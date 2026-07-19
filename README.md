# TraceLean IDE — User Guide

TraceLean is a command-sourced code editor with an integrated AI coding agent.
Every edit — yours or the AI's — is a small, undoable command, so the full
history of a session is a navigable tree, not a linear undo stack.

It ships in three forms, all sharing the same Rust core:

- **GUI** — Tauri + React desktop app (the main experience)
- **TUI** — Ratatui terminal interface
- **CLI agents** — headless agent binaries for benchmarks and ACP clients

All code lives in [`tracelean/`](tracelean/) (developer details in
[`tracelean/README.md`](tracelean/README.md)).

---

## 1. Quick start (GUI)

Prerequisites: Rust toolchain (rustup), Node.js ≥ 18, npm, and the
[Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
cd tracelean
npm install          # frontend dependencies (first time only)
cargo tauri dev      # launch the IDE with hot-reload
```

For a release build:

```bash
cargo tauri build
```

### Configuring an AI provider

Open the **AI Chat** panel → **Settings** (gear icon), then either:

- paste an **OpenRouter** API key, or
- paste an **AWS Bedrock** bearer token (or export `AWS_BEARER_TOKEN_BEDROCK`
  before launching) and pick a region (default `eu-west-1`),

then select a model from the catalog. A per-session **spend cap** (default $1)
refuses new requests once exceeded — raise it in the same settings pane.

---

## 2. The panels

| Panel | What it does |
|---|---|
| **File Tree** | Browse the project; badges show pending change counts per file while hovering undo-tree nodes. |
| **Editor** | CodeMirror editor. All edits become undoable commands. Hovering an undo node overlays its diff (red removed lines, green added blocks). |
| **AI Chat** | Chat with the coding agent. Tool calls appear as live chips (running / done / failed, with args and result previews). Multiple chat sessions via the switcher. |
| **Undo Tree** | Every command is a node; branches form when you undo and edit again. Click any node to jump the whole workspace to that state. Hover to preview its diff in the editor and file tree. |
| **Requirements** | Requirement tracking and traceability to code. |

### The status bar under the chat

- `Session: $0.0123` — cumulative AI spend this session.
- **Context meter** — how full the model's context window is
  (green < 60%, amber < 85%, red ≥ 85%). The `⟲` button resets the model's
  context (your visible transcript is kept).

---

## 3. Running the agent from the CLI

### Headless benchmark agent (`tracelean-bench`)

Runs the full agent loop (prompt → tools → LLM → … → tests pass) against
Exercism tasks, with no UI. Tasks are auto-downloaded on first run.

```bash
cd tracelean

# Credentials — pick one provider
export AWS_BEARER_TOKEN_BEDROCK=...        # Bedrock
export OPENROUTER_API_KEY=sk-or-...        # or OpenRouter

# List models available to your provider
cargo run --bin tracelean-bench -- -p bedrock --list-models

# Run one task, verbose, with a $0.10 spend cap
cargo run --bin tracelean-bench -- \
  -p bedrock -m minimax.minimax-m2.5 hello-world -v --cap 0.1 \
  2>&1 | tee trace.log

# Run the whole 10-task Exercism suite with a $2 cap
cargo run --bin tracelean-bench -- -p bedrock -m qwen.qwen3-coder-480b-a35b-instruct --cap 2.00 --all
```

Flags: `-p/--provider` (`bedrock` | `openrouter` | `mock`), `-m/--model`,
`-c/--cap` (USD), `--all`, `-v`. Environment fallbacks: `PROVIDER`,
`MODEL_ID`, `SPEND_CAP_USD`, `AWS_REGION`.

Results are printed and written to `benchmark_results.json`.

### ACP agent (`tracelean-agent`)

A standalone agent speaking the **Agent Client Protocol** over stdio, so any
ACP-capable client (including the IDE itself) can drive it:

```bash
cd tracelean
AWS_BEARER_TOKEN_BEDROCK=... cargo run --bin tracelean-agent
# or
OPENROUTER_API_KEY=sk-... cargo run --bin tracelean-agent
```

The agent reads keys from the environment; tool calls are executed by the
connecting client.

### Python benchmark wrapper (legacy)

```bash
cd tracelean
BENCHMARK_COST_CAP=0.50 python scripts/benchmark_exercism.py hello-world two-fer
```

---

## 4. Terminal UI

```bash
cd tracelean
cargo run -p tracelean-tui
```

Keys: `Ctrl+Q` quit, `Tab` cycle panels, `Alt+1–5` jump to a panel
(File Tree, Editor, AI Chat, Trace, Requirements).

---

## 5. Running the test suite

```bash
cd tracelean
cargo test --workspace          # Rust (core + backend + tests crate)
npx tsc --noEmit                # frontend type check
```

---

## 6. Where things live

```
tracelean/
  core/            shared engine: commands, undo tree, agent runtime, AI providers
  gui_backend/     Tauri backend (thin IPC bridge over core)
  react_frontend/  React GUI
  tui/             terminal frontend
  ui_settings/     user-editable keymap.json / bindings.json (Myth key system)
  data/            model catalog, tool schemas, cache/pricing metadata
  scripts/         model-metadata generators + Python benchmark
  exercism_tasks/  benchmark task cache
docs/              specs and design notes
example/           sample project used for manual testing
```
