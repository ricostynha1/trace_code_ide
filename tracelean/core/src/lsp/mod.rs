//! Language-server integration.
//!
//! The premise of this project is a Lean model and an implementation open side
//! by side, so the client is a *registry* of servers keyed by language rather
//! than one server. Two consumers share it: the editor, and the agent — an
//! agent writing a Lean proof without the goal state at the cursor is
//! guessing, and a goal costs a couple of hundred tokens against thousands for
//! re-reading the file.

pub mod edits;
pub mod registry;
pub mod transport;

pub use edits::{lower_workspace_edit, LowerError, PositionEncoding};
pub use registry::{LspRegistry, ServerKind, ServerStatus};
pub use transport::{LspClient, LspError, Position};

use serde::{Deserialize, Serialize};

/// What the agent-facing `lsp_query` tool can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMode {
    Hover,
    Definition,
    References,
    Diagnostics,
    Symbols,
    CodeActions,
    /// Lean's `$/lean/plainGoal` — the proof state at the cursor. The single
    /// most useful thing a model can be told while writing Lean.
    Goal,
    /// Lean's `$/lean/plainTermGoal` — the expected type at a term position.
    ///
    /// Separate from `Goal` because Lean answers exactly one of them: inside a
    /// tactic block `plainGoal` has the answer and `plainTermGoal` is null, and
    /// outside one it is the other way round. A panel that only ever asked for
    /// tactic goals would be blank everywhere except inside `by`.
    TermGoal,
}

impl QueryMode {
    pub fn parse(s: &str) -> Option<QueryMode> {
        Some(match s {
            "hover" => QueryMode::Hover,
            "definition" => QueryMode::Definition,
            "references" => QueryMode::References,
            "diagnostics" => QueryMode::Diagnostics,
            "symbols" => QueryMode::Symbols,
            "code_actions" => QueryMode::CodeActions,
            "goal" => QueryMode::Goal,
            "term_goal" => QueryMode::TermGoal,
            _ => return None,
        })
    }
}

/// A diagnostic, flattened out of the LSP payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: std::path::PathBuf,
    pub line: u32,
    pub character: u32,
    pub severity: String,
    pub message: String,
    pub source: Option<String>,
}

/// A named operation valid at a location — LSP's own description of a code
/// action, and exactly the shape Myth's capture→actions→which-key model wants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAction {
    pub title: String,
    /// `quickfix`, `refactor.extract`, … — used for grouping in which-key.
    pub kind: Option<String>,
    /// The edit, kept raw so it can be lowered into invertible commands.
    pub edit: Option<serde_json::Value>,
    pub command: Option<serde_json::Value>,
}
