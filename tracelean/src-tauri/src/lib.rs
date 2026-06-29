//! TraceLean IDE - Rust backend
//! Command-sourced architecture: every state mutation is a reversible command.

pub mod commands;
pub mod parser;
pub mod persistence;
pub mod state;
pub mod undo_tree;

use commands::Command;
use parser::{Symbol, SymbolTable};
use state::AppState;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::State;

/// Shared application state wrapped in Mutex for thread safety
pub struct AppStateWrapper(pub Mutex<AppState>);

/// Shared symbol table
pub struct SymbolTableWrapper(pub Mutex<SymbolTable>);

/// How many commands between auto-checkpoints
const CHECKPOINT_INTERVAL: usize = 100;

// --- Tauri IPC Commands ---

/// Apply a command from the frontend
#[tauri::command]
fn apply_command(
    state: State<'_, AppStateWrapper>,
    command: Command,
) -> Result<String, String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    let _inverse = app_state.apply(command);

    // Auto-checkpoint every N commands
    let log_len = app_state.command_log().len();
    if log_len > 0 && log_len % CHECKPOINT_INTERVAL == 0 {
        if let Some(root) = app_state.project_root().cloned() {
            let _ = persistence::save_checkpoint(&root, &app_state);
            let _ = persistence::save_command_log(&root, app_state.command_log());
        }
    }

    Ok("ok".to_string())
}

/// Undo the last command
#[tauri::command]
fn undo(state: State<'_, AppStateWrapper>) -> Result<bool, String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    Ok(app_state.undo())
}

/// Redo the next command
#[tauri::command]
fn redo(state: State<'_, AppStateWrapper>) -> Result<bool, String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    Ok(app_state.redo())
}

/// Get file content
#[tauri::command]
fn get_file_content(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<Option<String>, String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let p = PathBuf::from(&path);
    Ok(app_state.get_content(&p).map(|s| s.to_string()))
}

/// Open a project folder
#[tauri::command]
fn open_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<String>, String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = PathBuf::from(&path);

    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }

    app_state.set_project_root(root.clone());

    // Try to restore persisted state
    if let Ok(restored) = persistence::restore_state(&root) {
        *app_state = restored;
        app_state.set_project_root(root.clone());
    }

    // Parse all source files in parallel
    let files = collect_source_files(&root);
    let file_contents: Vec<(PathBuf, String)> = files.iter().filter_map(|p| {
        let content = std::fs::read_to_string(root.join(p)).ok()?;
        Some((p.clone(), content))
    }).collect();

    let results = parser::parse_files_parallel(&file_contents);
    if let Ok(mut table) = symbols_state.0.lock() {
        for result in results {
            table.files.insert(result.path, result.symbols);
        }
    }

    // List files
    let entries = list_files_recursive(&root, &root, 3)
        .map_err(|e| e.to_string())?;
    Ok(entries)
}

/// List files in a directory (for file tree panel)
#[tauri::command]
fn list_files(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<Vec<FileEntry>, String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = app_state.project_root().cloned().unwrap_or_default();
    let target = if path.is_empty() {
        root.clone()
    } else {
        root.join(&path)
    };

    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&target) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files and .tracelean
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let rel_path = entry.path().strip_prefix(&root)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .to_string();
            entries.push(FileEntry {
                name,
                path: rel_path,
                is_dir,
            });
        }
    }
    entries.sort_by(|a, b| {
        // Dirs first, then alpha
        b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name))
    });
    Ok(entries)
}

/// Open (load) a file into buffer
#[tauri::command]
fn open_file(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<String, String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = app_state.project_root().cloned().unwrap_or_default();
    let full_path = root.join(&path);
    let rel_path = PathBuf::from(&path);

    // If already in buffer, return content
    if let Some(content) = app_state.get_content(&rel_path) {
        return Ok(content.to_string());
    }

    // Read from disk
    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;
    app_state.load_file(rel_path, content.clone());
    Ok(content)
}

/// Save file to disk (writes buffer content to filesystem)
#[tauri::command]
fn save_file(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<(), String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = app_state.project_root().cloned().unwrap_or_default();
    let rel_path = PathBuf::from(&path);
    let full_path = root.join(&path);

    let content = app_state.get_content(&rel_path)
        .ok_or_else(|| format!("File not in buffer: {}", path))?;
    std::fs::write(&full_path, content)
        .map_err(|e| format!("Failed to write {}: {}", path, e))?;

    // Re-parse symbols on save
    if let Ok(mut table) = symbols_state.0.lock() {
        table.parse_file(&rel_path, content);
    }

    // Persist command log
    if let Err(e) = persistence::save_command_log(&root, app_state.command_log()) {
        eprintln!("Warning: failed to persist command log: {}", e);
    }

    Ok(())
}

/// Save checkpoint
#[tauri::command]
fn save_checkpoint(state: State<'_, AppStateWrapper>) -> Result<(), String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = app_state.project_root().cloned()
        .ok_or("No project open")?;
    persistence::save_checkpoint(&root, &app_state)
        .map_err(|e| e.to_string())
}

/// Get undo-tree structure for visualization
/// Get undo-tree structure for visualization.
/// Optional file_filter: if provided, only show nodes that affect that file.
#[tauri::command]
fn get_undo_tree(
    state: State<'_, AppStateWrapper>,
    file_filter: Option<String>,
) -> Result<UndoTreeView, String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let tree = app_state.undo_tree();
    let current_id = tree.current_node().map(|n| n.id.to_string());

    let nodes: Vec<UndoNodeView> = tree.nodes().iter()
        .filter(|node| {
            match &file_filter {
                None => true,
                Some(filter) => command_affects_file(&node.command, filter),
            }
        })
        .map(|node| {
            UndoNodeView {
                id: node.id.to_string(),
                parent: node.parent.map(|p| p.to_string()),
                children: node.children.iter().map(|c| c.to_string()).collect(),
                command_summary: command_summary(&node.command),
                file: command_file(&node.command),
                timestamp: node.timestamp.to_rfc3339(),
                is_commit_point: node.commit_point.is_some(),
                commit_name: node.commit_point.as_ref().map(|c| c.name.clone()),
            }
        }).collect();

    Ok(UndoTreeView { nodes, current_id })
}

/// Get recent command log (last N entries)
#[tauri::command]
fn get_command_log(
    state: State<'_, AppStateWrapper>,
    limit: Option<usize>,
) -> Result<Vec<CommandLogEntry>, String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let log = app_state.command_log();
    let n = limit.unwrap_or(50).min(log.len());
    let entries: Vec<CommandLogEntry> = log.iter()
        .rev()
        .take(n)
        .enumerate()
        .map(|(i, cmd)| CommandLogEntry {
            index: log.len() - 1 - i,
            summary: command_summary(cmd),
        })
        .collect();
    Ok(entries)
}

/// Jump to a specific undo-tree node
#[tauri::command]
fn jump_to_node(
    state: State<'_, AppStateWrapper>,
    node_id: String,
) -> Result<bool, String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::parse_str(&node_id).map_err(|e| e.to_string())?;
    if let Some(commands) = app_state.jump_to_node(id) {
        for cmd in commands {
            app_state.execute_raw(&cmd);
        }
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Clear undo tree and command log (reset history)
#[tauri::command]
fn clear_undo_tree(state: State<'_, AppStateWrapper>) -> Result<(), String> {
    let mut app_state = state.0.lock().map_err(|e| e.to_string())?;
    app_state.clear_history();
    if let Some(root) = app_state.project_root().cloned() {
        let _ = persistence::save_command_log(&root, &[]);
        let _ = std::fs::remove_file(
            persistence::commands_dir(&root).join("checkpoint.json")
        );
    }
    Ok(())
}

/// Parse a file and extract symbols
#[tauri::command]
fn parse_file_symbols(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<Symbol>, String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = app_state.project_root().cloned().unwrap_or_default();
    let rel_path = PathBuf::from(&path);
    let full_path = root.join(&path);

    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Read error: {}", e))?;

    let mut table = symbols_state.0.lock().map_err(|e| e.to_string())?;
    match table.parse_file(&rel_path, &content) {
        Some(symbols) => Ok(symbols),
        None => Ok(Vec::new()),
    }
}

/// Get symbols for a file (from cache, no re-parse)
#[tauri::command]
fn get_file_symbols(
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<Symbol>, String> {
    let table = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let p = PathBuf::from(&path);
    Ok(table.get_symbols(&p).cloned().unwrap_or_default())
}

/// Parse all project files in parallel (startup scan)
#[tauri::command]
fn parse_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
) -> Result<usize, String> {
    let app_state = state.0.lock().map_err(|e| e.to_string())?;
    let root = app_state.project_root().cloned()
        .ok_or("No project open")?;

    // Collect all parseable files
    let files = collect_source_files(&root);
    let file_contents: Vec<(PathBuf, String)> = files.iter().filter_map(|p| {
        let content = std::fs::read_to_string(root.join(p)).ok()?;
        Some((p.clone(), content))
    }).collect();

    // Parse in parallel
    let results = parser::parse_files_parallel(&file_contents);

    // Store results
    let mut table = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let count = results.len();
    for result in results {
        table.files.insert(result.path, result.symbols);
    }

    Ok(count)
}

/// Collect source files from project (recursively, skip hidden dirs)
fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_source_files_inner(root, root, &mut files);
    files
}

fn collect_source_files_inner(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_source_files_inner(&path, root, files);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if parser::Lang::from_extension(ext).is_some() {
                if let Ok(rel) = path.strip_prefix(root) {
                    files.push(rel.to_path_buf());
                }
            }
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct UndoTreeView {
    nodes: Vec<UndoNodeView>,
    current_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
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

#[derive(Debug, Clone, serde::Serialize)]
struct CommandLogEntry {
    index: usize,
    summary: String,
}

/// Human-readable summary of a command
fn command_summary(cmd: &Command) -> String {
    match cmd {
        Command::Insert { file, text, offset, .. } => {
            let preview = if text.len() > 20 {
                format!("{}...", &text[..20])
            } else {
                text.clone()
            };
            format!("Insert @{}:{} \"{}\"", file.display(), offset, preview)
        }
        Command::Delete { file, offset, len, .. } => {
            format!("Delete @{}:{} len={}", file.display(), offset, len)
        }
        Command::Replace { file, offset, .. } => {
            format!("Replace @{}:{}", file.display(), offset)
        }
        Command::SetCursor { file, new_pos, .. } => {
            format!("Cursor @{}:{}:{}", file.display(), new_pos.line, new_pos.col)
        }
        Command::SetSelection { file, .. } => {
            format!("Select @{}", file.display())
        }
        Command::CreateFile { path } => format!("Create {}", path.display()),
        Command::DeleteFile { path, .. } => format!("Delete file {}", path.display()),
        Command::RenameFile { from, to } => format!("Rename {} → {}", from.display(), to.display()),
        Command::Batch { commands } => format!("Batch ({} cmds)", commands.len()),
    }
}

/// Extract the file path from a command (if single-file)
fn command_file(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Insert { file, .. }
        | Command::Delete { file, .. }
        | Command::Replace { file, .. }
        | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => Some(file.to_string_lossy().to_string()),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => {
            Some(path.to_string_lossy().to_string())
        }
        Command::RenameFile { from, .. } => Some(from.to_string_lossy().to_string()),
        Command::Batch { .. } => None,
    }
}

/// Check if a command affects a specific file
fn command_affects_file(cmd: &Command, filter: &str) -> bool {
    match cmd {
        Command::Insert { file, .. }
        | Command::Delete { file, .. }
        | Command::Replace { file, .. }
        | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => file.to_string_lossy() == filter,
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => {
            path.to_string_lossy() == filter
        }
        Command::RenameFile { from, to } => {
            from.to_string_lossy() == filter || to.to_string_lossy() == filter
        }
        Command::Batch { commands } => commands.iter().any(|c| command_affects_file(c, filter)),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// Recursively list files up to a given depth
fn list_files_recursive(
    dir: &PathBuf,
    root: &PathBuf,
    max_depth: usize,
) -> std::io::Result<Vec<String>> {
    let mut result = Vec::new();
    list_files_inner(dir, root, max_depth, 0, &mut result)?;
    Ok(result)
}

fn list_files_inner(
    dir: &PathBuf,
    root: &PathBuf,
    max_depth: usize,
    current_depth: usize,
    result: &mut Vec<String>,
) -> std::io::Result<()> {
    if current_depth >= max_depth {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let rel = entry.path().strip_prefix(root)
            .unwrap_or(&entry.path())
            .to_string_lossy()
            .to_string();
        if entry.file_type()?.is_dir() {
            result.push(format!("{}/", rel));
            list_files_inner(&entry.path(), root, max_depth, current_depth + 1, result)?;
        } else {
            result.push(rel);
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppStateWrapper(Mutex::new(AppState::new())))
        .manage(SymbolTableWrapper(Mutex::new(SymbolTable::new())))
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
            parse_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
