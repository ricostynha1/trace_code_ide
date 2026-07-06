# Spec: TUI Decoupling & Folder Restructure

## Goal

Decouple core logic from Tauri so both a GUI (Tauri/React) and a TUI (Ratatui) can share the same engine. Rename folders for clarity.

## Folder Rename

### Current → New

| Current | New | Notes |
|---|---|---|
| `src/` | `react_frontend/` | React UI source |
| `src-tauri/` | `gui_backend/` | Tauri Rust backend |

### Tauri Rename Feasibility

Tauri does **not** hardcode `src-tauri`. The path is resolved from `Cargo.toml` workspace membership. Renaming is possible with these changes:

1. `Cargo.toml` (workspace): `members = ["gui_backend", "core", "tui", "tests"]`
2. `vite.config.ts`: update `ignored` pattern from `**/src-tauri/**` to `**/gui_backend/**`
3. `tests/Cargo.toml`: update path dep from `../src-tauri` to `../gui_backend`
4. Tauri CLI: pass `--config` or set `TAURI_CONFIG_DIR` if needed (Tauri v2 auto-detects via workspace Cargo.toml — should work without extra config)
5. `tauri.conf.json` stays inside renamed folder (`gui_backend/tauri.conf.json`)
6. `frontendDist`: update from `../dist` to `../react_frontend/dist` if vite output dir changes
7. Vite `root` or `build.outDir` must match

### React Frontend Rename

1. Move `src/` → `react_frontend/`
2. Update `vite.config.ts`: set `root: 'react_frontend'` or adjust `index.html` path
3. Update `tsconfig.json` include paths
4. Update `tauri.conf.json` `frontendDist` to `../react_frontend/dist` (or keep `../dist` if vite outDir stays at project root)

## New Project Structure

```
tracelean/
├── core/                  ← NEW shared lib crate (no Tauri dep)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs         ← SharedApp, EventSink trait, re-exports
│       ├── state.rs
│       ├── commands.rs
│       ├── undo_tree.rs
│       ├── trace_graph.rs
│       ├── parser.rs
│       ├── persistence.rs
│       ├── requirements.rs
│       ├── service.rs
│       └── ai/            ← full AI subsystem
├── gui_backend/           ← (was src-tauri) Tauri-specific thin layer
│   ├── Cargo.toml         ← depends on `core`
│   ├── tauri.conf.json
│   └── src/
│       ├── lib.rs         ← Tauri app setup, manage SharedApp
│       ├── main.rs
│       └── ipc/           ← thin command dispatchers
├── tui/                   ← NEW Ratatui binary
│   ├── Cargo.toml         ← depends on `core`, ratatui, crossterm
│   └── src/
│       ├── main.rs
│       ├── app.rs         ← event loop, Elm architecture
│       ├── input.rs       ← keybinds
│       └── ui/            ← render panels
├── react_frontend/        ← (was src/) React components
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   └── App.css
├── tests/                 ← existing unit/integration tests
├── Cargo.toml             ← workspace: members = [core, gui_backend, tui, tests]
├── package.json
├── vite.config.ts
└── tsconfig.json
```

## Core Crate Design

### SharedApp

```rust
pub struct SharedApp {
    pub state: Arc<Mutex<AppState>>,
    pub symbols: Arc<Mutex<SymbolTable>>,
    pub graph: Arc<Mutex<TraceGraph>>,
    pub ai_settings: Arc<Mutex<AiSettings>>,
    pub ai_log: Arc<Mutex<InteractionLog>>,
    pub ai_stats: Arc<Mutex<SessionStats>>,
    pub pending_diffs: Arc<Mutex<Vec<PendingDiff>>>,
    pub mcp_client: Arc<TokioMutex<McpClientManager>>,
    pub agent_permissions: Arc<Mutex<HashMap<String, AgentPermissions>>>,
    pub event_sink: Arc<dyn EventSink>,
}
```

### EventSink Trait

```rust
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: &str);
}
```

- Tauri impl: wraps `AppHandle.emit()`
- TUI impl: writes to `tokio::sync::broadcast` channel consumed by render loop

### Service Methods

All current `service.rs` fns become `impl SharedApp` methods:

```rust
impl SharedApp {
    pub fn apply_command(&self, cmd: Command) -> Result<(), String>;
    pub fn open_project(&self, path: &str) -> Result<Vec<String>, String>;
    pub fn open_file(&self, path: &str) -> Result<String, String>;
    pub fn save_file(&self, path: &str) -> Result<(), String>;
    pub fn build_trace_graph(&self) -> Result<(usize, usize), String>;
    pub fn ai_chat(&self, msg: &str) -> Result<String, String>;
    // ...
}
```

## Migration Steps (ordered)

1. **Rename folders** — `src/` → `react_frontend/`, `src-tauri/` → `gui_backend/`
2. **Fix references** — workspace Cargo.toml, vite.config.ts, tests/Cargo.toml, tsconfig paths
3. **Verify build** — `cargo build` + `npm run dev` still work
4. **Extract `core/` crate** — move domain modules out of `gui_backend/src/`, strip Tauri types
5. **Create `SharedApp`** — in core, hold all state as `Arc<Mutex<>>`
6. **Wire `gui_backend`** — depend on core, create `SharedApp` in `run()`, pass to IPC
7. **Verify build again**
8. **Create `tui/` crate** — skeleton with Ratatui, depend on core, instantiate `SharedApp`
9. **Implement TUI panels incrementally** — file tree → editor → AI chat → trace dashboard

## Risks & Notes

- **Tauri CLI detection**: Tauri v2 finds the backend via workspace. If it breaks, add `tauri.conf.json` → `"build": { "rustTarget": "gui_backend" }` or use `--target` flag.
- **Async boundary**: `gui_backend` uses `tokio` (Tauri runtime). `tui` also needs `tokio` for AI/MCP. Core should be runtime-agnostic where possible (sync Mutex for state, async only for IO).
- **Code editor in TUI**: Full syntax-highlighted editing in terminal is hard. Options: use `tui-textarea` crate, or delegate to `$EDITOR` for editing and show read-only highlighted view in TUI.
- **Feature parity**: TUI doesn't need pixel-perfect parity. Focus on: file nav, AI chat, trace queries, requirement management. Full code editing can stay GUI-primary.
