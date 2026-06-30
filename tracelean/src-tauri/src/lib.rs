//! TraceLean IDE - Rust backend
//! Tauri IPC layer: thin dispatch to service module. No business logic here.

pub mod commands;
pub mod parser;
pub mod persistence;
pub mod requirements;
pub mod service;
pub mod state;
pub mod trace_graph;
pub mod undo_tree;

use commands::Command;
use parser::{Symbol, SymbolTable};
use serde::{Deserialize, Serialize};
use state::AppState;
use trace_graph::{TraceGraph, RequirementTraceOwned, CodeElementTraceOwned};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;
use tauri::{Emitter, AppHandle};

// --- Shared State Wrappers ---

pub struct AppStateWrapper(pub Mutex<AppState>);
pub struct SymbolTableWrapper(pub Mutex<SymbolTable>);
pub struct TraceGraphWrapper(pub Mutex<TraceGraph>);

/// Cached undo tree view — only rebuilt when tree_version changes.
pub struct UndoTreeCache {
    pub version: u64,
    pub view: Option<UndoTreeView>,
    pub file_filter: Option<String>,
}

impl UndoTreeCache {
    pub fn new() -> Self {
        Self { version: 0, view: None, file_filter: None }
    }
}

pub struct UndoTreeCacheWrapper(pub Mutex<UndoTreeCache>);

// --- Tauri IPC Commands (thin dispatch) ---

#[tauri::command]
fn apply_command(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    command: Command,
) -> Result<String, String> {
    let checkpoint_info = {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        service::apply_command(&mut s, command)
    };
    // Invalidate cache and notify frontend (outside the lock)
    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());

    // Persistence work outside the lock to avoid blocking other commands
    if let Some((root, _)) = checkpoint_info {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let _ = persistence::save_checkpoint(&root, &s);
        let _ = persistence::save_command_log(&root, s.command_log());
    }
    Ok("ok".into())
}

#[tauri::command]
fn undo(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
) -> Result<bool, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let result = s.undo();
    if result {
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
    }
    Ok(result)
}

#[tauri::command]
fn redo(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
) -> Result<bool, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let result = s.redo();
    if result {
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
    }
    Ok(result)
}

#[tauri::command]
fn get_file_content(state: State<'_, AppStateWrapper>, path: String) -> Result<Option<String>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    Ok(s.get_content(&PathBuf::from(&path)).map(|c| c.to_string()))
}

#[tauri::command]
fn open_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<String>, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::open_project(&mut s, &mut sym, &path)
}

#[tauri::command]
fn list_files(state: State<'_, AppStateWrapper>, path: String) -> Result<Vec<FileEntry>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().unwrap_or_default();
    Ok(service::list_directory_files(&root, &path))
}

#[tauri::command]
fn open_file(state: State<'_, AppStateWrapper>, path: String) -> Result<String, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    service::open_file(&mut s, &path)
}

#[tauri::command]
fn save_file(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<(), String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::save_file(&s, &mut sym, &path)
}

#[tauri::command]
fn save_checkpoint(state: State<'_, AppStateWrapper>) -> Result<(), String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().ok_or("No project open")?;
    persistence::save_checkpoint(&root, &s).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_undo_tree(
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    file_filter: Option<String>,
) -> Result<UndoTreeView, String> {
    let mut c = cache.0.lock().map_err(|e| e.to_string())?;
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let tree_version = s.undo_tree().len() as u64;

    // Return cached if version & filter match
    if c.version == tree_version && c.file_filter == file_filter {
        if let Some(ref view) = c.view {
            return Ok(view.clone());
        }
    }

    // Rebuild
    let tree = s.undo_tree();
    let current_id = tree.current_node().map(|n| n.id.to_string());

    let nodes: Vec<UndoNodeView> = tree.nodes().iter()
        .filter(|node| match &file_filter {
            None => true,
            Some(f) => command_affects_file(&node.command, f),
        })
        .map(|node| UndoNodeView {
            id: node.id.to_string(),
            parent: node.parent.map(|p| p.to_string()),
            children: node.children.iter().map(|c| c.to_string()).collect(),
            command_summary: command_summary(&node.command),
            file: command_file(&node.command),
            timestamp: node.timestamp.to_rfc3339(),
            is_commit_point: node.commit_point.is_some(),
            commit_name: node.commit_point.as_ref().map(|cp| cp.name.clone()),
        })
        .collect();

    let view = UndoTreeView { nodes, current_id };
    c.version = tree_version;
    c.file_filter = file_filter;
    c.view = Some(view.clone());
    Ok(view)
}

#[tauri::command]
fn get_command_log(
    state: State<'_, AppStateWrapper>,
    limit: Option<usize>,
) -> Result<Vec<CommandLogEntry>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let log = s.command_log();
    let n = limit.unwrap_or(50).min(log.len());
    Ok(log.iter().rev().take(n).enumerate()
        .map(|(i, cmd)| CommandLogEntry {
            index: log.len() - 1 - i,
            summary: command_summary(cmd),
        })
        .collect())
}

#[tauri::command]
fn jump_to_node(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    node_id: String,
) -> Result<bool, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::parse_str(&node_id).map_err(|e| e.to_string())?;
    if let Some(commands) = s.jump_to_node(id) {
        for cmd in commands {
            s.execute_raw(&cmd);
        }
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
fn clear_undo_tree(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
) -> Result<(), String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    s.clear_history();
    if let Some(root) = s.project_root().cloned() {
        let _ = persistence::save_command_log(&root, &[]);
        let _ = std::fs::remove_file(persistence::commands_dir(&root).join("checkpoint.json"));
    }
    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());
    Ok(())
}

#[tauri::command]
fn parse_file_symbols(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<Symbol>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::parse_file_symbols(&s, &mut sym, &path)
}

#[tauri::command]
fn get_file_symbols(symbols_state: State<'_, SymbolTableWrapper>, path: String) -> Result<Vec<Symbol>, String> {
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    Ok(sym.get_symbols(&PathBuf::from(&path)).cloned().unwrap_or_default())
}

#[tauri::command]
fn get_highlights(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<Vec<parser::HighlightSpan>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let rel_path = PathBuf::from(&path);

    // Use buffer content if available, else read from disk
    let content = if let Some(c) = s.get_content(&rel_path) {
        c.to_string()
    } else {
        let root = s.project_root().cloned().unwrap_or_default();
        std::fs::read_to_string(root.join(&path))
            .map_err(|e| format!("Read error: {}", e))?
    };

    Ok(parser::get_highlights(&rel_path, &content))
}

#[tauri::command]
fn parse_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
) -> Result<usize, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::parse_project(&s, &mut sym)
}

// --- Trace Graph ---

#[tauri::command]
fn build_trace_graph(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
) -> Result<TraceGraphStats, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let mut g = graph_state.0.lock().map_err(|e| e.to_string())?;
    let (nodes, edges) = service::build_trace_graph(&s, &sym, &mut g)?;
    Ok(TraceGraphStats { nodes, edges })
}

#[tauri::command]
fn query_requirement_trace(
    graph_state: State<'_, TraceGraphWrapper>,
    req_id: String,
) -> Result<Option<RequirementTraceOwned>, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(g.query_requirement_owned(&req_id))
}

#[tauri::command]
fn query_code_trace(
    graph_state: State<'_, TraceGraphWrapper>,
    file: String,
    name: String,
) -> Result<Option<CodeElementTraceOwned>, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(g.query_code_element_owned(&PathBuf::from(&file), &name))
}

#[tauri::command]
fn update_trace_graph_file(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    path: String,
) -> Result<bool, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let mut g = graph_state.0.lock().map_err(|e| e.to_string())?;
    service::update_trace_graph_file(&s, &sym, &mut g, &path)
}

#[tauri::command]
fn get_trace_graph_stats(graph_state: State<'_, TraceGraphWrapper>) -> Result<TraceGraphStats, String> {
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(TraceGraphStats { nodes: g.node_count(), edges: g.edge_count() })
}

// --- Requirements ---

#[tauri::command]
fn list_requirements(state: State<'_, AppStateWrapper>) -> Result<Vec<requirements::RequirementInfo>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    service::list_requirements(&s)
}

#[tauri::command]
fn update_requirement_status(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    new_status: String,
) -> Result<String, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    service::update_requirement_status(&mut s, &req_id, &new_status)
}

#[tauri::command]
fn create_requirement(
    state: State<'_, AppStateWrapper>,
    req_id: String,
    title: String,
) -> Result<String, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    service::create_requirement(&mut s, &req_id, &title)
}

#[tauri::command]
fn check_lean_spec(state: State<'_, AppStateWrapper>, path: String) -> Result<requirements::LeanCheckResult, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    service::check_lean_spec(&s, &path)
}

#[tauri::command]
fn get_editor_mode(path: String) -> String {
    service::get_editor_mode(&path).to_string()
}

#[tauri::command]
fn navigate_trace_link(state: State<'_, AppStateWrapper>, from_path: String) -> Result<Option<String>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    service::navigate_trace_link(&s, &from_path)
}

// --- Shared Types (for IPC serialization) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TraceGraphStats {
    nodes: usize,
    edges: usize,
}

#[derive(Debug, Clone, Serialize)]
struct UndoTreeView {
    nodes: Vec<UndoNodeView>,
    current_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct UndoNodeView {
    id: String,
    parent: Option<String>,
    children: Vec<String>,
    command_summary: String,
    file: Option<String>,
    timestamp: String,
    is_commit_point: bool,
    commit_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CommandLogEntry {
    index: usize,
    summary: String,
}

// --- Command Helpers (presentation logic, OK to live here) ---

fn invalidate_undo_cache(cache: &State<'_, UndoTreeCacheWrapper>) {
    if let Ok(mut c) = cache.0.lock() {
        c.view = None;
    }
}

fn command_summary(cmd: &Command) -> String {
    match cmd {
        Command::Insert { file, text, offset, .. } => {
            let preview = if text.len() > 20 { format!("{}...", &text[..20]) } else { text.clone() };
            format!("Insert @{}:{} \"{}\"", file.display(), offset, preview)
        }
        Command::Delete { file, offset, len, .. } => format!("Delete @{}:{} len={}", file.display(), offset, len),
        Command::Replace { file, offset, .. } => format!("Replace @{}:{}", file.display(), offset),
        Command::SetCursor { file, new_pos, .. } => format!("Cursor @{}:{}:{}", file.display(), new_pos.line, new_pos.col),
        Command::SetSelection { file, .. } => format!("Select @{}", file.display()),
        Command::CreateFile { path } => format!("Create {}", path.display()),
        Command::DeleteFile { path, .. } => format!("Delete file {}", path.display()),
        Command::RenameFile { from, to } => format!("Rename {} → {}", from.display(), to.display()),
        Command::Batch { commands } => format!("Batch ({} cmds)", commands.len()),
    }
}

fn command_file(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Insert { file, .. } | Command::Delete { file, .. }
        | Command::Replace { file, .. } | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => Some(file.to_string_lossy().to_string()),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => Some(path.to_string_lossy().to_string()),
        Command::RenameFile { from, .. } => Some(from.to_string_lossy().to_string()),
        Command::Batch { .. } => None,
    }
}

fn command_affects_file(cmd: &Command, filter: &str) -> bool {
    match cmd {
        Command::Insert { file, .. } | Command::Delete { file, .. }
        | Command::Replace { file, .. } | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => file.to_string_lossy() == filter,
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.to_string_lossy() == filter,
        Command::RenameFile { from, to } => from.to_string_lossy() == filter || to.to_string_lossy() == filter,
        Command::Batch { commands } => commands.iter().any(|c| command_affects_file(c, filter)),
    }
}

// --- App Entry Point ---

#[tauri::command]
fn get_initial_project() -> Option<String> {
    let p = std::path::Path::new("/project");
    if p.is_dir() {
        Some("/project".to_string())
    } else {
        None
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppStateWrapper(Mutex::new(AppState::new())))
        .manage(SymbolTableWrapper(Mutex::new(SymbolTable::new())))
        .manage(TraceGraphWrapper(Mutex::new(TraceGraph::new())))
        .manage(UndoTreeCacheWrapper(Mutex::new(UndoTreeCache::new())))
        .invoke_handler(tauri::generate_handler![
            apply_command,
            undo,
            redo,
            get_file_content,
            open_project,
            list_files,
            open_file,
            save_file,
            save_checkpoint,
            get_undo_tree,
            get_command_log,
            jump_to_node,
            clear_undo_tree,
            parse_file_symbols,
            get_file_symbols,
            get_highlights,
            parse_project,
            build_trace_graph,
            query_requirement_trace,
            query_code_trace,
            update_trace_graph_file,
            get_trace_graph_stats,
            list_requirements,
            update_requirement_status,
            create_requirement,
            check_lean_spec,
            get_editor_mode,
            navigate_trace_link,
            get_initial_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
