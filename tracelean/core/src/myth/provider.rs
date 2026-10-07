//! Dynamic action providers (Myth phase 4).
//!
//! `bindings.json` maps a capture name to a *static* list of actions. That is
//! enough for a file tree, where the set of things you can do to a file never
//! changes, and wrong for everything this project added afterwards: LSP code
//! actions and traceability actions are computed *at the cursor* and differ
//! line by line.
//!
//! So the static map becomes one `ActionProvider` among several. The contract
//! is deliberately the same shape as LSP's `textDocument/codeAction` — "named
//! operations valid at this location" — because that is Myth's model stated in
//! someone else's spec.
//!
//! Providers only *offer*; they never execute. Every offered action still goes
//! through `ActionRegistry::dispatch`, so the rule that mutating actions return
//! `Command`s is not weakened by making the menu dynamic.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One offered action.
///
/// `name` is what gets dispatched; `title`, `group` and `priority` exist for
/// which-key. Showing nine raw LSP action names in a menu bar is unusable, and
/// LSP's `kind` (`quickfix`, `refactor.extract`) already carries the grouping,
/// so the display metadata is not invented — it is passed through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    /// Registry action name.
    pub name: String,
    /// Human-readable label.
    pub title: String,
    /// Menu grouping: "file", "edit", "navigate", "quickfix", "refactor",
    /// "source", "trace", "verify".
    pub group: String,
    /// Higher sorts first. Quick fixes outrank refactors outrank everything
    /// static, because a quick fix is usually an error the user is looking at.
    pub priority: i32,
    /// Payload handed back to the action through `ActionCtx::args`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// Which provider offered it (diagnostics, and de-duplication reporting).
    pub provider: String,
}

impl Action {
    pub fn new(name: impl Into<String>, title: impl Into<String>, group: &str, priority: i32) -> Action {
        Action {
            name: name.into(),
            title: title.into(),
            group: group.to_string(),
            priority,
            args: None,
            provider: "static".to_string(),
        }
    }

    pub fn with_args(mut self, args: serde_json::Value) -> Action {
        self.args = Some(args);
        self
    }

    pub fn from_provider(mut self, provider: &str) -> Action {
        self.provider = provider.to_string();
        self
    }

    /// Identity for de-duplication: two offers of the same action with the
    /// same payload are the same offer, even from different providers.
    fn dedup_key(&self) -> String {
        match &self.args {
            Some(args) => format!("{}\u{1}{}", self.name, args),
            None => self.name.clone(),
        }
    }
}

/// Where the cursor is, in the terms every provider needs.
///
/// Both a char offset (what the editor and `parser::semantic_nav` use) and a
/// line/character pair (what LSP uses) are carried, because converting between
/// them needs the buffer and providers should not each re-do it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CursorCx {
    #[serde(default)]
    pub surface: String,
    /// Capture names of the node under the cursor, outermost first.
    #[serde(default)]
    pub captures: Vec<String>,
    #[serde(default)]
    pub node_text: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
    #[serde(default)]
    pub char_pos: Option<usize>,
    /// Zero-based line, as LSP counts.
    #[serde(default)]
    pub line: u32,
    /// Zero-based character within the line, as LSP counts.
    #[serde(default)]
    pub character: u32,
}

impl CursorCx {
    /// Build the LSP-shaped half from the editor-shaped half.
    pub fn at_offset(file: &Path, content: &str, char_pos: usize) -> CursorCx {
        let (line, character) = line_character(content, char_pos);
        CursorCx {
            surface: "editor".to_string(),
            captures: Vec::new(),
            node_text: String::new(),
            file: Some(file.to_path_buf()),
            char_pos: Some(char_pos),
            line,
            character,
        }
    }

    pub fn position(&self) -> crate::lsp::Position {
        crate::lsp::Position { line: self.line, character: self.character }
    }
}

/// Char offset → (line, UTF-16 character), LSP's coordinate system.
pub fn line_character(content: &str, char_pos: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, c) in content.chars().enumerate() {
        if i >= char_pos {
            break;
        }
        if c == '\n' {
            line += 1;
            character = 0;
        } else {
            character += c.len_utf16() as u32;
        }
    }
    (line, character)
}

/// Anything that can offer actions at a cursor.
pub trait ActionProvider {
    fn name(&self) -> &'static str;
    fn actions_at(&self, cx: &CursorCx) -> Vec<Action>;
}

// ─── The static binding map, as a provider ───────────────────────────────────

/// `bindings.json`, unchanged — the capture → action map that predates this
/// trait, now one provider among several.
#[derive(Debug, Default)]
pub struct StaticProvider;

impl ActionProvider for StaticProvider {
    fn name(&self) -> &'static str {
        "static"
    }

    fn actions_at(&self, cx: &CursorCx) -> Vec<Action> {
        super::bindings::union_actions(&cx.captures)
            .into_iter()
            .map(|name| {
                let title = title_for(&name);
                let group = group_for(&name);
                Action::new(name, title, group, 0)
            })
            .collect()
    }
}

/// Human titles for the built-in registry actions. Unknown names fall back to
/// their own name with underscores softened, so a new action is never invisible
/// in which-key just because nobody added a title for it.
pub fn title_for(name: &str) -> String {
    match name {
        "open_file" => "Open".to_string(),
        "rename_file" => "Rename…".to_string(),
        "delete_file" => "Delete".to_string(),
        "create_file" => "New file…".to_string(),
        "save_file" => "Save".to_string(),
        "undo" => "Undo".to_string(),
        "redo" => "Redo".to_string(),
        "copy" => "Copy".to_string(),
        "go_parent" => "Parent node".to_string(),
        "go_child" => "Child node".to_string(),
        "go_prev_sibling" => "Previous sibling".to_string(),
        "go_next_sibling" => "Next sibling".to_string(),
        "lsp_definition" => "Go to definition".to_string(),
        "lsp_references" => "Find references".to_string(),
        "lsp_symbols" => "Document symbols".to_string(),
        "lsp_hover" => "Hover".to_string(),
        "next_diagnostic" => "Next problem".to_string(),
        "prev_diagnostic" => "Previous problem".to_string(),
        "lsp_code_action" => "Apply code action".to_string(),
        "show_goal" => "Lean goal at cursor".to_string(),
        "run_drt" => "Differential-test this binding".to_string(),
        "run_judge" => "Re-judge requirement ↔ model".to_string(),
        "replay_witness" => "Replay last witness".to_string(),
        "lake_build" => "lake build".to_string(),
        "goto_requirement" => "Go to requirement".to_string(),
        "goto_model" => "Go to Lean model".to_string(),
        "goto_code" => "Go to implementation".to_string(),
        "goto_tests" => "Go to tests".to_string(),
        "goto_harness" => "Go to DRT harness".to_string(),
        "show_evidence" => "Show evidence record".to_string(),
        "goto_provenance" => "Who wrote this line".to_string(),
        "annotate" => "Add annotation…".to_string(),
        "zoom_in" => "Zoom in".to_string(),
        "zoom_out" => "Zoom out".to_string(),
        "zoom_level" => "Set zoom level".to_string(),
        "coverage_map" => "Coverage map".to_string(),
        "explain_gap" => "Why is this not green?".to_string(),
        other => {
            let mut s = other.replace('_', " ");
            if let Some(first) = s.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            s
        }
    }
}

fn group_for(name: &str) -> &'static str {
    match name {
        "open_file" | "rename_file" | "delete_file" | "create_file" | "save_file" => "file",
        "undo" | "redo" | "copy" => "edit",
        n if n.starts_with("go_") || n.starts_with("goto_") || n.starts_with("lsp_") => "navigate",
        n if n.starts_with("run_") || n == "show_goal" || n == "lake_build" || n == "replay_witness" => "verify",
        n if n.starts_with("zoom") || n == "annotate" || n == "show_evidence" || n == "coverage_map" || n == "explain_gap" => "trace",
        _ => "other",
    }
}

// ─── LSP code actions, as a provider ─────────────────────────────────────────

/// Offers `textDocument/codeAction` results at the cursor.
///
/// A missing or crashed server yields an empty list, never an error: which-key
/// must still open when rust-analyzer is not installed. The *reason* it is
/// empty is reported separately through `LspRegistry::status`, so "no actions
/// here" and "no server at all" stay distinguishable — silently identical
/// behaviour for those two cases is how an IDE teaches you not to trust it.
pub struct LspActionProvider<'a> {
    pub registry: &'a crate::lsp::LspRegistry,
    /// Buffer contents, so the document can be opened before querying.
    pub content: &'a str,
}

impl<'a> ActionProvider for LspActionProvider<'a> {
    fn name(&self) -> &'static str {
        "lsp"
    }

    fn actions_at(&self, cx: &CursorCx) -> Vec<Action> {
        let Some(file) = cx.file.as_ref() else { return Vec::new() };
        if self.registry.open(file, self.content).is_err() {
            return Vec::new();
        }
        let Ok(actions) = self.registry.code_actions(file, cx.position()) else {
            return Vec::new();
        };
        actions
            .into_iter()
            .map(|action| {
                let group = code_action_group(action.kind.as_deref());
                let priority = code_action_priority(action.kind.as_deref());
                Action::new("lsp_code_action", action.title.clone(), group, priority)
                    .with_args(serde_json::json!({
                        "title": action.title,
                        "kind": action.kind,
                        "edit": action.edit,
                        "command": action.command,
                        "file": file,
                    }))
                    .from_provider("lsp")
            })
            .collect()
    }
}

fn code_action_group(kind: Option<&str>) -> &'static str {
    match kind {
        Some(k) if k.starts_with("quickfix") => "quickfix",
        Some(k) if k.starts_with("refactor") => "refactor",
        Some(k) if k.starts_with("source") => "source",
        _ => "quickfix",
    }
}

fn code_action_priority(kind: Option<&str>) -> i32 {
    match kind {
        Some(k) if k.starts_with("quickfix") => 30,
        Some(k) if k.starts_with("refactor") => 20,
        Some(k) if k.starts_with("source") => 10,
        // An untyped action is usually a server's "Try this" suggestion (Lean
        // does exactly that), which is worth as much as a quick fix.
        _ => 25,
    }
}

// ─── Traceability, as a provider ─────────────────────────────────────────────

/// Offers the trace actions that make sense *here*.
///
/// The distinction that matters: on an annotated declaration you get
/// navigation and evidence; on an unannotated one you get `annotate`. Offering
/// "go to requirement" on a line with no requirement would be the kind of dead
/// menu entry that makes a feature feel decorative.
pub struct TraceActionProvider<'a> {
    pub index: &'a crate::trace::TraceIndex,
}

impl<'a> ActionProvider for TraceActionProvider<'a> {
    fn name(&self) -> &'static str {
        "trace"
    }

    fn actions_at(&self, cx: &CursorCx) -> Vec<Action> {
        let Some(file) = cx.file.as_ref() else { return Vec::new() };
        let line = cx.line as usize;
        let mut here: Vec<&crate::trace::Link> = self
            .index
            .links
            .iter()
            .filter(|l| l.anchor.file == *file && l.anchor.covers_line(line))
            .collect();
        // Innermost first: a region inside a declaration is the more specific
        // answer to "what am I looking at".
        here.sort_by_key(|l| l.anchor.span_len());

        let mut out = Vec::new();
        let Some(link) = here.first() else {
            return vec![Action::new("annotate", "Add annotation here…", "trace", 5)
                .with_args(serde_json::json!({ "file": file, "line": cx.line }))
                .from_provider("trace")];
        };

        let req = link.req_id.clone();
        let args = serde_json::json!({ "req_id": req, "file": file, "line": cx.line });
        for (name, priority) in [
            ("goto_requirement", 40),
            ("goto_model", 38),
            ("goto_code", 36),
            ("goto_tests", 34),
            ("goto_harness", 32),
            ("show_evidence", 30),
            ("explain_gap", 28),
            ("goto_provenance", 26),
        ] {
            out.push(
                Action::new(name, title_for(name), group_for(name), priority)
                    .with_args(args.clone())
                    .from_provider("trace"),
            );
        }
        out.push(
            Action::new("annotate", "Add another annotation here…", "trace", 5)
                .with_args(serde_json::json!({ "file": file, "line": cx.line }))
                .from_provider("trace"),
        );
        out
    }
}

// ─── Collection ──────────────────────────────────────────────────────────────

/// Ask every provider, de-duplicate, and order for display.
///
/// Ordering is by priority descending and then stable by provider order, so
/// which-key's layout is a pure function of the cursor — a menu whose entries
/// move between invocations is a menu nobody learns.
pub fn collect(providers: &[&dyn ActionProvider], cx: &CursorCx) -> Vec<Action> {
    let mut out: Vec<Action> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for provider in providers {
        for action in provider.actions_at(cx) {
            let key = action.dedup_key();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(action);
        }
    }
    out.sort_by(|a, b| b.priority.cmp(&a.priority));
    out
}

/// Group the collected actions for a which-key panel, groups in a fixed order
/// so the panel does not reshuffle as providers come and go.
pub fn grouped(actions: Vec<Action>) -> Vec<(String, Vec<Action>)> {
    const ORDER: &[&str] = &[
        "quickfix", "refactor", "source", "trace", "verify", "navigate", "edit", "file", "other",
    ];
    let mut out: Vec<(String, Vec<Action>)> = Vec::new();
    for group in ORDER {
        let members: Vec<Action> = actions.iter().filter(|a| a.group == *group).cloned().collect();
        if !members.is_empty() {
            out.push((group.to_string(), members));
        }
    }
    // Anything with a group nobody listed still has to appear.
    let listed: Vec<String> = out.iter().map(|(g, _)| g.clone()).collect();
    let mut rest: Vec<Action> = actions
        .into_iter()
        .filter(|a| !listed.contains(&a.group) && !ORDER.contains(&a.group.as_str()))
        .collect();
    if !rest.is_empty() {
        rest.sort_by(|a, b| a.group.cmp(&b.group));
        out.push(("other".to_string(), rest));
    }
    out
}
