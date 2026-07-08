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

    /// Record that a file was opened. Creates initial base-state node if tree is empty.
    pub fn record_file_open(&mut self) {
        if self.undo_tree.is_empty() {
            self.undo_tree.push_initial();
        }
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

    /// Return a real unified diff between current state and the state at target node.
    /// Clone current state, replay jump_to(target) on clone, diff all buffers.
    pub fn node_content_diff(&self, node_id: NodeId) -> String {
        // Clone self and jump clone to target
        let mut clone = self.clone();
        let commands = match clone.undo_tree.jump_to(node_id) {
            Some(cmds) => cmds,
            None => return String::from("(node not found)"),
        };
        for cmd in &commands {
            clone.execute(cmd);
        }

        // Diff all buffers: current vs target
        let mut diff_output = String::new();
        let mut all_paths: Vec<&PathBuf> = self.buffers.keys().collect();
        for p in clone.buffers.keys() {
            if !all_paths.contains(&p) {
                all_paths.push(p);
            }
        }
        all_paths.sort();

        for path in all_paths {
            let current = self.buffers.get(path).map(|b| b.content.as_str()).unwrap_or("");
            let target = clone.buffers.get(path).map(|b| b.content.as_str()).unwrap_or("");
            if current == target {
                continue;
            }
            diff_output.push_str(&format!("--- a/{}\n+++ b/{}\n", path.display(), path.display()));
            // Simple line-based unified diff
            let cur_lines: Vec<&str> = current.lines().collect();
            let tar_lines: Vec<&str> = target.lines().collect();
            diff_output.push_str(&simple_unified_diff(&cur_lines, &tar_lines));
        }

        if diff_output.is_empty() {
            "(no changes)".to_string()
        } else {
            diff_output
        }
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
                let lines: Vec<&str> = text.lines().take(15).collect();
                let mut out = format!("--- {}\n+++ {} @offset {}\n", file.display(), file.display(), offset);
                for line in &lines {
                    out.push_str(&format!("+{}\n", line));
                }
                if text.lines().count() > 15 {
                    out.push_str(&format!("... (+{} more lines)\n", text.lines().count() - 15));
                }
                out
            }
            Command::Delete { file, offset, deleted_text, .. } => {
                let lines: Vec<&str> = deleted_text.lines().take(15).collect();
                let mut out = format!("--- {} @offset {}\n", file.display(), offset);
                for line in &lines {
                    out.push_str(&format!("-{}\n", line));
                }
                if deleted_text.lines().count() > 15 {
                    out.push_str(&format!("... (-{} more lines)\n", deleted_text.lines().count() - 15));
                }
                out
            }
            Command::CreateFile { path } => format!("+++ new file: {}\n", path.display()),
            Command::DeleteFile { path, .. } => format!("--- deleted: {}\n", path.display()),
            Command::RenameFile { from, to } => format!("rename: {} → {}\n", from.display(), to.display()),
            Command::SetCursor { .. } | Command::SetSelection { .. } => String::new(),
            Command::Batch { commands } => {
                let mut out = String::new();
                for (i, c) in commands.iter().take(10).enumerate() {
                    let sub = Self::command_diff_text(c);
                    if !sub.is_empty() {
                        if i > 0 { out.push_str("\n"); }
                        out.push_str(&sub);
                    }
                }
                if commands.len() > 10 {
                    out.push_str(&format!("\n... (+{} more operations)\n", commands.len() - 10));
                }
                out
            }
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple line-based unified diff (Myers-like LCS approach).
/// Produces @@ hunks with context lines.
fn simple_unified_diff(a: &[&str], b: &[&str]) -> String {
    // LCS-based diff: find longest common subsequence indices
    let n = a.len();
    let m = b.len();

    // For small files, use full DP. For large files, just show all changes.
    if n + m > 10_000 {
        // Fallback: show everything as remove+add
        let mut out = String::new();
        out.push_str(&format!("@@ -1,{} +1,{} @@\n", n, m));
        for line in a {
            out.push_str(&format!("-{}\n", line));
        }
        for line in b {
            out.push_str(&format!("+{}\n", line));
        }
        return out;
    }

    // Build edit script via DP
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            if a[i] == b[j] {
                dp[i][j] = dp[i + 1][j + 1] + 1;
            } else {
                dp[i][j] = dp[i + 1][j].max(dp[i][j + 1]);
            }
        }
    }

    // Generate edit operations
    #[derive(Clone, Copy)]
    enum Op { Keep, Remove, Add }
    let mut ops: Vec<(Op, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((Op::Keep, a[i]));
            i += 1;
            j += 1;
        } else if j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j]) {
            ops.push((Op::Add, b[j]));
            j += 1;
        } else {
            ops.push((Op::Remove, a[i]));
            i += 1;
        }
    }

    // Format as unified diff hunks (3 lines context)
    let context = 3;
    let mut out = String::new();
    let mut idx = 0;
    while idx < ops.len() {
        // Find next change
        let change_start = match ops[idx..].iter().position(|(op, _)| !matches!(op, Op::Keep)) {
            Some(pos) => idx + pos,
            None => break,
        };

        // Hunk start with context
        let hunk_start = change_start.saturating_sub(context);

        // Find end of this hunk (include trailing context, merge nearby changes)
        let mut hunk_end = change_start;
        loop {
            // Skip past changes
            while hunk_end < ops.len() && !matches!(ops[hunk_end].0, Op::Keep) {
                hunk_end += 1;
            }
            // Check if next change is within context range
            let next_change = ops[hunk_end..].iter().position(|(op, _)| !matches!(op, Op::Keep));
            match next_change {
                Some(pos) if pos <= context * 2 => {
                    hunk_end += pos;
                }
                _ => break,
            }
        }
        // Add trailing context
        hunk_end = (hunk_end + context).min(ops.len());

        // Calculate line numbers
        let mut a_start = 1usize;
        let mut b_start = 1usize;
        for op in &ops[..hunk_start] {
            match op.0 {
                Op::Keep => { a_start += 1; b_start += 1; }
                Op::Remove => { a_start += 1; }
                Op::Add => { b_start += 1; }
            }
        }
        let mut a_count = 0usize;
        let mut b_count = 0usize;
        for op in &ops[hunk_start..hunk_end] {
            match op.0 {
                Op::Keep => { a_count += 1; b_count += 1; }
                Op::Remove => { a_count += 1; }
                Op::Add => { b_count += 1; }
            }
        }

        out.push_str(&format!("@@ -{},{} +{},{} @@\n", a_start, a_count, b_start, b_count));
        for &(op, line) in &ops[hunk_start..hunk_end] {
            match op {
                Op::Keep => out.push_str(&format!(" {}\n", line)),
                Op::Remove => out.push_str(&format!("-{}\n", line)),
                Op::Add => out.push_str(&format!("+{}\n", line)),
            }
        }

        idx = hunk_end;
    }

    out
}


