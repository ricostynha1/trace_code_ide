//! Tool executor — runs tool calls from agents against the project environment.
//! Respects permission model (4.17). Used by both internal agents and MCP host.

use super::tools::{ToolCall, ToolResult};
use crate::commands::Command;
use crate::parser::SymbolTable;
use crate::state::AppState;
use crate::trace_graph::TraceGraph;
use std::path::{Path, PathBuf};

/// Agent permissions — which tools/files/commands an agent can access.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentPermissions {
    /// Agent identifier.
    pub agent_id: String,
    /// Allowed tool names (empty = all allowed).
    pub allowed_tools: Vec<String>,
    /// Denied tool names (takes precedence over allowed).
    pub denied_tools: Vec<String>,
    /// File path patterns the agent can read (glob). Empty = all.
    pub readable_paths: Vec<String>,
    /// File path patterns the agent can write (glob). Empty = all.
    pub writable_paths: Vec<String>,
    /// Whether shell commands are allowed.
    pub allow_shell: bool,
    /// Max shell command timeout (seconds).
    pub max_shell_timeout: u64,
    /// P10 review mode: edit tools stage diffs for user approval instead of
    /// applying directly to buffers.
    #[serde(default)]
    pub review_edits: bool,
}

impl Default for AgentPermissions {
    fn default() -> Self {
        Self {
            agent_id: "default".into(),
            allowed_tools: vec![],
            denied_tools: vec![],
            readable_paths: vec![],
            writable_paths: vec![],
            allow_shell: true,
            max_shell_timeout: 60,
            review_edits: false,
        }
    }
}

impl AgentPermissions {
    /// Full access (for trusted internal agents).
    pub fn full_access(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.into(),
            ..Default::default()
        }
    }

    /// Read-only access.
    pub fn read_only(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.into(),
            denied_tools: vec![
                "edit_file".into(),
                "str_replace".into(),
                "delete_file".into(),
                "run_shell".into(),
            ],
            allow_shell: false,
            ..Default::default()
        }
    }

    /// Check if a tool is allowed.
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if self.denied_tools.contains(&tool_name.to_string()) {
            return false;
        }
        if self.allowed_tools.is_empty() {
            return true;
        }
        self.allowed_tools.contains(&tool_name.to_string())
    }

    /// Check if a file path is readable.
    pub fn can_read(&self, path: &str) -> bool {
        if self.readable_paths.is_empty() {
            return true;
        }
        self.readable_paths.iter().any(|p| glob_match(p, path))
    }

    /// Check if a file path is writable.
    pub fn can_write(&self, path: &str) -> bool {
        if self.writable_paths.is_empty() {
            return true;
        }
        self.writable_paths.iter().any(|p| glob_match(p, path))
    }
}

/// Simple glob match (supports * and **).
fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == "*" || pattern == "**" || pattern == "**/*" {
        return true;
    }
    // Simple extension match: "*.rs"
    if let Some(ext) = pattern.strip_prefix("*.") {
        return path.ends_with(&format!(".{}", ext));
    }
    // Prefix match: "src/**"
    if let Some(prefix) = pattern.strip_suffix("/**") {
        return path.starts_with(prefix);
    }
    // Exact match
    pattern == path
}

use super::embeddings::SharedIndex;

/// P10 review mode: where edit tools stage proposed diffs instead of applying.
pub struct ReviewSink<'a> {
    pub pending: &'a mut Vec<super::diff_pipeline::PendingDiff>,
    pub agent: String,
}

impl ReviewSink<'_> {
    /// Stage a proposed full-content change and describe it to the agent.
    fn stage(&mut self, path: &str, original: &str, proposed: &str) -> ToolResult {
        let diff =
            super::diff_pipeline::create_pending_diff(path, original, proposed, &self.agent);
        let n = diff.hunks.len();
        self.pending.push(diff);
        ToolResult {
            success: true,
            content: format!(
                "Edit to '{}' staged for user review ({} hunk{} pending approval). \
                 It is NOT applied yet — the user will accept or reject hunks in the IDE. \
                 Continue with your remaining work; do not re-apply this edit.",
                path,
                n,
                if n == 1 { "" } else { "s" }
            ),
            data: None,
        }
    }
}

/// Execute a tool call. Returns the result.
/// Requires mutable access to state for write operations.
pub fn execute_tool(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    symbols: &SymbolTable,
    graph: &TraceGraph,
    permissions: &AgentPermissions,
) -> ToolResult {
    execute_tool_with_index(
        call,
        project_root,
        state,
        symbols,
        graph,
        permissions,
        &None,
    )
}

/// Execute a tool call with optional embeddings index.
pub fn execute_tool_with_index(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    symbols: &SymbolTable,
    graph: &TraceGraph,
    permissions: &AgentPermissions,
    embed_index: &Option<SharedIndex>,
) -> ToolResult {
    execute_tool_reviewed(
        call,
        project_root,
        state,
        symbols,
        graph,
        permissions,
        embed_index,
        None,
    )
}

/// Execute a tool call, staging edits into `review` instead of applying them
/// when review mode is active (P10). `review: None` = direct apply.
pub fn execute_tool_reviewed(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    symbols: &SymbolTable,
    graph: &TraceGraph,
    permissions: &AgentPermissions,
    embed_index: &Option<SharedIndex>,
    review: Option<&mut ReviewSink>,
) -> ToolResult {
    // Permission check
    if !permissions.is_tool_allowed(&call.name) {
        return ToolResult {
            success: false,
            content: format!(
                "Permission denied: tool '{}' not allowed for agent '{}'.",
                call.name, permissions.agent_id
            ),
            data: None,
        };
    }

    match call.name.as_str() {
        "read_file" => execute_read_file(call, project_root, permissions),
        "edit_file" => execute_edit_file(call, project_root, state, permissions, review),
        //
        "count_lines" => execute_count_lines(call, project_root, permissions),

        "find_grep" => execute_find_grep(call, project_root),
        "find_embed" => execute_find_embed(call, embed_index),
        // tools.json name "find" routes to grep (semantic mode handled externally)
        "find" => execute_find_grep(call, project_root),

        "str_replace" => execute_str_replace(call, project_root, state, permissions, review),
        // tools.json name "replace_str" → same as str_replace
        "replace_str" => execute_str_replace(call, project_root, state, permissions, review),

        "delete_file" => execute_delete_file(call, project_root, state, permissions),
        "list_files" => execute_list_directory(call, project_root),
        // tools.json canonical name
        "list_directory" => execute_list_directory(call, project_root),
        "query_trace_graph" => execute_query_trace(call, graph),
        "query_code_element" => execute_query_code(call, graph),
        "list_requirements" => execute_list_requirements(project_root),
        "get_symbols" => execute_get_symbols(call, symbols),
        "run_shell" => execute_run_shell(call, project_root, permissions),
        // Meta tools — handled inline
        "discover_tools" => execute_discover_tools(call),
        "help_tool" => execute_help_tool(call),
        _ => ToolResult {
            success: false,
            content: format!(
                "Unknown tool: {}. Consider using discover_tools(\"{}\") to find a tool that fulfills what you want.",
                call.name,
                call.name.replace('_', " ")
            ),
            data: None,
        },
    }
}

fn get_str_arg(call: &ToolCall, key: &str) -> Option<String> {
    call.arguments
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn get_int_arg(call: &ToolCall, key: &str) -> Option<i64> {
    call.arguments.get(key).and_then(|v| v.as_i64())
}

fn execute_read_file(call: &ToolCall, project_root: &Path, perms: &AgentPermissions) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'path' argument. Expected: read_file(path, offset?, max_results?)".into(),
                data: None,
            }
        }
    };

    if !perms.can_read(&path) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: cannot read '{}'.", path),
            data: None,
        };
    }

    let full = project_root.join(&path);
    let content = match std::fs::read_to_string(&full) {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                success: false,
                content: format!("Error reading '{}': {}", path, e),
                data: None,
            }
        }
    };

    let lines: Vec<&str> = content.lines().collect();
    let num_lines = lines.len();
    let file_size = content.len();

    // File metadata
    let modified = std::fs::metadata(&full)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let offset_raw = get_int_arg(call, "offset").unwrap_or(0);

    let max_results = get_int_arg(call, "max_results").unwrap_or(200);
    let max_results_truncated = max_results > 200;
    let max_results = max_results.min(200);

    // Resolve negative indexes
    let offset = resolve_line_idx_read(offset_raw, num_lines);

    let offset = offset.min(num_lines);

    let start = offset;
    let end_raw = start + max_results as usize;

    let end = end_raw.min(num_lines);

    let selected: Vec<&str> = lines[start..end].to_vec();

    // Build metadata header
    let meta_line = format!("[{} | {} lines | {} bytes]", path, num_lines, file_size);

    let (result_content, hint) = if selected.is_empty() {
        (
            meta_line,
            Some("Range described by offset and max_results does not contain any line".to_string()),
        )
    } else {
        let lines_text = selected
            .iter()
            .enumerate()
            .map(|(i, l)| format!("{:>4}| {}", start + i + 1, l))
            .collect::<Vec<_>>()
            .join("\n");

        let result_content = format!("{}\n{}", meta_line, lines_text);

        let hint = if max_results_truncated {
            Some(format!(
                "Result was truncated because more than 200 lines were requested. \
             To get the rest, make another read starting at offset={}.",
                start + 200
            ))
        } else if end < num_lines {
            Some(format!(
                "Showing lines {}-{} of {}. Use offset={} to read more.",
                start + 1, end, num_lines, end
            ))
        } else {
            None
        };

        (result_content, hint)
    };

    ToolResult {
        success: true,
        content: result_content,
        data: Some(serde_json::json!({
            "lines": num_lines,
            "bytes": file_size,
            "modified": modified,
            "hint": hint,
        })),
    }
}

fn execute_count_lines(
    call: &ToolCall,
    project_root: &Path,
    perms: &AgentPermissions,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'path' argument. Expected: count_lines(path)".into(),
                data: None,
            }
        }
    };

    if !perms.can_read(&path) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: cannot read '{}'.", path),
            data: None,
        };
    }

    let full = project_root.join(&path);
    match std::fs::read_to_string(&full) {
        Ok(content) => {
            let count = content.lines().count();
            ToolResult {
                success: true,
                content: format!("{} lines", count),
                data: None,
            }
        }
        Err(e) => ToolResult {
            success: false,
            content: format!("Error reading '{}': {}", path, e),
            data: None,
        },
    }
}

/// Resolve a possibly-negative line index for READING.
/// -1 = last line (num_lines-1), -2 = second-to-last, etc.
fn resolve_line_idx_read(raw: i64, num_lines: usize) -> usize {
    if raw >= 0 {
        raw as usize
    } else {
        let resolved = num_lines as i64 + raw;
        if resolved < 0 {
            0
        } else {
            resolved as usize
        }
    }
}

/// Resolve a possibly-negative line index for WRITING.
/// -1 = EOF (past last line = num_lines), -2 = before last line, etc.
fn resolve_line_idx_write(raw: i64, num_lines: usize) -> usize {
    if raw >= 0 {
        raw as usize
    } else {
        let resolved = num_lines as i64 + raw + 1;
        if resolved < 0 {
            0
        } else {
            resolved as usize
        }
    }
}

/// Writes `rel_path`'s current buffer content (read from `state`, not a
/// caller-supplied copy) to disk under `project_root`. Side-effecting;
/// name carries `_eff` suffix by convention.
fn save_eff(state: &AppState, project_root: &Path, rel_path: &Path) -> std::io::Result<()> {
    let content = state
        .get_content(&rel_path.to_path_buf())
        .unwrap_or_default();
    std::fs::write(project_root.join(rel_path), content)
}

fn execute_edit_file(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
    review: Option<&mut ReviewSink>,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'path' argument. Expected: edit_file(path, text, start?, end?) — omit start+end to replace whole file.".into(),
                data: None,
            }
        }
    };
    let text = match get_str_arg(call, "text") {
        Some(t) => t,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'text' argument. Expected: edit_file(path, text, start?, end?) — 'text' is the content to write.".into(),
                data: None,
            }
        }
    };
    // Both omitted = replace whole file (as documented in schema)
    let start_opt = get_int_arg(call, "start");
    let end_opt = get_int_arg(call, "end");
    let (start_raw, end_raw) = match (start_opt, end_opt) {
        (Some(s), Some(e)) => (s, e),
        (Some(s), None) => (s, s), // start without end = insert at start
        (None, Some(e)) => (0, e), // end without start = replace from beginning
        (None, None) => {
            // Both omitted: replace whole file content
            // Write directly — no line-range logic needed
            if !perms.can_write(&path) {
                return ToolResult {
                    success: false,
                    content: format!("Permission denied: cannot write '{}'.", path),
                    data: None,
                };
            }
            let rel_path = PathBuf::from(&path);
            let full_path = project_root.join(&path);

            // Ensure parent dirs exist
            if let Some(parent) = full_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            // Load or create file in state
            let old_content = if let Some(existing) = state.get_content(&rel_path) {
                existing.to_string()
            } else {
                match std::fs::read_to_string(&full_path) {
                    Ok(c) => {
                        state.load_file(rel_path.clone(), c.clone());
                        c
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let _ = state.apply(Command::CreateFile { path: rel_path.clone() });
                        state.load_file(rel_path.clone(), String::new());
                        String::new()
                    }
                    Err(e) => {
                        return ToolResult {
                            success: false,
                            content: format!("Cannot read '{}': {}", path, e),
                            data: None,
                        };
                    }
                }
            };

            // P10 review mode: stage the whole-file change instead of applying
            if let Some(sink) = review {
                return sink.stage(&path, &old_content, &text);
            }

            // Replace entire content
            if let Err(e) = state.apply(Command::replace(
                rel_path.clone(),
                0,
                old_content,
                text.clone(),
            )) {
                return ToolResult {
                    success: false,
                    content: format!("Edit rejected: {}", e),
                    data: None,
                };
            }

            return match save_eff(state, project_root, &rel_path) {
                Ok(_) => ToolResult {
                    success: true,
                    content: format!("Wrote whole file '{}' ({} chars).", path, text.len()),
                    data: None,
                },
                Err(e) => ToolResult {
                    success: false,
                    content: format!("Write error: {}", e),
                    data: None,
                },
            };
        }
    };

    if !perms.can_write(&path) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: cannot write '{}'.", path),
            data: None,
        };
    }

    let rel_path = PathBuf::from(&path);
    let full_path = project_root.join(&path);

    // Load content from buffer or disk
    let content = if let Some(existing) = state.get_content(&rel_path) {
        existing.to_string()
    } else {
        match std::fs::read_to_string(&full_path) {
            Ok(c) => {
                state.load_file(rel_path.clone(), c.clone());
                c
            }
            Err(e) => {
                // File doesn't exist — create it if start==0 && end==0
                if e.kind() == std::io::ErrorKind::NotFound {
                    if let Some(parent) = full_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = state.apply(Command::CreateFile {
                        path: rel_path.clone(),
                    });
                    state.load_file(rel_path.clone(), String::new());
                    String::new()
                } else {
                    return ToolResult {
                        success: false,
                        content: format!("Cannot read '{}': {}", path, e),
                        data: None,
                    };
                }
            }
        }
    };

    let lines: Vec<&str> = content.lines().collect();
    let num_lines = lines.len();

    // Resolve indexes
    let start = resolve_line_idx_write(start_raw, num_lines).min(num_lines);
    let end = resolve_line_idx_write(end_raw, num_lines)
        .min(num_lines)
        .max(start);

    // Compute byte offsets for start..end line range
    let mut start_offset = 0;
    for (i, line) in content.lines().enumerate() {
        if i == start {
            break;
        }
        start_offset += line.len() + 1; // +1 for \n
    }
    if start >= num_lines {
        start_offset = content.len();
    }

    let mut end_offset = start_offset;
    if end > start {
        let mut off = 0;
        for (i, line) in content.lines().enumerate() {
            if i == end {
                break;
            }
            off += line.len() + 1;
        }
        if end >= num_lines {
            end_offset = content.len();
        } else {
            end_offset = off;
        }
    }

    // Clamp offsets
    start_offset = start_offset.min(content.len());
    end_offset = end_offset.min(content.len());

    let old_text = content[start_offset..end_offset].to_string();

    // P10 review mode: stage the line-range change instead of applying
    if let Some(sink) = review {
        let proposed = format!(
            "{}{}{}",
            &content[..start_offset],
            text,
            &content[end_offset..]
        );
        return sink.stage(&path, &content, &proposed);
    }

    // Apply as Replace command (positions are char indices, not bytes)
    let at = content[..start_offset].chars().count();
    if let Err(e) = state.apply(Command::replace(
        rel_path.clone(),
        at,
        old_text.clone(),
        text.clone(),
    )) {
        return ToolResult {
            success: false,
            content: format!("Edit rejected: {}", e),
            data: None,
        };
    }

    match save_eff(state, project_root, &rel_path) {
        Ok(_) => {
            let action = if start == end { "Inserted" } else { "Replaced" };
            ToolResult {
                success: true,
                content: format!(
                    "{} at lines {}-{} in '{}' ({} chars).",
                    action,
                    start,
                    end,
                    path,
                    text.len()
                ),
                data: None,
            }
        }
        Err(e) => ToolResult {
            success: false,
            content: format!("Write error: {}", e),
            data: None,
        },
    }
}

fn execute_delete_file(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'path' argument. Expected: delete_file(path)".into(),
                data: None,
            }
        }
    };

    if !perms.can_write(&path) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: cannot delete '{}'.", path),
            data: None,
        };
    }

    let rel_path = PathBuf::from(&path);
    let full_path = project_root.join(&path);

    // Read current content (needed for undo) — from buffer or disk
    let content = if let Some(buf) = state.get_content(&rel_path) {
        buf.to_string()
    } else {
        match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return ToolResult {
                        success: false,
                        content: format!("File '{}' does not exist.", path),
                        data: None,
                    };
                }
                return ToolResult {
                    success: false,
                    content: format!("Error reading '{}': {}", path, e),
                    data: None,
                };
            }
        }
    };

    // Check file exists on disk
    if !full_path.exists() {
        return ToolResult {
            success: false,
            content: format!("File '{}' does not exist.", path),
            data: None,
        };
    }

    // Apply DeleteFile command (captures content for undo)
    let _ = state.apply(Command::DeleteFile {
        path: rel_path.clone(),
        content,
    });

    // Remove from disk
    match std::fs::remove_file(&full_path) {
        Ok(_) => ToolResult {
            success: true,
            content: format!("Deleted '{}'.", path),
            data: None,
        },
        Err(e) => ToolResult {
            success: false,
            content: format!("Error deleting '{}': {}", path, e),
            data: None,
        },
    }
}

fn execute_str_replace(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
    review: Option<&mut ReviewSink>,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'path' argument. Expected: replace_str(path, old_str, new_str)".into(),
                data: None,
            }
        }
    };
    let old_str = match get_str_arg(call, "old_str") {
        Some(s) => s,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'old_str' argument. Expected: replace_str(path, old_str, new_str)".into(),
                data: None,
            }
        }
    };
    let new_str = match get_str_arg(call, "new_str") {
        Some(s) => s,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'new_str' argument. Expected: replace_str(path, old_str, new_str)".into(),
                data: None,
            }
        }
    };

    if !perms.can_write(&path) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: cannot write '{}'.", path),
            data: None,
        };
    }

    let rel_path = PathBuf::from(&path);
    let full_path = project_root.join(&path);

    // Use buffer content if loaded, otherwise read from disk
    let content = if let Some(existing) = state.get_content(&rel_path) {
        existing.to_string()
    } else {
        match std::fs::read_to_string(&full_path) {
            Ok(c) => {
                state.load_file(rel_path.clone(), c.clone());
                c
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: format!("Cannot read '{}': {}", path, e),
                    data: None,
                }
            }
        }
    };

    // Find the occurrence — must be unique
    let matches: Vec<_> = content.match_indices(&old_str).collect();
    if matches.is_empty() {
        return ToolResult {
            success: false,
            content: "old_str not found in file.".into(),
            data: None,
        };
    }
    if matches.len() > 1 {
        return ToolResult {
            success: false,
            content: format!(
                "old_str matches {} times — must be unique. Add more context.",
                matches.len()
            ),
            data: None,
        };
    }

    let offset = matches[0].0;

    // P10 review mode: stage the replacement instead of applying
    if let Some(sink) = review {
        let proposed = content.replacen(&old_str, &new_str, 1);
        return sink.stage(&path, &content, &proposed);
    }

    let at = content[..offset].chars().count();
    if let Err(e) = state.apply(Command::replace(
        rel_path.clone(),
        at,
        old_str.clone(),
        new_str.clone(),
    )) {
        return ToolResult {
            success: false,
            content: format!("Edit rejected: {}", e),
            data: None,
        };
    }

    match save_eff(state, project_root, &rel_path) {
        Ok(_) => ToolResult {
            success: true,
            content: format!(
                "Replaced {} chars at offset {} in '{}'.",
                old_str.len(),
                offset,
                path
            ),
            data: None,
        },
        Err(e) => ToolResult {
            success: false,
            content: format!("Write error: {}", e),
            data: None,
        },
    }
}

// insert_lines removed — use edit_file(start==end) instead

fn execute_list_directory(call: &ToolCall, project_root: &Path) -> ToolResult {
    let rel_path = get_str_arg(call, "path").unwrap_or_default();
    let recursive = call.arguments.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
    let max_results = get_int_arg(call, "max_results").unwrap_or(50) as usize;
    let offset = get_int_arg(call, "offset").unwrap_or(0) as usize;
    let file_filter = get_str_arg(call, "file_filter");
    let max_depth = get_int_arg(call, "max_depth").map(|d| d as usize);

    let target = if rel_path.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(&rel_path)
    };

    let mut all_entries: Vec<String> = Vec::new();

    if recursive {
        collect_entries_recursive(&target, project_root, &file_filter, max_depth.unwrap_or(10), 0, &mut all_entries);
    } else {
        match std::fs::read_dir(&target) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        continue;
                    }
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    let display = if is_dir {
                        format!("{}/", name)
                    } else {
                        name.clone()
                    };
                    if let Some(ref filter) = file_filter {
                        if !matches_glob_simple(filter, &name) && !is_dir {
                            continue;
                        }
                    }
                    all_entries.push(display);
                }
            }
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: format!("Error listing '{}': {}", rel_path, e),
                    data: None,
                };
            }
        }
    }

    all_entries.sort();
    let total = all_entries.len();
    let page: Vec<&String> = all_entries.iter().skip(offset).take(max_results).collect();
    let has_more = offset + max_results < total;

    let mut output = page.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
    if has_more {
        output.push_str(&format!("\n\n[has_more=true, next_offset={}, total={}]", offset + max_results, total));
    }

    ToolResult {
        success: true,
        content: output,
        data: Some(serde_json::json!({
            "entries": page,
            "total": total,
            "has_more": has_more,
            "next_offset": if has_more { offset + max_results } else { total },
        })),
    }
}

/// Recursively collect directory entries.
fn collect_entries_recursive(
    dir: &Path,
    project_root: &Path,
    file_filter: &Option<String>,
    max_depth: usize,
    current_depth: usize,
    out: &mut Vec<String>,
) {
    if current_depth > max_depth {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let rel = entry.path().strip_prefix(project_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| name.clone());

        let display = if is_dir { format!("{}/", rel) } else { rel.clone() };

        if is_dir {
            out.push(display);
            collect_entries_recursive(&entry.path(), project_root, file_filter, max_depth, current_depth + 1, out);
        } else {
            if let Some(ref filter) = file_filter {
                if !matches_glob_simple(filter, &name) {
                    continue;
                }
            }
            out.push(display);
        }
    }
}

/// Simple glob matching (supports *.ext patterns).
fn matches_glob_simple(pattern: &str, name: &str) -> bool {
    if pattern.starts_with("*.") {
        let ext = &pattern[1..]; // e.g. ".rs"
        name.ends_with(ext)
    } else if pattern.contains('*') {
        // Very basic: split on * and check prefix/suffix
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            name.starts_with(parts[0]) && name.ends_with(parts[1])
        } else {
            name.contains(pattern)
        }
    } else {
        name.contains(pattern)
    }
}

/// discover_tools — returns available dynamic tools (stub for now; returns static list).
fn execute_discover_tools(_call: &ToolCall) -> ToolResult {
    // In the benchmark/headless context, all tools are already provided.
    // Return a helpful message listing available tools.
    ToolResult {
        success: true,
        content: "All available tools are already in your tool set. Use list_directory, read_file, edit_file, replace_str, find, run_shell, help_tool.".into(),
        data: None,
    }
}

/// help_tool — returns usage info for a specific tool.
fn execute_help_tool(call: &ToolCall) -> ToolResult {
    let tool_name = get_str_arg(call, "tool_name").unwrap_or_default();
    let help = match tool_name.as_str() {
        "list_directory" => "list_directory(path?, recursive?, max_results?, offset?, file_filter?, max_depth?) — List files/dirs. Paginated.",
        "read_file" => "read_file(path, offset?, max_results?) — Read file lines. Paginated. max_results=0 returns metadata only.",
        "edit_file" => "edit_file(path, content) — Write full file content.",
        "replace_str" => "replace_str(path, old_str, new_str) — Replace exact string in file.",
        "find" => "find(query, mode?, case_sensitive?, max_results?, path?, file_filter?) — Search files by regex or semantic.",
        "run_shell" => "run_shell(command, timeout?) — Run shell command in project root.",
        "discover_tools" => "discover_tools(query) — Find additional tools by capability.",
        _ => "Unknown tool. Available: list_directory, read_file, edit_file, replace_str, find, run_shell, discover_tools, help_tool.",
    };
    ToolResult {
        success: true,
        content: help.to_string(),
        data: None,
    }
}

// emit_command removed — agents use edit_file/str_replace directly

fn execute_query_trace(call: &ToolCall, graph: &TraceGraph) -> ToolResult {
    let req_id = match get_str_arg(call, "req_id") {
        Some(id) => id,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'req_id' argument. Expected: query_trace_graph(req_id)".into(),
                data: None,
            }
        }
    };

    match graph.query_requirement_owned(&req_id) {
        Some(trace) => ToolResult {
            success: true,
            content: serde_json::to_string_pretty(&trace).unwrap_or_default(),
            data: Some(serde_json::to_value(&trace).unwrap_or_default()),
        },
        None => ToolResult {
            success: false,
            content: format!("Requirement '{}' not found in trace graph.", req_id),
            data: None,
        },
    }
}

fn execute_query_code(call: &ToolCall, graph: &TraceGraph) -> ToolResult {
    let file = match get_str_arg(call, "file") {
        Some(f) => f,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'file' argument. Expected: query_code_element(file, name)".into(),
                data: None,
            }
        }
    };
    let name = match get_str_arg(call, "name") {
        Some(n) => n,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'name' argument. Expected: query_code_element(file, name)".into(),
                data: None,
            }
        }
    };

    match graph.query_code_element_owned(&PathBuf::from(&file), &name) {
        Some(trace) => ToolResult {
            success: true,
            content: serde_json::to_string_pretty(&trace).unwrap_or_default(),
            data: Some(serde_json::to_value(&trace).unwrap_or_default()),
        },
        None => ToolResult {
            success: false,
            content: format!("Code element '{}' in '{}' not found.", name, file),
            data: None,
        },
    }
}

fn execute_list_requirements(project_root: &Path) -> ToolResult {
    let reqs = crate::requirements::list_requirements(project_root);
    let content = reqs
        .iter()
        .map(|r| format!("{} | {} | {}", r.id, r.title, r.status.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    ToolResult {
        success: true,
        content,
        data: Some(serde_json::to_value(&reqs).unwrap_or_default()),
    }
}

fn execute_get_symbols(call: &ToolCall, symbols: &SymbolTable) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'path' argument. Expected: get_symbols(path)".into(),
                data: None,
            }
        }
    };

    match symbols.get_symbols(&PathBuf::from(&path)) {
        Some(syms) => {
            let summary: Vec<String> = syms
                .iter()
                .map(|s| format!("{} {:?} L{}-L{}", s.name, s.kind, s.start_line, s.end_line))
                .collect();
            ToolResult {
                success: true,
                content: summary.join("\n"),
                data: Some(serde_json::to_value(syms).unwrap_or_default()),
            }
        }
        None => ToolResult {
            success: false,
            content: format!("No symbols parsed for '{}'.", path),
            data: None,
        },
    }
}

fn execute_run_shell(call: &ToolCall, project_root: &Path, perms: &AgentPermissions) -> ToolResult {
    if !perms.allow_shell {
        return ToolResult {
            success: false,
            content: "Permission denied: shell commands not allowed for this agent.".into(),
            data: None,
        };
    }

    let command = match get_str_arg(call, "command") {
        Some(c) => c,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'command' argument. Expected: run_shell(command, cwd?, timeout_secs?)".into(),
                data: None,
            }
        }
    };

    let _timeout = get_int_arg(call, "timeout_secs")
        .unwrap_or(30)
        .min(perms.max_shell_timeout as i64) as u64;

    // Run synchronously (agents run in tokio, but shell is blocking)
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .current_dir(project_root)
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout
            } else {
                format!("{}\n--- stderr ---\n{}", stdout, stderr)
            };
            ToolResult {
                success: out.status.success(),
                content: combined,
                data: None,
            }
        }
        Err(e) => ToolResult {
            success: false,
            content: format!("Shell error: {}", e),
            data: None,
        },
    }
}

fn execute_find_grep(call: &ToolCall, project_root: &Path) -> ToolResult {
    // Accept both 'pattern' (internal name) and 'query' (schema name from tools.json)
    let pattern = match get_str_arg(call, "pattern").or_else(|| get_str_arg(call, "query")) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'query' argument. Expected: find(query, mode?, case_sensitive?, max_results?, path?, file_filter?)".into(),
                data: None,
            }
        }
    };
    let search_path = get_str_arg(call, "path").unwrap_or_default();
    let recursive = call
        .arguments
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let max_results = get_int_arg(call, "max_results").unwrap_or(20) as usize;
    let context_lines = get_int_arg(call, "context_lines").unwrap_or(0) as usize;
    let file_filter = get_str_arg(call, "file_filter").unwrap_or_default();

    // Compile regex
    let re = match regex::Regex::new(&pattern) {
        Ok(r) => r,
        Err(e) => {
            return ToolResult {
                success: false,
                content: format!("Invalid regex '{}': {}", pattern, e),
                data: None,
            }
        }
    };

    let target = if search_path.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(&search_path)
    };

    let mut results: Vec<String> = Vec::new();

    if target.is_file() {
        grep_file(
            &target,
            project_root,
            &re,
            context_lines,
            max_results,
            &mut results,
        );
    } else {
        grep_dir(
            &target,
            project_root,
            &re,
            &file_filter,
            recursive,
            context_lines,
            max_results,
            &mut results,
            0,
        );
    }

    if results.is_empty() {
        ToolResult {
            success: true,
            content: "No matches found.".into(),
            data: None,
        }
    } else {
        let truncated = results.len() >= max_results;
        let mut content = results.join("\n");
        if truncated {
            content.push_str(&format!("\n... (capped at {} results)", max_results));
        }
        ToolResult {
            success: true,
            content,
            data: None,
        }
    }
}

fn grep_file(
    file_path: &Path,
    root: &Path,
    re: &regex::Regex,
    context_lines: usize,
    max_results: usize,
    results: &mut Vec<String>,
) {
    if results.len() >= max_results {
        return;
    }
    let Ok(content) = std::fs::read_to_string(file_path) else {
        return;
    };
    let rel = file_path.strip_prefix(root).unwrap_or(file_path);
    let all_lines: Vec<&str> = content.lines().collect();

    for (line_num, line) in all_lines.iter().enumerate() {
        if re.is_match(line) {
            if context_lines == 0 {
                results.push(format!(
                    "{}:{}: {}",
                    rel.display(),
                    line_num + 1,
                    line.trim()
                ));
            } else {
                let start = line_num.saturating_sub(context_lines);
                let end = (line_num + context_lines + 1).min(all_lines.len());
                let mut block = format!("{}:{}\n", rel.display(), line_num + 1);
                for i in start..end {
                    let marker = if i == line_num { ">" } else { " " };
                    block.push_str(&format!("{}{:>4}| {}\n", marker, i + 1, all_lines[i]));
                }
                results.push(block);
            }
            if results.len() >= max_results {
                return;
            }
        }
    }
}

fn grep_dir(
    dir: &Path,
    root: &Path,
    re: &regex::Regex,
    file_filter: &str,
    recursive: bool,
    context_lines: usize,
    max_results: usize,
    results: &mut Vec<String>,
    depth: usize,
) {
    if depth > 15 || results.len() >= max_results {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                grep_dir(
                    &path,
                    root,
                    re,
                    file_filter,
                    recursive,
                    context_lines,
                    max_results,
                    results,
                    depth + 1,
                );
            }
        } else {
            // Apply file filter
            if !file_filter.is_empty() {
                if let Some(ext) = file_filter.strip_prefix("*.") {
                    if !name.ends_with(&format!(".{}", ext)) {
                        continue;
                    }
                }
            }
            grep_file(&path, root, re, context_lines, max_results, results);
        }
    }
}

fn execute_find_embed(call: &ToolCall, embed_index: &Option<SharedIndex>) -> ToolResult {
    let query = match get_str_arg(call, "query") {
        Some(q) => q,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'query' argument. Expected: find_embed(query, max_results?, path?, file_filter?)".into(),
                data: None,
            }
        }
    };
    let max_results = get_int_arg(call, "max_results").unwrap_or(5) as usize;
    let path_filter = get_str_arg(call, "path").unwrap_or_default();
    let file_filter = get_str_arg(call, "file_filter").unwrap_or_default();

    let index_arc = match embed_index {
        Some(arc) => arc.clone(),
        None => return ToolResult {
            success: false,
            content: "find_embed: embeddings index not initialized. Build it first (rebuild context button).".into(),
            data: None,
        },
    };

    let mut guard = index_arc.lock().unwrap();
    let index = match guard.as_mut() {
        Some(idx) => idx,
        None => {
            return ToolResult {
                success: false,
                content: "find_embed: embeddings index not built yet. Trigger rebuild.".into(),
                data: None,
            }
        }
    };

    let results = index.query(&query, max_results, &path_filter, &file_filter);

    if results.is_empty() {
        return ToolResult {
            success: true,
            content: "No semantic matches found.".into(),
            data: None,
        };
    }

    let content = results
        .iter()
        .map(|r| {
            format!(
                "{}:{}-{} (score: {:.3})\n{}",
                r.file.display(),
                r.start_line + 1,
                r.end_line,
                r.score,
                r.snippet
            )
        })
        .collect::<Vec<_>>()
        .join("\n---\n");

    ToolResult {
        success: true,
        content,
        data: None,
    }
}

// command_file_path removed — emit_command is gone

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;
    use crate::parser::{Symbol, SymbolKind, SymbolTable};
    use crate::trace_graph::{
        CodeElement, CodeElementKind, ReqStatus, Requirement, Spec, Test, TestKind, TraceGraph,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn full_perms() -> AgentPermissions {
        AgentPermissions::full_access("test-agent")
    }

    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.into(),
            arguments: args,
        }
    }

    // ===== read_file tests =====

    #[test]
    fn test_read_file_full_file() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("read_file", json!({"path": "f.txt", "max_results": 200}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("line1"));
        assert!(result.content.contains("line3"));
    }

    #[test]
    fn test_read_file_subset() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // offset=1, max_results=2 → lines b, c
        let call = make_call("read_file", json!({"path": "f.txt", "offset": 1, "max_results": 2}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("b"));
        assert!(result.content.contains("c"));
        assert!(!result.content.contains("| a"));
        assert!(!result.content.contains("| d"));
    }

    #[test]
    fn test_read_file_negative_index() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "a\nb\nc\nd\ne").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // offset=-2, max_results=200 → last 2 lines (d, e)
        let call = make_call("read_file", json!({"path": "f.txt", "offset": -2, "max_results": 200}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("d"));
        assert!(result.content.contains("e"));
    }

    #[test]
    fn test_read_file_missing_path() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("read_file", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    // ===== count_lines tests =====

    #[test]
    fn test_count_lines_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "a\nb\nc\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("count_lines", json!({"path": "f.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("3 lines"));
    }

    // ===== edit_file tests =====

    #[test]
    fn test_edit_file_prepend() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "existing\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call(
            "edit_file",
            json!({"path": "f.txt", "start": 0, "end": 0, "text": "// header\n"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success, "edit_file failed: {}", result.content);

        let on_disk = std::fs::read_to_string(tmp.path().join("f.txt")).unwrap();
        assert!(on_disk.starts_with("// header\n"));
        assert!(on_disk.contains("existing"));
    }

    #[test]
    fn test_edit_file_append() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "line1\nline2\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // -1 resolves to EOF
        let call = make_call(
            "edit_file",
            json!({"path": "f.txt", "start": -1, "end": -1, "text": "// end\n"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success, "edit_file failed: {}", result.content);

        let on_disk = std::fs::read_to_string(tmp.path().join("f.txt")).unwrap();
        assert!(on_disk.contains("// end"));
    }

    #[test]
    fn test_edit_file_replace_lines() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // Replace lines 1-3 (b, c, d) with "X\n"
        let call = make_call(
            "edit_file",
            json!({"path": "f.txt", "start": 1, "end": 4, "text": "X\n"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success, "edit_file failed: {}", result.content);

        let on_disk = std::fs::read_to_string(tmp.path().join("f.txt")).unwrap();
        assert_eq!(on_disk, "a\nX\ne\n");
    }

    #[test]
    fn test_edit_file_creates_new_file() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call(
            "edit_file",
            json!({"path": "new.txt", "start": 0, "end": 0, "text": "hello\n"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success, "edit_file failed: {}", result.content);

        let on_disk = std::fs::read_to_string(tmp.path().join("new.txt")).unwrap();
        assert_eq!(on_disk, "hello\n");
    }

    #[test]
    fn test_edit_file_undo() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "original\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("f.txt");
        state.load_file(rel.clone(), "original\n".into());

        let call = make_call(
            "edit_file",
            json!({"path": "f.txt", "start": 0, "end": 0, "text": "// added\n"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);

        assert!(state.get_content(&rel).unwrap().contains("// added"));

        let undone = state.undo();
        assert!(undone.changed);
        assert_eq!(state.get_content(&rel).unwrap(), "original\n");
    }

    // ===== str_replace tests =====

    #[test]
    fn test_str_replace_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hello world").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call(
            "str_replace",
            json!({"path": "f.txt", "old_str": "world", "new_str": "rust"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);

        let on_disk = std::fs::read_to_string(tmp.path().join("f.txt")).unwrap();
        assert_eq!(on_disk, "hello rust");
    }

    #[test]
    fn test_str_replace_not_found() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hello").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call(
            "str_replace",
            json!({"path": "f.txt", "old_str": "xyz", "new_str": "abc"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("not found"));
    }

    #[test]
    fn test_str_replace_nonunique() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "foo foo foo").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        state.load_file(PathBuf::from("f.txt"), "foo foo foo".into());
        let call = make_call(
            "str_replace",
            json!({"path": "f.txt", "old_str": "foo", "new_str": "bar"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("3 times"));
    }

    // ===== find_grep tests =====

    #[test]
    fn test_find_grep_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() { todo!(); }").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "// nothing").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find_grep", json!({"pattern": "todo!"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("a.rs"));
        assert!(result.content.contains("todo!"));
    }

    #[test]
    fn test_find_grep_no_matches() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find_grep", json!({"pattern": "zzz_never_match"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("No matches"));
    }

    #[test]
    fn test_find_grep_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find_grep", json!({"pattern": "[invalid"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("Invalid regex"));
    }

    // ===== find_embed tests =====

    #[test]
    fn test_find_embed_no_index() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find_embed", json!({"query": "auth handler"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        // No index passed via execute_tool (uses None), so returns error
        assert!(!result.success);
        assert!(result.content.contains("not initialized"));
    }

    // ===== list_files tests =====

    #[test]
    fn test_list_files_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("list_files", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("a.txt"));
        assert!(result.content.contains("sub/"));
    }

    // ===== delete_file tests =====

    #[test]
    fn test_delete_file_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("del.txt"), "bye").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("delete_file", json!({"path": "del.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(!tmp.path().join("del.txt").exists());
    }

    // ===== Permission tests =====

    #[test]
    fn test_read_file_permission_denied() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("secret.txt"), "hidden").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = AgentPermissions {
            agent_id: "restricted".into(),
            readable_paths: vec!["other/**".into()],
            ..Default::default()
        };

        let call = make_call("read_file", json!({"path": "secret.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.to_lowercase().contains("denied"));
    }

    #[test]
    fn test_edit_file_permission_denied() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "ok").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = AgentPermissions::read_only("ro-agent");

        let call = make_call(
            "edit_file",
            json!({"path": "f.txt", "start": 0, "end": 0, "text": "bad\n"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(
            result.content.to_lowercase().contains("denied")
                || result.content.to_lowercase().contains("not allowed")
        );
    }

    // ===== Unknown tool =====

    #[test]
    fn test_unknown_tool() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("nonexistent_tool_xyz", json!({"path": "f.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("Unknown tool"));
    }

    // ===== query_trace_graph =====

    #[test]
    fn test_query_trace_graph_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let mut graph = TraceGraph::new();
        let perms = full_perms();

        graph.add_requirement(Requirement {
            id: "REQ-01".into(),
            title: "Auth".into(),
            status: ReqStatus::Draft,
            file: PathBuf::from("reqs/REQ-01.md"),
        });

        let call = make_call("query_trace_graph", json!({"req_id": "REQ-01"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("REQ-01"));
    }

    // ===== run_shell =====

    #[test]
    fn test_run_shell_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("run_shell", json!({"command": "echo hello"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("hello"));
    }

    #[test]
    fn test_run_shell_denied() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = AgentPermissions::read_only("ro");

        let call = make_call("run_shell", json!({"command": "echo hi"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    // ===== get_symbols =====

    #[test]
    fn test_get_symbols_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let mut symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let path = PathBuf::from("src/main.rs");
        symbols.files.insert(
            path.clone(),
            vec![Symbol {
                name: "main".into(),
                kind: SymbolKind::Function,
                file: path.clone(),
                start_line: 1,
                end_line: 5,
                start_col: 0,
            }],
        );

        let call = make_call("get_symbols", json!({"path": "src/main.rs"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("main"));
    }
}
