//! Surgical Editing API — syntax-aware, minimal, transactional edits.
//!
//! Architecture:
//! 1. Each edit mode (AST, Patch, Search&Replace) produces new content in a shadow buffer
//! 2. A diff between original and shadow buffer is computed
//! 3. The diff is converted to a sequence of Insert/Delete commands ONLY
//!
//! This guarantees perfect undo/redo via the existing command system.
//! No Replace commands ever. Just Insert and Delete.

pub mod ast_edit;
pub mod patch_edit;
pub mod search_replace;
pub mod policy;
pub mod error;
pub mod shadow_diff;

pub use ast_edit::{AstEdit, AstEditOp};
pub use patch_edit::PatchEdit;
pub use search_replace::SearchReplace;
pub use policy::{EditPolicy, EditMode};
pub use error::SurgicalEditError;
pub use shadow_diff::diff_to_commands;

use crate::commands::Command;
use std::path::PathBuf;

/// Result of a successful surgical edit.
/// Commands are ONLY Insert/Delete — never Replace.
#[derive(Debug, Clone)]
pub struct EditResult {
    pub file: PathBuf,
    /// Sequence of Insert/Delete commands. Apply in order = get new_content.
    pub commands: Vec<Command>,
    /// New file content after edit.
    pub new_content: String,
}
