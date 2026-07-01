//! AppState: the central application state.
//! Has exactly ONE mutation path: apply(Command) -> Inverse.
//! No other way to mutate state exists.

use crate::commands::{Command, Position, Range};
use crate::undo_tree::{UndoTree, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// In-memory representation of a file's content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBuffer {
    pub content: String,
    pub cursor: Position,
    pub selection: Option<Range>,
}

impl FileBuffer {
    pub fn new(content: String) -> Self {
        Self {
            content,
            cursor: Position { line: 0, col: 0 },
            selection: None,
        }
    }

    pub fn empty() -> Self {
        Self::new(String::new())
    }
}

/// The application state. All fields are private to enforce command-only mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    /// Open file buffers (path -> content)
    buffers: HashMap<PathBuf, FileBuffer>,
    /// The undo tree
    undo_tree: UndoTree,
    /// Command log (all commands ever applied, for persistence)
    command_log: Vec<Command>,
    /// Project root directory
    project_root: Option<PathBuf>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            undo_tree: UndoTree::new(),
            command_log: Vec::new(),
            project_root: None,
        }
    }

    /// THE ONLY WAY to mutate state. No other mutation path exists.
    /// Returns the inverse command for undo.
    pub fn apply(&mut self, cmd: Command) -> Command {
        let inverse = cmd.inverse();
        self.execute(&cmd);
        self.undo_tree.push(cmd.clone(), inverse.clone());
        self.command_log.push(cmd);
        inverse
    }

    /// Execute a command's effect on state (internal only).
    fn execute(&mut self, cmd: &Command) {
        match cmd {
            Command::Insert { file, offset, text } => {
                let buffer = self.get_or_create_buffer(file);
                if *offset <= buffer.content.len() {
                    buffer.content.insert_str(*offset, text);
                }
            }
            Command::Delete {
                file, offset, len, ..
            } => {
                let buffer = self.get_or_create_buffer(file);
                let end = (*offset + *len).min(buffer.content.len());
                if *offset <= buffer.content.len() {
                    buffer.content.drain(*offset..end);
                }
            }
            Command::Replace {
                file,
                offset,
                old_text,
                new_text,
            } => {
                let buffer = self.get_or_create_buffer(file);
                let end = (*offset + old_text.len()).min(buffer.content.len());
                if *offset <= buffer.content.len() {
                    buffer.content.drain(*offset..end);
                    buffer.content.insert_str(*offset, new_text);
                }
            }
            Command::SetCursor { file, new_pos, .. } => {
                let buffer = self.get_or_create_buffer(file);
                buffer.cursor = new_pos.clone();
            }
            Command::SetSelection {
                file, new_range, ..
            } => {
                let buffer = self.get_or_create_buffer(file);
                buffer.selection = Some(new_range.clone());
            }
            Command::CreateFile { path } => {
                self.buffers.insert(path.clone(), FileBuffer::empty());
            }
            Command::DeleteFile { path, .. } => {
                self.buffers.remove(path);
            }
            Command::RenameFile { from, to } => {
                if let Some(buffer) = self.buffers.remove(from) {
                    self.buffers.insert(to.clone(), buffer);
                }
            }
            Command::Batch { commands } => {
                for c in commands {
                    self.execute(c);
                }
            }
        }
    }

    /// Undo: apply the inverse of the current command, move back in tree.
    pub fn undo(&mut self) -> bool {
        if let Some(inverse) = self.undo_tree.undo().cloned() {
            self.execute(&inverse);
            true
        } else {
            false
        }
    }

    /// Redo: reapply the forward command, move forward in tree.
    pub fn redo(&mut self) -> bool {
        if let Some(cmd) = self.undo_tree.redo().cloned() {
            self.execute(&cmd);
            true
        } else {
            false
        }
    }

    /// Get file content (read-only)
    pub fn get_content(&self, path: &PathBuf) -> Option<&str> {
        self.buffers.get(path).map(|b| b.content.as_str())
    }

    /// Get buffer (read-only)
    pub fn get_buffer(&self, path: &PathBuf) -> Option<&FileBuffer> {
        self.buffers.get(path)
    }

    /// List all open buffers
    pub fn open_files(&self) -> Vec<&PathBuf> {
        self.buffers.keys().collect()
    }

    /// Set project root (not a command — initialization only)
    pub fn set_project_root(&mut self, root: PathBuf) {
        self.project_root = Some(root);
    }

    pub fn project_root(&self) -> Option<&PathBuf> {
        self.project_root.as_ref()
    }

    /// Load file content into buffer (initialization, not a command)
    pub fn load_file(&mut self, path: PathBuf, content: String) {
        self.buffers.insert(path, FileBuffer::new(content));
    }

    /// Get the undo tree (read-only, for visualization)
    pub fn undo_tree(&self) -> &UndoTree {
        &self.undo_tree
    }

    /// Get command log (for persistence)
    pub fn command_log(&self) -> &[Command] {
        &self.command_log
    }

    /// Replay commands (for startup from persisted log)
    pub fn replay(&mut self, commands: Vec<Command>) {
        for cmd in commands {
            self.apply(cmd);
        }
    }

    /// Jump to a specific undo-tree node. Returns commands needed for traversal.
    pub fn jump_to_node(&mut self, target: NodeId) -> Option<Vec<Command>> {
        self.undo_tree.jump_to(target)
    }

    /// Execute a command without recording it (used during tree traversal)
    pub fn execute_raw(&mut self, cmd: &Command) {
        self.execute(cmd);
    }

    /// Clear undo tree and command log (reset history)
    pub fn clear_history(&mut self) {
        self.undo_tree = UndoTree::new();
        self.command_log.clear();
    }

    fn get_or_create_buffer(&mut self, path: &PathBuf) -> &mut FileBuffer {
        self.buffers
            .entry(path.clone())
            .or_insert_with(FileBuffer::empty)
    }

    /// Return a human-readable diff summary for a given undo node.
    pub fn node_diff_summary(&self, node_id: NodeId) -> String {
        if let Some(node) = self.undo_tree.get_node(node_id) {
            Self::command_diff_text(&node.command)
        } else {
            String::from("(node not found)")
        }
    }

    fn command_diff_text(cmd: &Command) -> String {
        match cmd {
            Command::Insert { file, offset, text } => {
                let preview = if text.len() > 80 { &text[..80] } else { text };
                format!("+ {} @{}: \"{}\"", file.display(), offset, preview)
            }
            Command::Delete { file, offset, deleted_text, .. } => {
                let preview = if deleted_text.len() > 80 { &deleted_text[..80] } else { deleted_text };
                format!("- {} @{}: \"{}\"", file.display(), offset, preview)
            }
            Command::Replace { file, offset, old_text, new_text } => {
                let old_p = if old_text.len() > 40 { &old_text[..40] } else { old_text };
                let new_p = if new_text.len() > 40 { &new_text[..40] } else { new_text };
                format!("~ {} @{}: \"{}\" → \"{}\"", file.display(), offset, old_p, new_p)
            }
            Command::CreateFile { path } => format!("+ new file: {}", path.display()),
            Command::DeleteFile { path, .. } => format!("- del file: {}", path.display()),
            Command::RenameFile { from, to } => format!("→ rename: {} → {}", from.display(), to.display()),
            Command::SetCursor { file, new_pos, .. } => format!("cursor {} L{}:C{}", file.display(), new_pos.line, new_pos.col),
            Command::SetSelection { file, new_range, .. } => format!("select {} L{}:C{}-L{}:C{}", file.display(), new_range.start.line, new_range.start.col, new_range.end.line, new_range.end.col),
            Command::Batch { commands } => {
                let summaries: Vec<String> = commands.iter().take(5).map(|c| Self::command_diff_text(c)).collect();
                let suffix = if commands.len() > 5 { format!(" (+{} more)", commands.len() - 5) } else { String::new() };
                format!("batch[{}]:{}{}", commands.len(), summaries.join("; "), suffix)
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}


