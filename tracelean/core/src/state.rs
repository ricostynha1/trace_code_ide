//! AppState: the central application state.
//! Has exactly ONE mutation path: apply(Command) -> Result<inverse>.
//! No other way to mutate state exists.
//!
//! All text positions are Unicode-scalar (char) indices. Byte conversion is
//! internal to this module.

use crate::commands::{byte_index_at_char, Command, CursorHint};
use crate::undo_tree::{UndoTree, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Max age of the previous edit for typing-run coalescing into one undo node.
const COALESCE_WINDOW_MS: i64 = 750;

/// In-memory representation of a file's content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBuffer {
    pub content: String,
}

impl FileBuffer {
    pub fn new(content: String) -> Self {
        Self { content }
    }

    pub fn empty() -> Self {
        Self::new(String::new())
    }
}

/// Outcome of undo/redo: whether anything changed and where the cursor lands.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditOutcome {
    pub changed: bool,
    pub cursor: Option<CursorHint>,
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
    /// Node created by the most recent apply() — coalescing target.
    /// Cleared by undo/redo/jump so we never amend into history.
    #[serde(default)]
    last_applied_node: Option<NodeId>,
    /// Runtime-only switch: disable typing-run coalescing (replay, tests).
    #[serde(skip)]
    coalesce_disabled: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            undo_tree: UndoTree::new(),
            command_log: Vec::new(),
            project_root: None,
            last_applied_node: None,
            coalesce_disabled: false,
        }
    }

    /// Enable/disable typing-run coalescing (on by default).
    pub fn set_coalescing(&mut self, on: bool) {
        self.coalesce_disabled = !on;
    }

    /// THE ONLY WAY to mutate state. Returns the inverse command for undo.
    /// Fails (without mutating) if a Replace witness does not match the buffer —
    /// that means caller and state have diverged; the caller must resync.
    pub fn apply(&mut self, cmd: Command) -> Result<Command, String> {
        // Execute with rollback: a failed sub-command un-does prior sub-commands.
        let mut applied: Vec<Command> = Vec::new();
        if let Err(e) = self.execute_checked(&cmd, &mut applied) {
            for done in applied.iter().rev() {
                let _ = self.execute_one(&done.inverse());
            }
            return Err(e);
        }

        let inverse = cmd.inverse();

        // Typing-run coalescing: merge consecutive small edits into one undo node.
        if let Some(merged) = self.try_coalesce(&cmd) {
            let merged_inverse = merged.inverse();
            if self
                .undo_tree
                .amend_current(merged.clone(), merged_inverse)
            {
                if let Some(last) = self.command_log.last_mut() {
                    *last = merged;
                }
                return Ok(inverse);
            }
        }

        let node_id = self.undo_tree.push(cmd.clone(), inverse.clone());
        self.last_applied_node = Some(node_id);
        self.command_log.push(cmd);
        Ok(inverse)
    }

    /// If `cmd` continues a typing/deleting run on the current node, return the
    /// merged command. Only pure inserts merge with pure inserts and pure
    /// deletes with pure deletes; never across files, branches, or commit points.
    fn try_coalesce(&self, cmd: &Command) -> Option<Command> {
        if self.coalesce_disabled {
            return None;
        }
        let Command::Replace { file, at, old, new } = cmd else {
            return None;
        };
        let node = self.undo_tree.current_node()?;
        // Only amend the node the previous apply() created (leaf, no commits).
        if self.last_applied_node != Some(node.id)
            || !node.children.is_empty()
            || node.commit_point.is_some()
        {
            return None;
        }
        let age_ms = (chrono::Utc::now() - node.timestamp).num_milliseconds();
        if age_ms > COALESCE_WINDOW_MS {
            return None;
        }
        let Command::Replace {
            file: pfile,
            at: pat,
            old: pold,
            new: pnew,
        } = &node.command
        else {
            return None;
        };
        if pfile != file {
            return None;
        }

        // Typing run: pure insert directly after the previous insert's end.
        if old.is_empty() && !new.is_empty() && *at == pat + pnew.chars().count() {
            return Some(Command::Replace {
                file: file.clone(),
                at: *pat,
                old: pold.clone(),
                new: format!("{}{}", pnew, new),
            });
        }
        // Backspace run: pure delete ending exactly where the previous delete started.
        if new.is_empty() && !old.is_empty() && pnew.is_empty() && !pold.is_empty() {
            if at + old.chars().count() == *pat {
                return Some(Command::Replace {
                    file: file.clone(),
                    at: *at,
                    old: format!("{}{}", old, pold),
                    new: String::new(),
                });
            }
            // Forward-delete run: same position as the previous delete.
            if at == pat {
                return Some(Command::Replace {
                    file: file.clone(),
                    at: *at,
                    old: format!("{}{}", pold, old),
                    new: String::new(),
                });
            }
        }
        None
    }

    /// Execute recursively, recording each successfully applied leaf command.
    fn execute_checked(&mut self, cmd: &Command, applied: &mut Vec<Command>) -> Result<(), String> {
        match cmd {
            Command::Batch { commands } => {
                for c in commands {
                    self.execute_checked(c, applied)?;
                }
                Ok(())
            }
            _ => {
                self.execute_one(cmd)?;
                applied.push(cmd.clone());
                Ok(())
            }
        }
    }

    /// Execute a single non-batch command's effect on state (internal only).
    fn execute_one(&mut self, cmd: &Command) -> Result<(), String> {
        match cmd {
            Command::Replace { file, at, old, new } => {
                let buffer = self.get_or_create_buffer(file);
                let start_byte = byte_index_at_char(&buffer.content, *at);
                let old_char_len = old.chars().count();
                let end_byte = byte_index_at_char(&buffer.content, *at + old_char_len);

                // Witness check: the buffer must contain exactly `old` at `at`.
                let actual = &buffer.content[start_byte..end_byte];
                if actual != old {
                    return Err(format!(
                        "integrity error in {}: expected {:?} at char {}, found {:?} — state diverged, resync required",
                        file.display(),
                        truncate_for_error(old),
                        at,
                        truncate_for_error(actual),
                    ));
                }
                buffer.content.replace_range(start_byte..end_byte, new);
                Ok(())
            }
            Command::CreateFile { path } => {
                self.buffers.insert(path.clone(), FileBuffer::empty());
                Ok(())
            }
            Command::DeleteFile { path, .. } => {
                self.buffers.remove(path);
                Ok(())
            }
            Command::RenameFile { from, to } => {
                if let Some(buffer) = self.buffers.remove(from) {
                    self.buffers.insert(to.clone(), buffer);
                }
                Ok(())
            }
            Command::Batch { commands } => {
                for c in commands {
                    self.execute_one(c)?;
                }
                Ok(())
            }
        }
    }

    /// Undo: apply the inverse of the current command, move back in tree.
    pub fn undo(&mut self) -> EditOutcome {
        self.last_applied_node = None;
        if let Some(inverse) = self.undo_tree.undo().cloned() {
            if let Err(e) = self.execute_one(&inverse) {
                eprintln!("undo integrity failure: {}", e);
            }
            EditOutcome {
                changed: true,
                cursor: inverse.cursor_after(),
            }
        } else {
            EditOutcome { changed: false, cursor: None }
        }
    }

    /// Redo: reapply the forward command, move forward in tree.
    pub fn redo(&mut self) -> EditOutcome {
        self.last_applied_node = None;
        if let Some(cmd) = self.undo_tree.redo().cloned() {
            if let Err(e) = self.execute_one(&cmd) {
                eprintln!("redo integrity failure: {}", e);
            }
            EditOutcome {
                changed: true,
                cursor: cmd.cursor_after(),
            }
        } else {
            EditOutcome { changed: false, cursor: None }
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

    /// Stable FNV-1a 32-bit hash of a buffer's content (divergence detection).
    /// Must match the frontend implementation byte-for-byte (UTF-8).
    pub fn content_hash(&self, path: &PathBuf) -> Option<u32> {
        self.get_content(path).map(|c| fnv1a_32(c.as_bytes()))
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

    /// Replay commands (for startup from persisted log).
    /// Stops at the first failing command — a failure means the log no longer
    /// matches the on-disk state it was recorded against.
    pub fn replay(&mut self, commands: Vec<Command>) {
        // No coalescing during replay: the log is already in final granularity.
        let was_disabled = self.coalesce_disabled;
        self.coalesce_disabled = true;
        for cmd in commands {
            if let Err(e) = self.apply(cmd) {
                eprintln!("replay stopped: {}", e);
                break;
            }
        }
        self.coalesce_disabled = was_disabled;
        self.last_applied_node = None;
    }

    /// Jump to a specific undo-tree node. Returns commands needed for traversal.
    pub fn jump_to_node(&mut self, target: NodeId) -> Option<Vec<Command>> {
        self.last_applied_node = None;
        self.undo_tree.jump_to(target)
    }

    /// Execute a command without recording it (used during tree traversal)
    pub fn execute_raw(&mut self, cmd: &Command) {
        if let Err(e) = self.execute_one(cmd) {
            eprintln!("traversal integrity failure: {}", e);
        }
    }

    /// Clear undo tree and command log (reset history)
    pub fn clear_history(&mut self) {
        self.undo_tree = UndoTree::new();
        self.command_log.clear();
        self.last_applied_node = None;
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
            clone.execute_raw(cmd);
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

    /// Structured diff between current state and the state at a target node
    /// (P5, D5.1). Direction: "what changes if I jump there" — removed_lines
    /// are lines present NOW that would disappear; added_lines are incoming.
    pub fn node_diff_structured(&self, node_id: NodeId) -> Option<NodeDiff> {
        let target_buffers = self.buffers_at_node(node_id)?;

        let mut all_paths: Vec<PathBuf> = self.buffers.keys().cloned().collect();
        for p in target_buffers.keys() {
            if !all_paths.contains(p) {
                all_paths.push(p.clone());
            }
        }
        all_paths.sort();

        let mut files = Vec::new();
        for path in all_paths {
            let current = self.buffers.get(&path).map(|b| b.content.as_str()).unwrap_or("");
            let target = target_buffers.get(&path).map(|s| s.as_str()).unwrap_or("");
            if current == target {
                continue;
            }
            let cur_lines: Vec<&str> = current.lines().collect();
            let tar_lines: Vec<&str> = target.lines().collect();
            let ops = diff_ops(&cur_lines, &tar_lines);

            let mut hunks: Vec<DiffHunk> = Vec::new();
            let mut added = 0u32;
            let mut removed = 0u32;
            // 1-indexed line number in the CURRENT document
            let mut cur_line = 1u32;
            let mut open: Option<DiffHunk> = None;
            for (op, line) in ops {
                match op {
                    DiffOp::Keep => {
                        if let Some(h) = open.take() {
                            hunks.push(h);
                        }
                        cur_line += 1;
                    }
                    DiffOp::Remove => {
                        removed += 1;
                        let h = open.get_or_insert_with(|| DiffHunk {
                            current_start_line: cur_line,
                            removed_lines: Vec::new(),
                            added_lines: Vec::new(),
                        });
                        h.removed_lines.push(line.to_string());
                        cur_line += 1;
                    }
                    DiffOp::Add => {
                        added += 1;
                        let h = open.get_or_insert_with(|| DiffHunk {
                            current_start_line: cur_line,
                            removed_lines: Vec::new(),
                            added_lines: Vec::new(),
                        });
                        h.added_lines.push(line.to_string());
                    }
                }
            }
            if let Some(h) = open.take() {
                hunks.push(h);
            }

            files.push(FileDiff {
                path: path.to_string_lossy().to_string(),
                added,
                removed,
                hunks,
            });
        }

        Some(NodeDiff { files })
    }

    /// Compute the buffer contents at a target node (for structured diffs).
    /// Returns (path -> content) for every buffer that differs from current.
    pub fn buffers_at_node(&self, node_id: NodeId) -> Option<HashMap<PathBuf, String>> {
        let mut clone = self.clone();
        let commands = clone.undo_tree.jump_to(node_id)?;
        for cmd in &commands {
            clone.execute_raw(cmd);
        }
        Some(
            clone
                .buffers
                .into_iter()
                .map(|(p, b)| (p, b.content))
                .collect(),
        )
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
            Command::Replace { file, at, old, new } => {
                let mut out = format!("--- {} @char {}\n", file.display(), at);
                for line in old.lines().take(15) {
                    out.push_str(&format!("-{}\n", line));
                }
                if old.lines().count() > 15 {
                    out.push_str(&format!("... (-{} more lines)\n", old.lines().count() - 15));
                }
                for line in new.lines().take(15) {
                    out.push_str(&format!("+{}\n", line));
                }
                if new.lines().count() > 15 {
                    out.push_str(&format!("... (+{} more lines)\n", new.lines().count() - 15));
                }
                out
            }
            Command::CreateFile { path } => format!("+++ new file: {}\n", path.display()),
            Command::DeleteFile { path, .. } => format!("--- deleted: {}\n", path.display()),
            Command::RenameFile { from, to } => format!("rename: {} → {}\n", from.display(), to.display()),
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

/// Structured node diff shipped to the UI (P5, D5.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeDiff {
    pub files: Vec<FileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    /// 1-indexed line in the CURRENT document where this hunk begins.
    pub current_start_line: u32,
    pub removed_lines: Vec<String>,
    pub added_lines: Vec<String>,
}

/// FNV-1a 32-bit. Mirrored in the frontend for divergence detection.
pub fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for &b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn truncate_for_error(s: &str) -> String {
    if s.chars().count() > 40 {
        let head: String = s.chars().take(40).collect();
        format!("{}…", head)
    } else {
        s.to_string()
    }
}

/// Simple line-based unified diff (Myers-like LCS approach).
/// Produces @@ hunks with context lines.
fn simple_unified_diff(a: &[&str], b: &[&str]) -> String {
    let mut out = String::new();
    for hunk in diff_hunks(a, b, 3) {
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.a_start, hunk.a_count, hunk.b_start, hunk.b_count
        ));
        for line in &hunk.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// A rendered hunk (text form) — used by the unified-diff renderer.
struct TextHunk {
    a_start: usize,
    a_count: usize,
    b_start: usize,
    b_count: usize,
    lines: Vec<String>,
}

/// Structured diff op-stream shared by the text renderer and structured IPC (P5).
#[derive(Clone, Copy, PartialEq)]
pub enum DiffOp {
    Keep,
    Remove,
    Add,
}

/// LCS diff between two line slices, as an op stream.
pub fn diff_ops<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(DiffOp, &'a str)> {
    let n = a.len();
    let m = b.len();

    // For large files, fall back to full remove+add.
    if n + m > 10_000 {
        let mut ops = Vec::with_capacity(n + m);
        for line in a {
            ops.push((DiffOp::Remove, *line));
        }
        for line in b {
            ops.push((DiffOp::Add, *line));
        }
        return ops;
    }

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

    let mut ops: Vec<(DiffOp, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((DiffOp::Keep, a[i]));
            i += 1;
            j += 1;
        } else if j < m && (i >= n || dp[i][j + 1] >= dp[i + 1][j]) {
            ops.push((DiffOp::Add, b[j]));
            j += 1;
        } else {
            ops.push((DiffOp::Remove, a[i]));
            i += 1;
        }
    }
    ops
}

fn diff_hunks(a: &[&str], b: &[&str], context: usize) -> Vec<TextHunk> {
    let ops = diff_ops(a, b);
    let mut hunks = Vec::new();
    let mut idx = 0;
    while idx < ops.len() {
        // Find next change
        let change_start = match ops[idx..].iter().position(|(op, _)| *op != DiffOp::Keep) {
            Some(pos) => idx + pos,
            None => break,
        };

        // Hunk start with context
        let hunk_start = change_start.saturating_sub(context);

        // Find end of this hunk (include trailing context, merge nearby changes)
        let mut hunk_end = change_start;
        loop {
            while hunk_end < ops.len() && ops[hunk_end].0 != DiffOp::Keep {
                hunk_end += 1;
            }
            let next_change = ops[hunk_end..].iter().position(|(op, _)| *op != DiffOp::Keep);
            match next_change {
                Some(pos) if pos <= context * 2 => {
                    hunk_end += pos;
                }
                _ => break,
            }
        }
        hunk_end = (hunk_end + context).min(ops.len());

        // Calculate line numbers
        let mut a_start = 1usize;
        let mut b_start = 1usize;
        for op in &ops[..hunk_start] {
            match op.0 {
                DiffOp::Keep => {
                    a_start += 1;
                    b_start += 1;
                }
                DiffOp::Remove => {
                    a_start += 1;
                }
                DiffOp::Add => {
                    b_start += 1;
                }
            }
        }
        let mut a_count = 0usize;
        let mut b_count = 0usize;
        let mut lines = Vec::new();
        for &(op, line) in &ops[hunk_start..hunk_end] {
            match op {
                DiffOp::Keep => {
                    a_count += 1;
                    b_count += 1;
                    lines.push(format!(" {}", line));
                }
                DiffOp::Remove => {
                    a_count += 1;
                    lines.push(format!("-{}", line));
                }
                DiffOp::Add => {
                    b_count += 1;
                    lines.push(format!("+{}", line));
                }
            }
        }

        hunks.push(TextHunk {
            a_start,
            a_count,
            b_start,
            b_count,
            lines,
        });
        idx = hunk_end;
    }
    hunks
}
