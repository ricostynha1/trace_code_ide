# TraceLean IDE

Command-sourced editor with formal verification traceability. Ships as both a GUI (Tauri + React) and a TUI (Ratatui terminal interface).

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

## Running Tests

```bash
cargo test --workspace
```

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
