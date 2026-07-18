//! Command types — every mutation to AppState is one of these.
//! Each variant carries enough data to compute its inverse.
//! The ONLY text-edit primitive is `Replace`. Insert = Replace with old=="",
//! Delete = Replace with new=="". Positions are Unicode-scalar (char) indices —
//! never bytes, never UTF-16 code units.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Every state mutation is a Command. No exceptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    /// The single text-editing primitive.
    /// `old` is a verification witness: execution asserts the buffer contains
    /// exactly `old` at char index `at`, and refuses the edit otherwise.
    /// Inverse: swap `old` and `new`.
    Replace {
        file: PathBuf,
        /// Unicode-scalar (char) index into the buffer.
        at: usize,
        /// Text currently at `at` (may be empty for pure insert).
        old: String,
        /// Replacement text (may be empty for pure delete).
        new: String,
    },

    // --- File operations ---
    CreateFile {
        path: PathBuf,
    },
    DeleteFile {
        path: PathBuf,
        content: String,
    },
    RenameFile {
        from: PathBuf,
        to: PathBuf,
    },

    // --- Multi-command atomic group ---
    Batch {
        commands: Vec<Command>,
    },
}

impl Command {
    /// Compute the inverse command. Always possible by construction.
    pub fn inverse(&self) -> Command {
        match self {
            Command::Replace { file, at, old, new } => Command::Replace {
                file: file.clone(),
                at: *at,
                old: new.clone(),
                new: old.clone(),
            },
            Command::CreateFile { path } => Command::DeleteFile {
                path: path.clone(),
                content: String::new(),
            },
            Command::DeleteFile { path, content } => Command::Batch {
                commands: vec![
                    Command::CreateFile { path: path.clone() },
                    Command::Replace {
                        file: path.clone(),
                        at: 0,
                        old: String::new(),
                        new: content.clone(),
                    },
                ],
            },
            Command::RenameFile { from, to } => Command::RenameFile {
                from: to.clone(),
                to: from.clone(),
            },
            Command::Batch { commands } => Command::Batch {
                commands: commands.iter().rev().map(|c| c.inverse()).collect(),
            },
        }
    }

    /// Unique ID for command tracking
    pub fn id(&self) -> Uuid {
        Uuid::new_v4()
    }

    /// Helper: text replacement at a char index. `at` is a Unicode-scalar index.
    pub fn replace(file: PathBuf, at: usize, old: String, new: String) -> Command {
        Command::Replace { file, at, old, new }
    }

    /// Helper: pure insert at a char index.
    pub fn insert(file: PathBuf, at: usize, text: String) -> Command {
        Command::Replace { file, at, old: String::new(), new: text }
    }

    /// Helper: pure delete of `old` at a char index.
    pub fn delete(file: PathBuf, at: usize, old: String) -> Command {
        Command::Replace { file, at, old, new: String::new() }
    }
}

/// Where the cursor should land after a command executes (char index).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorHint {
    pub file: PathBuf,
    pub char_pos: usize,
}

impl Command {
    /// Derived cursor position after executing this command:
    /// end of the inserted text of the last text edit. None for pure file ops.
    pub fn cursor_after(&self) -> Option<CursorHint> {
        match self {
            Command::Replace { file, at, new, .. } => Some(CursorHint {
                file: file.clone(),
                char_pos: at + new.chars().count(),
            }),
            Command::Batch { commands } => {
                commands.iter().rev().find_map(|c| c.cursor_after())
            }
            _ => None,
        }
    }
}

/// Convert a char (Unicode-scalar) index into a byte index of `s`.
/// Indices past the end clamp to `s.len()`.
pub fn byte_index_at_char(s: &str, char_index: usize) -> usize {
    s.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}
