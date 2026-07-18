//! Action registry (Myth phase 2).
//!
//! An action is a named Rust function in a static registry. The critical
//! rule: **actions that mutate state return `Command`s; they never mutate
//! directly.** That routes every surface's behavior — file tree, menu,
//! keybinding, agent — through the same invertible-command/undo-tree path.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::commands::Command;
use crate::state::AppState;

/// Context the dispatcher passes to an action.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ActionCtx {
    /// Surface the action was triggered from ("editor", "file_tree", …).
    #[serde(default)]
    pub surface: String,
    /// Capture name of the node under the cursor (e.g. "function", "file").
    #[serde(default)]
    pub capture: String,
    /// Text of the node.
    #[serde(default)]
    pub node_text: String,
    /// File the node refers to (editor file, or the path a tree node names).
    #[serde(default)]
    pub file: Option<PathBuf>,
    /// Cursor char position within the surface content (editor surfaces).
    #[serde(default)]
    pub char_pos: Option<usize>,
    /// Extra arguments (e.g. `{"new_name": "..."}` for rename).
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

/// What an action produced.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionOutcome {
    /// State mutations — the dispatcher applies them via `AppState::apply`,
    /// so they land in the undo tree.
    Commands { commands: Vec<Command> },
    /// UI-only effect, interpreted by the frontend (open a file, move the
    /// selection, run undo/redo through the existing IPC…). No state change.
    Ui { effect: serde_json::Value },
    /// Nothing to do.
    None,
}

pub type ActionFn = fn(&AppState, &ActionCtx) -> Result<ActionOutcome, String>;

/// Static action registry (fable Q4): built once at startup; unknown names
/// in binding maps / keymaps are a validation error, not a runtime surprise.
pub struct ActionRegistry {
    actions: HashMap<&'static str, ActionFn>,
}

impl ActionRegistry {
    pub fn contains(&self, name: &str) -> bool {
        self.actions.contains_key(name)
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.actions.keys().copied().collect();
        names.sort();
        names
    }

    pub fn dispatch(
        &self,
        name: &str,
        state: &AppState,
        ctx: &ActionCtx,
    ) -> Result<ActionOutcome, String> {
        let f = self
            .actions
            .get(name)
            .ok_or_else(|| format!("unknown action `{}`", name))?;
        f(state, ctx)
    }
}

/// The global registry, built lazily.
pub fn registry() -> &'static ActionRegistry {
    static REGISTRY: std::sync::OnceLock<ActionRegistry> = std::sync::OnceLock::new();
    REGISTRY.get_or_init(register_all)
}

fn register_all() -> ActionRegistry {
    let mut actions: HashMap<&'static str, ActionFn> = HashMap::new();
    actions.insert("open_file", act_open_file);
    actions.insert("rename_file", act_rename_file);
    actions.insert("delete_file", act_delete_file);
    actions.insert("create_file", act_create_file);
    actions.insert("save_file", act_save_file);
    actions.insert("undo", act_undo);
    actions.insert("redo", act_redo);
    actions.insert("copy", act_copy);
    actions.insert("go_parent", act_go_parent);
    actions.insert("go_child", act_go_child);
    actions.insert("go_prev_sibling", act_go_prev_sibling);
    actions.insert("go_next_sibling", act_go_next_sibling);
    ActionRegistry { actions }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

fn ctx_file(ctx: &ActionCtx) -> Result<PathBuf, String> {
    ctx.file
        .clone()
        .ok_or_else(|| "action needs a file in context".to_string())
}

fn arg_str(ctx: &ActionCtx, key: &str) -> Option<String> {
    ctx.args
        .as_ref()
        .and_then(|a| a.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn ui(effect: serde_json::Value) -> Result<ActionOutcome, String> {
    Ok(ActionOutcome::Ui { effect })
}

// ─── UI-effect actions ───────────────────────────────────────────────────────

fn act_open_file(_state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let file = ctx_file(ctx)?;
    ui(serde_json::json!({"kind": "open_file", "path": file}))
}

fn act_save_file(_state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({"kind": "save_file", "path": ctx.file}))
}

fn act_undo(_state: &AppState, _ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({"kind": "undo"}))
}

fn act_redo(_state: &AppState, _ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({"kind": "redo"}))
}

fn act_copy(_state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({"kind": "copy", "text": ctx.node_text}))
}

// ─── Command-producing actions (undoable for free) ───────────────────────────

fn act_rename_file(_state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let from = ctx_file(ctx)?;
    let new_name =
        arg_str(ctx, "new_name").ok_or_else(|| "rename_file needs args.new_name".to_string())?;
    if new_name.trim().is_empty() {
        return Err("rename_file: new name is empty".into());
    }
    let to = match from.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(new_name.trim()),
        _ => PathBuf::from(new_name.trim()),
    };
    Ok(ActionOutcome::Commands {
        commands: vec![Command::RenameFile { from, to }],
    })
}

fn act_delete_file(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let path = ctx_file(ctx)?;
    // Witness content: buffer if loaded, else disk (relative to project root).
    let content = match state.get_content(&path) {
        Some(c) => c.to_string(),
        None => {
            let abs = state
                .project_root()
                .map(|r| r.join(&path))
                .unwrap_or_else(|| path.clone());
            std::fs::read_to_string(&abs).unwrap_or_default()
        }
    };
    Ok(ActionOutcome::Commands {
        commands: vec![Command::DeleteFile { path, content }],
    })
}

fn act_create_file(_state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let path = arg_str(ctx, "path")
        .map(PathBuf::from)
        .or_else(|| ctx.file.clone())
        .ok_or_else(|| "create_file needs args.path".to_string())?;
    Ok(ActionOutcome::Commands {
        commands: vec![Command::CreateFile { path }],
    })
}

// ─── Semantic navigation (the cursor is on an AST node) ──────────────────────

fn semantic_nav(
    state: &AppState,
    ctx: &ActionCtx,
    dir: crate::parser::NavDirection,
) -> Result<ActionOutcome, String> {
    let file = ctx_file(ctx)?;
    let pos = ctx
        .char_pos
        .ok_or_else(|| "semantic navigation needs char_pos".to_string())?;
    let content = state
        .get_content(&file)
        .ok_or_else(|| format!("file not loaded: {}", file.display()))?;
    match crate::parser::semantic_nav(&file, content, pos, dir) {
        Some((from, to)) => ui(serde_json::json!({"kind": "select", "from": from, "to": to})),
        None => Ok(ActionOutcome::None),
    }
}

fn act_go_parent(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    semantic_nav(state, ctx, crate::parser::NavDirection::Parent)
}

fn act_go_child(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    semantic_nav(state, ctx, crate::parser::NavDirection::Child)
}

fn act_go_prev_sibling(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    semantic_nav(state, ctx, crate::parser::NavDirection::PrevSibling)
}

fn act_go_next_sibling(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    semantic_nav(state, ctx, crate::parser::NavDirection::NextSibling)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_core_actions() {
        let reg = registry();
        for name in ["open_file", "rename_file", "delete_file", "create_file", "undo"] {
            assert!(reg.contains(name), "missing action {}", name);
        }
    }

    #[test]
    fn shipped_binding_map_is_valid() {
        let unknown = super::super::bindings::validate_against_registry(registry());
        assert!(unknown.is_empty(), "bindings.json references unknown actions: {:?}", unknown);
    }

    #[test]
    fn rename_returns_undoable_command() {
        let state = AppState::new();
        let ctx = ActionCtx {
            surface: "file_tree".into(),
            capture: "file".into(),
            file: Some(PathBuf::from("src/old.rs")),
            args: Some(serde_json::json!({"new_name": "new.rs"})),
            ..Default::default()
        };
        let outcome = registry().dispatch("rename_file", &state, &ctx).unwrap();
        match outcome {
            ActionOutcome::Commands { commands } => {
                assert_eq!(commands.len(), 1);
                match &commands[0] {
                    Command::RenameFile { from, to } => {
                        assert_eq!(from, &PathBuf::from("src/old.rs"));
                        assert_eq!(to, &PathBuf::from("src/new.rs"));
                    }
                    other => panic!("expected RenameFile, got {:?}", other),
                }
                // Inverse exists (undo tree integration is free)
                let inv = commands[0].inverse();
                match inv {
                    Command::RenameFile { from, to } => {
                        assert_eq!(from, PathBuf::from("src/new.rs"));
                        assert_eq!(to, PathBuf::from("src/old.rs"));
                    }
                    other => panic!("expected inverse RenameFile, got {:?}", other),
                }
            }
            other => panic!("expected Commands, got {:?}", other),
        }
    }

    #[test]
    fn delete_uses_buffer_content_as_witness() {
        let mut state = AppState::new();
        let file = PathBuf::from("a.rs");
        state.load_file(file.clone(), "fn main() {}".into());
        let ctx = ActionCtx {
            file: Some(file.clone()),
            ..Default::default()
        };
        let outcome = registry().dispatch("delete_file", &state, &ctx).unwrap();
        match outcome {
            ActionOutcome::Commands { commands } => match &commands[0] {
                Command::DeleteFile { path, content } => {
                    assert_eq!(path, &file);
                    assert_eq!(content, "fn main() {}");
                }
                other => panic!("expected DeleteFile, got {:?}", other),
            },
            other => panic!("expected Commands, got {:?}", other),
        }
    }

    #[test]
    fn open_file_is_ui_effect() {
        let state = AppState::new();
        let ctx = ActionCtx {
            file: Some(PathBuf::from("src/main.rs")),
            ..Default::default()
        };
        match registry().dispatch("open_file", &state, &ctx).unwrap() {
            ActionOutcome::Ui { effect } => assert_eq!(effect["kind"], "open_file"),
            other => panic!("expected Ui, got {:?}", other),
        }
    }

    #[test]
    fn unknown_action_is_error() {
        let state = AppState::new();
        assert!(registry()
            .dispatch("bogus", &state, &ActionCtx::default())
            .is_err());
    }
}
