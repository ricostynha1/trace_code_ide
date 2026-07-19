//! Editor IPC commands — file ops, undo/redo, parsing, highlights.

use crate::commands::Command;
use crate::parser::{self, Symbol};
use crate::persistence;
use crate::service;
use crate::{
    AppStateWrapper, FileEntry, SymbolTableWrapper, UndoTreeCacheWrapper,
    UndoTreeView, UndoNodeView, CommandLogEntry,
    invalidate_undo_cache, command_summary, command_file, command_affects_file,
};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn apply_command(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    command: Command,
) -> Result<service::ApplyResult, String> {
    let result = {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        service::apply_command(&mut s, command)
    };
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            // Witness mismatch: state refused the edit. Tell the UI to resync.
            let _ = app.emit("state-integrity-error", &e);
            return Err(e);
        }
    };
    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());

    if let Some((root, _)) = result.checkpoint.as_ref() {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let _ = persistence::save_checkpoint(root, &s);
        let _ = persistence::save_command_log(root, s.command_log());
    }
    Ok(result)
}

/// Sync filesystem for file-level operations (CreateFile/DeleteFile/RenameFile) after undo/redo.
/// Ensures files in buffers exist on disk, and files removed from buffers are deleted from disk.
/// Only touches files tracked in the undo tree — not a full filesystem scan.
pub(crate) fn sync_file_operations_to_disk(s: &crate::state::AppState) {
    let root = match s.project_root() {
        Some(r) => r.clone(),
        None => return,
    };

    // Collect all file paths mentioned in file-level commands across the undo tree
    let tree = s.undo_tree();
    let mut tracked_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for node in tree.nodes() {
        collect_file_op_paths(&node.command, &mut tracked_paths);
    }

    // Reconcile: for each tracked path, disk should match buffer state
    for path in &tracked_paths {
        let full = root.join(path);
        let in_buffer = s.get_content(path);
        match in_buffer {
            Some(content) => {
                // File should exist on disk with this content
                if let Some(parent) = full.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&full, content);
            }
            None => {
                // File should NOT exist on disk
                if full.exists() {
                    let _ = std::fs::remove_file(&full);
                }
            }
        }
    }
}

/// Collect paths from CreateFile/DeleteFile/RenameFile commands.
fn collect_file_op_paths(cmd: &Command, paths: &mut std::collections::HashSet<PathBuf>) {
    match cmd {
        Command::CreateFile { path } => { paths.insert(path.clone()); }
        Command::DeleteFile { path, .. } => { paths.insert(path.clone()); }
        Command::RenameFile { from, to } => { paths.insert(from.clone()); paths.insert(to.clone()); }
        Command::Batch { commands } => {
            for c in commands {
                collect_file_op_paths(c, paths);
            }
        }
        _ => {}
    }
}

#[tauri::command]
pub fn undo(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
) -> Result<crate::core::state::EditOutcome, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let result = s.undo();
    if result.changed {
        sync_file_operations_to_disk(&s);
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
        let _ = app.emit("files-changed", ());
    }
    Ok(result)
}

#[tauri::command]
pub fn redo(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
) -> Result<crate::core::state::EditOutcome, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let result = s.redo();
    if result.changed {
        sync_file_operations_to_disk(&s);
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
        let _ = app.emit("files-changed", ());
    }
    Ok(result)
}

#[tauri::command]
pub fn get_file_content(state: State<'_, AppStateWrapper>, path: String) -> Result<Option<String>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    Ok(s.get_content(&PathBuf::from(&path)).map(|c| c.to_string()))
}

#[tauri::command]
pub fn open_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    ai_settings: State<'_, crate::AiSettingsWrapper>,
    path: String,
) -> Result<Vec<String>, String> {
    let result = {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
        service::open_project(&mut s, &mut sym, &path)?
    };
    // Project-scoped config wins over the home fallback loaded at startup.
    if let Some(project_settings) =
        crate::core::ai::service::load_settings_from(std::path::Path::new(&path))
    {
        if let Ok(mut s) = ai_settings.0.lock() {
            *s = project_settings;
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn list_files(state: State<'_, AppStateWrapper>, path: String) -> Result<Vec<FileEntry>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().unwrap_or_default();
    Ok(service::list_directory_files(&root, &path))
}

#[tauri::command]
pub fn open_file(state: State<'_, AppStateWrapper>, path: String) -> Result<String, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    service::open_file(&mut s, &path)
}

#[tauri::command]
pub fn save_file(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<(), String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::save_file(&s, &mut sym, &path)
}

#[tauri::command]
pub fn save_checkpoint(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
) -> Result<(), String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().ok_or("No project open")?;
    // Bug 7: a checkpoint is this app's commit — mark it so the Commits
    // filter in the undo-tree panel has something to show.
    s.mark_commit_point(format!(
        "checkpoint {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    ));
    persistence::save_checkpoint(&root, &s).map_err(|e| e.to_string())?;
    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());
    Ok(())
}

#[tauri::command]
pub fn get_undo_tree(
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    file_filter: Option<String>,
) -> Result<UndoTreeView, String> {
    let mut c = cache.0.lock().map_err(|e| e.to_string())?;
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let tree_version = s.undo_tree().len() as u64;

    if c.version == tree_version && c.file_filter == file_filter {
        if let Some(ref view) = c.view {
            return Ok(view.clone());
        }
    }

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
pub fn get_command_log(
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
pub fn jump_to_node(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    node_id: String,
) -> Result<crate::core::state::EditOutcome, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::parse_str(&node_id).map_err(|e| e.to_string())?;
    if let Some(commands) = s.jump_to_node(id) {
        let cursor = commands.iter().rev().find_map(|c| c.cursor_after());
        for cmd in commands {
            s.execute_raw(&cmd);
        }
        sync_file_operations_to_disk(&s);
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
        let _ = app.emit("files-changed", ());
        Ok(crate::core::state::EditOutcome { changed: true, cursor })
    } else {
        Ok(crate::core::state::EditOutcome { changed: false, cursor: None })
    }
}

#[tauri::command]
pub fn clear_undo_tree(
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
pub fn get_undo_node_diff(
    state: State<'_, AppStateWrapper>,
    node_id: String,
) -> Result<String, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::parse_str(&node_id).map_err(|e| e.to_string())?;
    Ok(s.node_content_diff(id))
}

/// Structured hover diff (P5, D5.1): what changes if we jump to `node_id`.
#[tauri::command]
pub fn get_undo_node_diff_structured(
    state: State<'_, AppStateWrapper>,
    node_id: String,
) -> Result<crate::core::state::NodeDiff, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::parse_str(&node_id).map_err(|e| e.to_string())?;
    s.node_diff_structured(id)
        .ok_or_else(|| "node not found".to_string())
}

#[tauri::command]
pub fn parse_file_symbols(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<Symbol>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::parse_file_symbols(&s, &mut sym, &path)
}

#[tauri::command]
pub fn get_file_symbols(symbols_state: State<'_, SymbolTableWrapper>, path: String) -> Result<Vec<Symbol>, String> {
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    Ok(sym.get_symbols(&PathBuf::from(&path)).cloned().unwrap_or_default())
}

/// Query-based highlighting (new default): .scm queries + theme map color resolution.
#[tauri::command]
pub fn get_highlights(
    state: State<'_, AppStateWrapper>,
    path: String,
) -> Result<Vec<parser::HighlightSpan>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let rel_path = PathBuf::from(&path);

    let content = if let Some(c) = s.get_content(&rel_path) {
        c.to_string()
    } else {
        let root = s.project_root().cloned().unwrap_or_default();
        std::fs::read_to_string(root.join(&path))
            .map_err(|e| format!("Read error: {}", e))?
    };

    Ok(parser::get_highlights_query(&rel_path, &content))
}

#[tauri::command]
pub fn parse_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
) -> Result<usize, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::parse_project(&s, &mut sym)
}

#[tauri::command]
pub fn get_initial_project() -> Option<String> {
    let p = std::path::Path::new("/project");
    if p.is_dir() {
        Some("/project".to_string())
    } else {
        None
    }
}
