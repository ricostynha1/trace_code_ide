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
    // Goto mode — LSP navigation.
    actions.insert("lsp_definition", act_lsp_definition);
    actions.insert("lsp_references", act_lsp_references);
    actions.insert("lsp_symbols", act_lsp_symbols);
    actions.insert("lsp_hover", act_lsp_hover);
    actions.insert("next_diagnostic", act_next_diagnostic);
    actions.insert("prev_diagnostic", act_prev_diagnostic);
    actions.insert("lsp_code_action", act_lsp_code_action);
    // Verify mode — the Lean model and the two bonds.
    actions.insert("show_goal", act_show_goal);
    actions.insert("run_drt", act_run_drt);
    actions.insert("run_judge", act_run_judge);
    actions.insert("replay_witness", act_replay_witness);
    actions.insert("lake_build", act_lake_build);
    // Trace mode — the uniquely-TraceLean surface.
    actions.insert("goto_requirement", act_goto_requirement);
    actions.insert("goto_model", act_goto_model);
    actions.insert("goto_code", act_goto_code);
    actions.insert("goto_tests", act_goto_tests);
    actions.insert("goto_harness", act_goto_harness);
    actions.insert("show_evidence", act_show_evidence);
    actions.insert("goto_provenance", act_goto_provenance);
    actions.insert("annotate", act_annotate);
    actions.insert("zoom_in", act_zoom_in);
    actions.insert("zoom_out", act_zoom_out);
    actions.insert("zoom_level", act_zoom_level);
    actions.insert("coverage_map", act_coverage_map);
    actions.insert("explain_gap", act_explain_gap);
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


// ─── Goto mode: LSP navigation ───────────────────────────────────────────────
//
// These actions do not call the language server themselves. `ActionFn` is
// synchronous and takes only `&AppState`, while the server registry lives in
// the app layer and its queries block on a child process — so an action emits
// the *request* and the frontend runs it through the existing `lsp_query`
// command. That is the same split `undo` and `save_file` already use, and it
// keeps the registry free of I/O that a unit test would have to stub.

fn lsp_request(mode: &str, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let file = ctx_file(ctx)?;
    ui(serde_json::json!({
        "kind": "lsp_query",
        "mode": mode,
        "file": file,
        "char_pos": ctx.char_pos,
    }))
}

fn act_lsp_definition(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    lsp_request("definition", ctx)
}

fn act_lsp_references(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    lsp_request("references", ctx)
}

fn act_lsp_symbols(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    lsp_request("symbols", ctx)
}

fn act_lsp_hover(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    lsp_request("hover", ctx)
}

fn act_show_goal(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    lsp_request("goal", ctx)
}

fn diagnostic_step(ctx: &ActionCtx, direction: i32) -> Result<ActionOutcome, String> {
    let file = ctx_file(ctx)?;
    ui(serde_json::json!({
        "kind": "step_diagnostic",
        "file": file,
        "from": ctx.char_pos,
        "direction": direction,
    }))
}

fn act_next_diagnostic(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    diagnostic_step(ctx, 1)
}

fn act_prev_diagnostic(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    diagnostic_step(ctx, -1)
}

/// Apply a code action offered by `LspActionProvider`.
///
/// The provider put the server's raw `WorkspaceEdit` in `args.edit`; lowering
/// it here is what makes an LSP quick fix land in the undo tree as ordinary
/// invertible commands. An action that carries a server-side `command` instead
/// of an edit is refused rather than half-run — executing it needs a round trip
/// the registry owns, and pretending it succeeded would leave the buffer and
/// the history disagreeing.
fn act_lsp_code_action(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let args = ctx
        .args
        .as_ref()
        .ok_or_else(|| "lsp_code_action needs the action payload in args".to_string())?;
    let Some(edit) = args.get("edit").filter(|v| !v.is_null()) else {
        if args.get("command").map(|c| !c.is_null()).unwrap_or(false) {
            return Err(
                "this code action runs a server-side command; apply it from the LSP panel"
                    .to_string(),
            );
        }
        return Err("code action carries no edit".to_string());
    };
    let root = state
        .project_root()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    let command = crate::lsp::lower_workspace_edit(
        edit,
        &root,
        crate::lsp::PositionEncoding::default(),
        &|path: &std::path::Path| state.get_content(&path.to_path_buf()).map(|c| c.to_string()),
    )
    .map_err(|e| e.to_string())?;
    Ok(ActionOutcome::Commands { commands: vec![command] })
}

// ─── Verify mode ─────────────────────────────────────────────────────────────

/// Which requirement/clause is the cursor inside? Answered from the annotation
/// index so `run_drt`/`run_judge` act on what you are looking at rather than on
/// a requirement you have to name.
fn link_at_cursor(state: &AppState, ctx: &ActionCtx) -> Option<crate::trace::Link> {
    let root = state.project_root()?;
    let file = ctx.file.clone()?;
    let content = state.get_content(&file)?;
    let pos = ctx.char_pos?;
    let (line, _) = super::provider::line_character(content, pos);
    let index = crate::trace::build(root);
    let mut here: Vec<&crate::trace::Link> = index
        .links
        .iter()
        .filter(|l| l.anchor.file == file && l.anchor.covers_line(line as usize))
        .collect();
    here.sort_by_key(|l| l.anchor.span_len());
    here.first().map(|l| (*l).clone())
}

fn req_of(state: &AppState, ctx: &ActionCtx) -> Result<(String, Option<String>), String> {
    if let Some(id) = arg_str(ctx, "req_id") {
        return Ok((id, arg_str(ctx, "clause")));
    }
    let link = link_at_cursor(state, ctx)
        .ok_or_else(|| "no annotation at the cursor — nothing to verify here".to_string())?;
    Ok((link.req_id, link.clause))
}

fn act_run_drt(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let (req_id, clause) = req_of(state, ctx)?;
    ui(serde_json::json!({ "kind": "run_drt", "req_id": req_id, "clause": clause }))
}

fn act_run_judge(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let (req_id, clause) = req_of(state, ctx)?;
    ui(serde_json::json!({ "kind": "run_judge", "req_id": req_id, "clause": clause }))
}

fn act_replay_witness(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let (req_id, clause) = req_of(state, ctx)?;
    ui(serde_json::json!({ "kind": "replay_witness", "req_id": req_id, "clause": clause }))
}

fn act_lake_build(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({ "kind": "lake_build", "file": ctx.file }))
}

// ─── Trace mode ──────────────────────────────────────────────────────────────

/// Jump to the first link with `role` for the requirement at the cursor.
fn goto_role(
    state: &AppState,
    ctx: &ActionCtx,
    role: crate::trace::Role,
    what: &str,
) -> Result<ActionOutcome, String> {
    let (req_id, _) = req_of(state, ctx)?;
    let root = state
        .project_root()
        .ok_or_else(|| "no project open".to_string())?;
    let index = crate::trace::build(root);
    let target = index
        .links
        .iter()
        .find(|l| l.req_id == req_id && l.role == role);
    match target {
        Some(link) => ui(serde_json::json!({
            "kind": "reveal",
            "file": link.anchor.file,
            "line": link.anchor.start_line,
            "why": format!("{} for {}", what, req_id),
        })),
        // An empty result is a finding, not a failure: "nothing implements this
        // requirement" is exactly what the user pressed the key to learn.
        None => ui(serde_json::json!({
            "kind": "no_target",
            "req_id": req_id,
            "what": what,
            "message": format!("{} has no {} yet", req_id, what),
        })),
    }
}

fn act_goto_requirement(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let (req_id, _) = req_of(state, ctx)?;
    let root = state
        .project_root()
        .ok_or_else(|| "no project open".to_string())?;
    let index = crate::trace::build(root);
    match index.requirements.get(&req_id) {
        Some(req) => ui(serde_json::json!({
            "kind": "reveal",
            "file": req.file,
            "line": 0,
            "why": format!("requirement {}", req_id),
        })),
        None => ui(serde_json::json!({
            "kind": "no_target",
            "req_id": req_id,
            "what": "requirement",
            "message": format!("{} is annotated but no requirement file declares it", req_id),
        })),
    }
}

fn act_goto_model(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    goto_role(state, ctx, crate::trace::Role::Models, "Lean model")
}

fn act_goto_code(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    goto_role(state, ctx, crate::trace::Role::Implements, "implementation")
}

fn act_goto_tests(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    goto_role(state, ctx, crate::trace::Role::Tests, "test")
}

fn act_goto_harness(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    goto_role(state, ctx, crate::trace::Role::Drt, "DRT harness")
}

fn act_show_evidence(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let (req_id, clause) = req_of(state, ctx)?;
    ui(serde_json::json!({ "kind": "show_evidence", "req_id": req_id, "clause": clause }))
}

fn act_explain_gap(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let (req_id, clause) = req_of(state, ctx)?;
    ui(serde_json::json!({ "kind": "explain_gap", "req_id": req_id, "clause": clause }))
}

/// `t u` — jump the undo tree to the edit that wrote the line under the cursor.
fn act_goto_provenance(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let file = ctx_file(ctx)?;
    let pos = ctx
        .char_pos
        .ok_or_else(|| "goto_provenance needs char_pos".to_string())?;
    match crate::provenance::provenance_at(state.undo_tree(), &file, pos) {
        Some(p) => ui(serde_json::json!({ "kind": "goto_node", "provenance": p })),
        None => ui(serde_json::json!({
            "kind": "no_target",
            "what": "provenance",
            "message": "this text arrived with the file; no edit in this history wrote it",
        })),
    }
}

/// Offer an annotation to insert. The action deliberately stops at *offering*:
/// writing `@implements REQ-4` for a requirement the user did not pick would be
/// the tool fabricating a traceability claim, which is the one thing this
/// subsystem exists to prevent.
fn act_annotate(state: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let file = ctx_file(ctx)?;
    let root = state
        .project_root()
        .ok_or_else(|| "no project open".to_string())?;
    let index = crate::trace::build(root);
    let candidates: Vec<serde_json::Value> = index
        .requirements
        .values()
        .map(|r| serde_json::json!({ "id": r.id, "title": r.title }))
        .collect();
    let suggested_role = match file.extension().and_then(|e| e.to_str()) {
        Some("lean") => "models",
        _ => "implements",
    };
    ui(serde_json::json!({
        "kind": "annotate",
        "file": file,
        "char_pos": ctx.char_pos,
        "suggested_role": suggested_role,
        "requirements": candidates,
    }))
}

fn act_zoom_in(_s: &AppState, _ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({ "kind": "zoom", "delta": 1 }))
}

fn act_zoom_out(_s: &AppState, _ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({ "kind": "zoom", "delta": -1 }))
}

fn act_zoom_level(_s: &AppState, ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    let level = ctx
        .args
        .as_ref()
        .and_then(|a| a.get("level"))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "zoom_level needs args.level (1-5)".to_string())?;
    if !(1..=5).contains(&level) {
        return Err(format!("zoom level {} is outside 1-5", level));
    }
    ui(serde_json::json!({ "kind": "zoom", "level": level }))
}

fn act_coverage_map(_s: &AppState, _ctx: &ActionCtx) -> Result<ActionOutcome, String> {
    ui(serde_json::json!({ "kind": "coverage_map" }))
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
