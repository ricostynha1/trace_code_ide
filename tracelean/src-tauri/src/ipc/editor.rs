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
) -> Result<String, String> {
    let checkpoint_info = {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        service::apply_command(&mut s, command)
    };
    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());

    if let Some((root, _)) = checkpoint_info {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let _ = persistence::save_checkpoint(&root, &s);
        let _ = persistence::save_command_log(&root, s.command_log());
    }
    Ok("ok".into())
}

#[tauri::command]
pub fn undo(
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
pub fn redo(
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
pub fn get_file_content(state: State<'_, AppStateWrapper>, path: String) -> Result<Option<String>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    Ok(s.get_content(&PathBuf::from(&path)).map(|c| c.to_string()))
}

#[tauri::command]
pub fn open_project(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    path: String,
) -> Result<Vec<String>, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let mut sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    service::open_project(&mut s, &mut sym, &path)
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
pub fn save_checkpoint(state: State<'_, AppStateWrapper>) -> Result<(), String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().ok_or("No project open")?;
    persistence::save_checkpoint(&root, &s).map_err(|e| e.to_string())
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
    Ok(s.node_diff_summary(id))
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

/// Legacy highlighter endpoint (deprecated — now redirects to query-based system).
/// Kept for dev-mode comparison logging in frontend.
#[tauri::command]
pub fn get_highlights_legacy(
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
