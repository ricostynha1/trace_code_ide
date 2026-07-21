# TraceLean IDE

Command-sourced editor with formal verification traceability. Ships as both 
a GUI (Tauri + React) and a TUI (Ratatui terminal interface).

## Project Structure

```
core/              — shared engine (no UI deps)
gui_backend/       — Tauri backend (thin IPC bridge)
react_frontend/    — React GUI components
tui/               — Ratatui terminal frontend
tests/             — unit & integration tests
```

## Prerequisites

- Rust toolchain (rustup)
- Node.js ≥ 18
- npm

## Launching the GUI (Tauri + React)

```bash
# Install frontend deps
npm install

# Run in development mode (hot-reload)
cargo tauri dev

# Build release binary
cargo tauri build
```

> User-facing docs (keybindings, settings, panels) live in the
> [top-level README](../README.md). This file is developer-facing: build,
> run, benchmark, test.

## Launching the TUI

```bash
# Development
cargo run -p tracelean-tui

# Build release
cargo build --release -p tracelean-tui
# Binary at target/release/tracelean-tui
```

### TUI Keybindings

| Key | Action |
|---|---|
| Ctrl+Q | Quit |
| Tab | Cycle panels |
| Alt+1–5 | Jump to panel (FileTree, Editor, AI Chat, Trace, Requirements) |

## Generating Model Metadata JSONs

Single script fetches external data and produces `data/models.json`:

```bash
pip install -r scripts/requirements.txt
export LLM_STATS_API_KEY=ze_your_key_here

python scripts/generate_models.py
```

This runs internally:
1. `fetch_model_metadata.py` — fetches pricing (LiteLLM) + rankings (llm-stats.com) → `model_pricing.json`, `model_rankings.json`
2. `merge_model_data.py` — merges + enriches with provider cache config → `models.json`

Data files in `data/`:

| File | Source | Purpose |
|------|--------|---------|
| `models.json` | Generated | Unified model catalog: pricing + rank + cache config |
| `provider_cache.json` | Manual | Provider cache modes, discounts, TTL, write costs |
| `tools.json` | Manual | Tool schemas (static + dynamic) with `embedded_txt` |
| `retention_policy.json` | Manual | Retention engine entry policies and tuning defaults |

## Running Benchmarks

### Headless Agent (Rust binary)

The `tracelean-bench` binary runs the agent runtime headlessly — no GUI, no TUI. It exercises the full LLM tool loop (prompt → tools → LLM → ... → done) against exercism tasks and outputs JSON results.

```bash
# Set provider credentials (pick one)
export OPENROUTER_API_KEY=sk-or-...
# or
export AWS_BEARER_TOKEN_BEDROCK=...

# List available models for your provider
cargo run --bin tracelean-bench -- -p bedrock --list-models

# Custom cost cap
cargo run --bin tracelean-bench -- -p bedrock -m qwen.qwen3-coder-480b-a35b-instruct --cap 2.00 --all

# Real command to use 
  cargo run --bin tracelean-bench -- -p bedrock -m minimax.minimax-m2.5 hello-world -v --cap 0.1 2>&1 | tee full_output.log

```



**CLI flags**:

| Flag | Short | Default | Purpose |
|------|-------|---------|---------|
| `--provider` | `-p` | auto-detect | `openrouter`, `bedrock`, or `mock` |
| `--model` | `-m` | `anthropic/claude-sonnet-4-20250514` | Model ID |
| `--cap` | `-c` | `1.0` | Spend cap in USD |
| `--all` | | | Run all 10 default exercism tasks |

**Environment variables** (used as fallbacks when flags not given):

| Variable | Purpose |
|----------|---------|
| `OPENROUTER_API_KEY` | OpenRouter provider key |
| `AWS_BEARER_TOKEN_BEDROCK` | Bedrock provider token | 
| `AWS_REGION` | Bedrock region (default: eu-west-1) |
| `PROVIDER` | Same as `--provider` |
| `MODEL_ID` | Same as `--model` |
| `SPEND_CAP_USD` | Same as `--cap` |

**What it tests**: The full `core/src/agent/runtime.rs` loop — provider dispatch, tool selection, tool execution, cost tracking, failure handling — without any UI dependency.

### Python Script (legacy wrapper)

Exercism tasks are auto-downloaded from GitHub on first run.

```bash
# Run Aider Exercism benchmark (default $1 cost cap)
python scripts/benchmark_exercism.py

# Custom tasks and cost cap
BENCHMARK_COST_CAP=0.50 python scripts/benchmark_exercism.py hello-world two-fer

# Use different language track (default: python)
EXERCISM_LANGUAGE=rust python scripts/benchmark_exercism.py
```

Results written to `benchmark_results.json`.

## Running Tests

```bash
cargo test --workspace
```

> Settings/configuration reference has moved to the
> [top-level README](../README.md#1-quick-start-gui).

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
