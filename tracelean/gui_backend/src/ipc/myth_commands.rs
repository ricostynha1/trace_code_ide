//! Myth IPC commands — structured surfaces, capture→action dispatch, and the
//! keymap mode machine. Thin bridge: all behavior lives in core::myth.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, State};
use tracelean_core::myth::{ActionCtx, ActionOutcome, FileTreeSurface, KeyResult, Keymap, SurfaceView};

use crate::service;
use crate::{invalidate_undo_cache, AppStateWrapper, UndoTreeCacheWrapper};

/// The keymap, loaded once at startup from ui_settings/keymap.json.
pub struct MythKeymapWrapper(pub Keymap);

/// Current keymap mode ("Main" at startup); one global mode machine.
pub struct MythKeymapStateWrapper(pub Mutex<String>);

/// Actions + node info available at a position in a loaded file (editor
/// context menus / which-key over code).
#[tauri::command]
pub fn list_actions_at(
    state: State<'_, AppStateWrapper>,
    file: String,
    char_pos: usize,
) -> Result<serde_json::Value, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let path = PathBuf::from(&file);
    let content = s
        .get_content(&path)
        .ok_or_else(|| format!("file not loaded: {}", file))?;
    let node = tracelean_core::parser::node_at(&path, content, char_pos);
    let actions = tracelean_core::parser::actions_at(&path, content, char_pos);
    Ok(serde_json::json!({ "actions": actions, "node": node }))
}

/// Dispatch a registry action. `Commands` outcomes are applied through the
/// same service path as user edits (undo tree, integrity checks, disk sync);
/// `Ui` outcomes are returned for the frontend to interpret.
#[tauri::command]
pub fn dispatch_action(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    name: String,
    ctx: ActionCtx,
) -> Result<serde_json::Value, String> {
    let outcome = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        tracelean_core::myth::actions::registry().dispatch(&name, &s, &ctx)?
    };

    if let ActionOutcome::Commands { commands } = &outcome {
        {
            let mut s = state.0.lock().map_err(|e| e.to_string())?;
            for cmd in commands.clone() {
                service::apply_command(&mut s, cmd)?;
            }
            super::editor::sync_file_operations_to_disk(&s);
        }
        invalidate_undo_cache(&cache);
        let _ = app.emit("undo-tree-changed", ());
        let _ = app.emit("files-changed", ());
    }

    serde_json::to_value(&outcome).map_err(|e| e.to_string())
}

/// Feed one key event into the keymap mode machine. Returns the interpreted
/// result, the new mode, and that mode's bindings (which-key data).
#[tauri::command]
pub fn myth_key_event(
    keymap: State<'_, MythKeymapWrapper>,
    mode: State<'_, MythKeymapStateWrapper>,
    key: String,
) -> Result<serde_json::Value, String> {
    let mut cur = mode.0.lock().map_err(|e| e.to_string())?;
    let result = keymap.0.interpret(&cur, &key);
    let new_state = match &result {
        KeyResult::Transition { state } => state.clone(),
        KeyResult::Dispatch { next_state, .. } => next_state.clone(),
        KeyResult::Passthrough => cur.clone(),
    };
    *cur = new_state.clone();
    let bindings = keymap.0.bindings_for_state(&new_state);
    Ok(serde_json::json!({
        "result": result,
        "state": new_state,
        "bindings": bindings,
    }))
}

/// Which-key: the bindings available in a keymap state.
#[tauri::command]
pub fn myth_which_key(
    keymap: State<'_, MythKeymapWrapper>,
    state: String,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(keymap.0.bindings_for_state(&state)).map_err(|e| e.to_string())
}

/// A surface snapshot: content + parsed nodes + capture bindings.
#[tauri::command]
pub fn get_surface(
    state: State<'_, AppStateWrapper>,
    id: String,
) -> Result<SurfaceView, String> {
    match id.as_str() {
        "file_tree" => {
            let root = {
                let s = state.0.lock().map_err(|e| e.to_string())?;
                s.project_root()
                    .cloned()
                    .ok_or_else(|| "no project open".to_string())?
            };
            Ok(FileTreeSurface::build(&root))
        }
        other => Err(format!("unknown surface `{}`", other)),
    }
}
