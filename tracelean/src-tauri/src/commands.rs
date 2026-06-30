//! Command types — every mutation to AppState is one of these.
//! Each variant carries enough data to compute its inverse.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Position in a file (line, column)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub col: u32,
}

/// Text range (start inclusive, end exclusive)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// Every state mutation is a Command. No exceptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    // --- Text editing ---
    Insert {
        file: PathBuf,
        offset: usize,
        text: String,
    },
    Delete {
        file: PathBuf,
        offset: usize,
        len: usize,
        deleted_text: String,
    },
    // TO EVALUATE is this necessary? this is equal to a delete forllowing a insert
    Replace {
        file: PathBuf,
        offset: usize,
        old_text: String,
        new_text: String,
    },

    // --- Cursor / selection ---
    SetCursor {
        file: PathBuf,
        new_pos: Position,
        prev_pos: Position,
    },
    SetSelection {
        file: PathBuf,
        new_range: Range,
        prev_range: Option<Range>,
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

    // --- Multi-file atomic group ---
    Batch {
        commands: Vec<Command>,
    },
}

impl Command {
    /// Compute the inverse command. Always possible by construction.
    pub fn inverse(&self) -> Command {
        match self {
            Command::Insert { file, offset, text } => Command::Delete {
                file: file.clone(),
                offset: *offset,
                len: text.len(),
                deleted_text: text.clone(),
            },
            Command::Delete {
                file,
                offset,
                deleted_text,
                ..
            } => Command::Insert {
                file: file.clone(),
                offset: *offset,
                text: deleted_text.clone(),
            },
            Command::Replace {
                file,
                offset,
                old_text,
                new_text,
            } => Command::Replace {
                file: file.clone(),
                offset: *offset,
                old_text: new_text.clone(),
                new_text: old_text.clone(),
            },
            Command::SetCursor {
                file,
                new_pos,
                prev_pos,
            } => Command::SetCursor {
                file: file.clone(),
                new_pos: prev_pos.clone(),
                prev_pos: new_pos.clone(),
            },
            Command::SetSelection {
                file,
                new_range,
                prev_range,
            } => Command::SetSelection {
                file: file.clone(),
                new_range: prev_range.clone().unwrap_or(new_range.clone()),
                prev_range: Some(new_range.clone()),
            },
            Command::CreateFile { path } => Command::DeleteFile {
                path: path.clone(),
                content: String::new(),
            },
            Command::DeleteFile { path, content: _ } => {
                Command::CreateFile { path: path.clone() }
            }
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
}


