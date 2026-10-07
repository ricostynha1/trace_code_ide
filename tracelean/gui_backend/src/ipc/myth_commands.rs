//! Myth IPC commands — structured surfaces, capture→action dispatch, and the
//! keymap mode machine. Thin bridge: all behavior lives in core::myth.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, State};
use tracelean_core::lsp::LspRegistry;
use tracelean_core::myth::provider::{self, ActionProvider, CursorCx};
use tracelean_core::myth::{
    ActionCtx, ActionOutcome, FileTreeSurface, Keymap, ModeStack, SurfaceView,
};

use crate::service;
use crate::{invalidate_undo_cache, AppStateWrapper, UndoTreeCacheWrapper};

/// The keymap, loaded once at startup from ui_settings/keymap.json.
pub struct MythKeymapWrapper(pub Keymap);

/// Current keymap mode; one global mode machine, with a stack so `Escape`
/// leaves one level rather than dropping to `Main` from any depth.
pub struct MythModeStackWrapper(pub Mutex<ModeStack>);

/// The language-server registry, created lazily for the open project.
///
/// It is long-lived on purpose: the agent-facing `lsp_query` tool starts a
/// fresh registry per call because it is stateless by design, but the editor
/// asks for code actions on a keystroke, and paying a rust-analyzer start each
/// time would make the feature unusable.
#[derive(Default)]
pub struct LspRegistryWrapper(pub Mutex<Option<(PathBuf, Arc<LspRegistry>)>>);

impl LspRegistryWrapper {
    fn for_root(&self, root: &std::path::Path) -> Result<Arc<LspRegistry>, String> {
        let mut slot = self.0.lock().map_err(|e| e.to_string())?;
        if let Some((existing, registry)) = slot.as_ref() {
            if existing == root {
                return Ok(registry.clone());
            }
            // Project changed: the old servers are indexing the wrong tree.
            registry.shutdown_all();
        }
        let registry = Arc::new(LspRegistry::new(root));
        *slot = Some((root.to_path_buf(), registry.clone()));
        Ok(registry)
    }
}

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
    mode: State<'_, MythModeStackWrapper>,
    key: String,
) -> Result<serde_json::Value, String> {
    let mut stack = mode.0.lock().map_err(|e| e.to_string())?;
    let result = keymap.0.interpret(stack.current(), &key);
    let new_state = stack.apply(&result).to_string();
    let bindings = keymap.0.bindings_for_state(&new_state);
    Ok(serde_json::json!({
        "result": result,
        "state": new_state,
        "path": stack.path(),
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

/// Actions available at the cursor, from every provider.
///
/// This is the dynamic counterpart to `bindings.json`: the static capture map
/// answers "what can you do to a node of this kind", while LSP and the trace
/// index answer "what can you do *here*". `include_lsp` is a parameter because
/// a code-action round trip costs a server call, and the file tree should not
/// pay it.
#[tauri::command]
pub fn myth_actions_at(
    state: State<'_, AppStateWrapper>,
    lsp: State<'_, LspRegistryWrapper>,
    file: String,
    char_pos: usize,
    include_lsp: bool,
) -> Result<serde_json::Value, String> {
    let (content, root, captures, node_text) = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let path = PathBuf::from(&file);
        let content = s
            .get_content(&path)
            .ok_or_else(|| format!("file not loaded: {}", file))?
            .to_string();
        let root = s.project_root().cloned();
        let node = tracelean_core::parser::node_at(&path, &content, char_pos);
        let captures = node
            .as_ref()
            .map(|n| n.captures.clone())
            .unwrap_or_default();
        let node_text = node.map(|n| n.text).unwrap_or_default();
        (content, root, captures, node_text)
    };

    let path = PathBuf::from(&file);
    let mut cx = CursorCx::at_offset(&path, &content, char_pos);
    cx.captures = captures;
    cx.node_text = node_text;

    let static_provider = provider::StaticProvider;
    let mut providers: Vec<&dyn ActionProvider> = vec![&static_provider];

    // Built outside the `if` so the borrows outlive the `collect` call.
    let index;
    let trace_provider;
    let registry;
    let lsp_provider;

    if let Some(root) = root.as_ref() {
        index = tracelean_core::trace::build(root);
        trace_provider = provider::TraceActionProvider { index: &index };
        providers.push(&trace_provider);

        if include_lsp {
            registry = lsp.for_root(root)?;
            lsp_provider = provider::LspActionProvider {
                registry: &registry,
                content: &content,
            };
            providers.push(&lsp_provider);
        }
    }

    let actions = provider::collect(&providers, &cx);
    let (bindings, dropped) = tracelean_core::myth::keymap::dynamic_bindings(&actions);
    Ok(serde_json::json!({
        "actions": actions,
        "groups": provider::grouped(actions.clone()),
        "bindings": bindings,
        "dropped": dropped,
    }))
}

/// Why is a language server not answering? Reported explicitly so "no actions
/// here" and "rust-analyzer is not installed" never look the same.
#[tauri::command]
pub fn lsp_status(
    state: State<'_, AppStateWrapper>,
    lsp: State<'_, LspRegistryWrapper>,
) -> Result<serde_json::Value, String> {
    let root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root()
            .cloned()
            .ok_or_else(|| "no project open".to_string())?
    };
    let registry = lsp.for_root(&root)?;
    let statuses: Vec<_> = [
        tracelean_core::lsp::ServerKind::Rust,
        tracelean_core::lsp::ServerKind::Lean,
        tracelean_core::lsp::ServerKind::Python,
    ]
    .iter()
    .map(|kind| registry.status(*kind))
    .collect();
    serde_json::to_value(statuses).map_err(|e| e.to_string())
}

/// Everything a Lean infoview shows at one position, in one round trip.
///
/// Four separate calls per cursor move would be four subprocess round trips on
/// a keystroke-driven surface. They are batched here instead.
///
/// The critical field is `status`. With no Lean toolchain installed, the goal
/// list must never simply come back empty: **an empty goal list means the proof
/// is complete**, and rendering that for a server that never started would be
/// exactly the kind of green-when-unchecked lie this project exists to prevent.
#[tauri::command]
pub fn lsp_goal_at(
    state: State<'_, AppStateWrapper>,
    lsp: State<'_, LspRegistryWrapper>,
    file: String,
    char_pos: usize,
) -> Result<serde_json::Value, String> {
    use tracelean_core::lsp::{Position, QueryMode, ServerKind};

    let (content, root) = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let path = PathBuf::from(&file);
        let content = s
            .get_content(&path)
            .ok_or_else(|| format!("file not loaded: {}", file))?
            .to_string();
        let root = s
            .project_root()
            .cloned()
            .ok_or_else(|| "no project open".to_string())?;
        (content, root)
    };

    let path = PathBuf::from(&file);
    let kind = ServerKind::for_path(&path);
    let registry = lsp.for_root(&root)?;
    let status = kind.map(|k| registry.status(k));

    // Opening can fail (no server on PATH). That is a status, not an error: the
    // panel still has to render, saying why it is empty.
    if registry.open(&path, &content).is_err() {
        return Ok(serde_json::json!({
            "status": status,
            "goals": serde_json::Value::Null,
            "term_goal": serde_json::Value::Null,
            "diagnostics": [],
            "line": 0,
        }));
    }

    let (line, character) = provider::line_character(&content, char_pos);
    let at = Position { line, character };

    let ask = |mode: QueryMode| registry.query(&path, at, mode).ok();

    // Lean answers exactly one of these depending on where the cursor is, so
    // both are asked and the panel shows whichever came back.
    let goals = if kind == Some(ServerKind::Lean) { ask(QueryMode::Goal) } else { None };
    let term_goal = if kind == Some(ServerKind::Lean) { ask(QueryMode::TermGoal) } else { None };
    let diagnostics = ask(QueryMode::Diagnostics).unwrap_or_else(|| serde_json::json!([]));

    Ok(serde_json::json!({
        "status": status,
        "goals": goals,
        "term_goal": term_goal,
        "diagnostics": diagnostics,
        "line": line,
    }))
}

/// The AST node under the cursor — capture names, kind, range, text.
///
/// Split out from `list_actions_at` so a caller that gets its actions from the
/// providers can still label the menu with what the cursor is on.
#[tauri::command]
pub fn myth_node_at(
    state: State<'_, AppStateWrapper>,
    file: String,
    char_pos: usize,
) -> Result<Option<tracelean_core::parser::NodeRefInfo>, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let path = PathBuf::from(&file);
    let content = s
        .get_content(&path)
        .ok_or_else(|| format!("file not loaded: {}", file))?;
    Ok(tracelean_core::parser::node_at(&path, content, char_pos))
}

/// Ask the language server one question at a position.
///
/// The editor-side twin of the agent's `lsp_query` tool. It differs in one way
/// that matters: it reuses the long-lived registry and the *buffer* contents,
/// so the answer reflects what is on screen rather than what was last saved.
#[tauri::command]
pub fn lsp_query(
    state: State<'_, AppStateWrapper>,
    lsp: State<'_, LspRegistryWrapper>,
    file: String,
    char_pos: usize,
    mode: String,
) -> Result<serde_json::Value, String> {
    let query_mode = tracelean_core::lsp::QueryMode::parse(&mode)
        .ok_or_else(|| format!("unknown lsp query mode `{}`", mode))?;
    let (content, root) = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let path = PathBuf::from(&file);
        let content = s
            .get_content(&path)
            .ok_or_else(|| format!("file not loaded: {}", file))?
            .to_string();
        let root = s
            .project_root()
            .cloned()
            .ok_or_else(|| "no project open".to_string())?;
        (content, root)
    };
    let path = PathBuf::from(&file);
    let registry = lsp.for_root(&root)?;
    registry.open(&path, &content).map_err(|e| e.to_string())?;
    let (line, character) = provider::line_character(&content, char_pos);
    registry
        .query(
            &path,
            tracelean_core::lsp::Position { line, character },
            query_mode,
        )
        .map_err(|e| e.to_string())
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
