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
    /// bugs.md Feature 4: run_shell waits for explicit user approval per command.
    #[serde(default)]
    pub review_commands: bool,
    /// T12 hard stop: polled while a shell command runs so Stop can kill the
    /// in-flight child process. Runtime-only — never part of the serialized
    /// permission config.
    #[serde(skip)]
    pub cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// sandboxing_better.md: shell sandbox mode ("off" | "detect" | "strict").
    #[serde(default = "default_shell_sandbox_mode")]
    pub shell_sandbox: String,
    /// Sandbox network policy ("deny" | "ask" | "allow").
    #[serde(default = "default_shell_network_policy")]
    pub shell_network: String,
    /// Per-command network grant from the approval prompt (policy "ask").
    /// Runtime-only, set by the agent loop just before dispatching run_shell.
    #[serde(skip)]
    pub shell_network_once: bool,
}

fn default_shell_sandbox_mode() -> String { "detect".to_string() }
fn default_shell_network_policy() -> String { "ask".to_string() }

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
            review_commands: false,
            cancel_flag: None,
            shell_sandbox: default_shell_sandbox_mode(),
            shell_network: default_shell_network_policy(),
            shell_network_once: false,
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
                "replace_str".into(),
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
    /// Stage a proposed full-content change. `applied_message` is the
    /// success message a direct (non-review) apply of this same edit would
    /// have returned — Bug 2: `runtime.rs` uses it (not this ToolResult's
    /// content, which never reaches the AI) as the tool result once the user
    /// accepts every hunk, so review mode is invisible to the model on the
    /// happy path.
    fn stage(&mut self, path: &str, original: &str, proposed: &str, applied_message: &str) -> ToolResult {
        let diff = super::diff_pipeline::create_pending_diff(
            path, original, proposed, &self.agent, applied_message,
        );
        let n = diff.hunks.len();
        self.pending.push(diff);
        ToolResult {
            success: true,
            content: format!(
                "Edit to '{}' staged for user review ({} hunk{} pending approval).",
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

    // Schema-driven argument validation (P1): every tool's arg requirements
    // live in tools.json, so this one gate replaces the per-tool hardcoded
    // "Missing 'x' argument" strings that used to live in each execute_*
    // function below. Tools not found in the registry (legacy dispatch
    // aliases) skip validation and fall through to their own checks.
    let registry = super::tool_registry::ToolRegistry::embedded();
    if let Some(schema) = registry.and_then(|r| r.schema_for(&call.name)) {
        if let Err(error) = super::tool_errors::validate_args(&call.arguments, schema) {
            return ToolResult {
                success: false,
                content: super::tool_errors::format_arg_validation_error(
                    &call.name,
                    &error,
                    registry,
                ),
                data: None,
            };
        }
    }

    match call.name.as_str() {
        "read_file" => execute_read_file(call, project_root, permissions),
        "edit_file" => execute_edit_file(call, project_root, state, permissions, review),
        // Mode-dispatched (auto|regex|semantic) with fallback chain; also the
        // only caller of execute_find_embed for the standalone semantic path.
        "find" => execute_find(call, project_root, embed_index),
        "replace_str" => execute_str_replace(call, project_root, state, permissions, review),
        "delete_file" => execute_delete_file(call, project_root, state, permissions),
        "list_directory" => execute_list_directory(call, project_root),
        "query_trace_graph" => execute_query_trace(call, graph),
        "query_code_element" => execute_query_code(call, graph),
        "list_requirements" => execute_list_requirements(project_root),
        "get_symbols" => execute_get_symbols(call, symbols),
        "run_shell" => execute_run_shell(call, project_root, state, permissions, review),
        // bugs.md Feature 6: web search / URL fetch (fetch saves to a project temp file)
        "web_search" => execute_web_search(call, project_root),
        "web_fetch" => execute_web_fetch(call, project_root),
        // Meta tools — handled inline
        "discover_tools" => execute_discover_tools(call),
        "help_tool" => execute_help_tool(call),
        _ => ToolResult {
            success: false,
            content: format!(
                "Unknown tool: {}. Available: {}. Consider using discover_tools(\"{}\") to find a tool that fulfills what you want.",
                call.name,
                super::tool_registry::ToolRegistry::embedded()
                    .map(|r| r.static_tool_names().join(", "))
                    .unwrap_or_default(),
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
                let applied_message = format!("Wrote whole file '{}' ({} chars).", path, text.len());
                return sink.stage(&path, &old_content, &text, &applied_message);
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
        let action = if start == end { "Inserted" } else { "Replaced" };
        let applied_message = format!(
            "{} at lines {}-{} in '{}' ({} chars).",
            action, start, end, path, text.len()
        );
        return sink.stage(&path, &content, &proposed, &applied_message);
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
        let near = near_match_lines(&content, &old_str, 3);
        let mut msg = format!("old_str not found in '{}'.", path);
        if near.is_empty() {
            msg.push_str(" No similar lines found either — re-read the file to get its current content.");
        } else {
            msg.push_str(" Closest matching line(s):\n");
            for (line_no, text) in &near {
                msg.push_str(&format!("  line {}: {}\n", line_no, text));
            }
            msg.push_str(
                "Check exact whitespace/indentation and invisible characters against these lines, \
                 or use edit_file(path, text, start, end) with the line numbers above instead.",
            );
        }
        return ToolResult {
            success: false,
            content: msg,
            data: None,
        };
    }
    if matches.len() > 1 {
        let mut msg = format!(
            "old_str matches {} times — must be unique. Matches at:\n",
            matches.len()
        );
        for (offset, _) in matches.iter().take(10) {
            let line_no = content[..*offset].matches('\n').count() + 1;
            let line_text = content.lines().nth(line_no - 1).unwrap_or("").trim();
            msg.push_str(&format!("  line {}: {}\n", line_no, line_text));
        }
        if matches.len() > 10 {
            msg.push_str(&format!("  ... and {} more\n", matches.len() - 10));
        }
        msg.push_str(
            "Add more surrounding context to old_str to disambiguate, or use edit_file with a \
             line range from the list above — when editing multiple occurrences, go back-to-front \
             (highest line number first) so earlier line numbers don't shift.",
        );
        return ToolResult {
            success: false,
            content: msg,
            data: None,
        };
    }

    let offset = matches[0].0;

    // P10 review mode: stage the replacement instead of applying
    if let Some(sink) = review {
        let proposed = content.replacen(&old_str, &new_str, 1);
        let applied_message = format!(
            "Replaced {} chars at offset {} in '{}'.",
            old_str.len(),
            offset,
            path
        );
        return sink.stage(&path, &content, &proposed, &applied_message);
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

/// Find lines resembling a failed `old_str` so replace_str's 0-match error can
/// show the model *why* the match failed (wrong whitespace, small typo, stale
/// content) and where the intended text actually lives. Tries progressively
/// looser matches on the first non-empty line of `old_str`: exact substring →
/// case-insensitive → whitespace-normalized → token overlap.
fn near_match_lines(content: &str, old_str: &str, max: usize) -> Vec<(usize, String)> {
    let needle = match old_str.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(n) => n,
        None => return Vec::new(),
    };
    let lines: Vec<&str> = content.lines().collect();
    let snippet = |i: usize| (i + 1, lines[i].trim().to_string());

    // Pass 1: exact substring.
    let hits: Vec<_> = lines.iter().enumerate()
        .filter(|(_, l)| l.contains(needle))
        .take(max).map(|(i, _)| snippet(i)).collect();
    if !hits.is_empty() {
        return hits;
    }

    // Pass 2: case-insensitive.
    let needle_lower = needle.to_lowercase();
    let hits: Vec<_> = lines.iter().enumerate()
        .filter(|(_, l)| l.to_lowercase().contains(&needle_lower))
        .take(max).map(|(i, _)| snippet(i)).collect();
    if !hits.is_empty() {
        return hits;
    }

    // Pass 3: whitespace-normalized (runs of whitespace collapse to one space).
    let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let needle_norm = normalize(needle);
    if !needle_norm.is_empty() {
        let hits: Vec<_> = lines.iter().enumerate()
            .filter(|(_, l)| normalize(l).contains(&needle_norm))
            .take(max).map(|(i, _)| snippet(i)).collect();
        if !hits.is_empty() {
            return hits;
        }
    }

    // Pass 4: token overlap — lines sharing most alphanumeric tokens with the needle.
    let tokens: Vec<String> = needle
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 3)
        .map(str::to_lowercase)
        .collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, usize)> = lines.iter().enumerate()
        .map(|(i, l)| {
            let ll = l.to_lowercase();
            (tokens.iter().filter(|t| ll.contains(t.as_str())).count(), i)
        })
        .filter(|(score, _)| *score * 2 >= tokens.len().max(1))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(max).map(|(_, i)| snippet(i)).collect()
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

/// discover_tools — search the registry for dynamic tools matching a
/// capability query. NOTE: the agent runtime intercepts discover_tools and
/// additionally LOADS the matches into the session's dynamic tool set; this
/// executor path serves direct/MCP callers and returns the matches only.
fn execute_discover_tools(call: &ToolCall) -> ToolResult {
    let registry = match super::tool_registry::ToolRegistry::embedded() {
        Some(r) => r,
        None => {
            return ToolResult {
                success: false,
                content: "Tool registry unavailable.".into(),
                data: None,
            }
        }
    };
    let query = get_str_arg(call, "query").unwrap_or_default();
    if query.trim().is_empty() {
        return ToolResult {
            success: false,
            content: format!(
                "Missing 'query' argument. Available additional tools: {}.",
                registry.available_dynamic_names().join(", ")
            ),
            data: None,
        };
    }
    let max = get_int_arg(call, "max_results").unwrap_or(10).clamp(1, 20) as usize;
    let matches = registry.discover_tools(&query, max);
    if matches.is_empty() {
        return ToolResult {
            success: true,
            content: format!(
                "No additional tools match '{}'. Available additional tools: {}.",
                query,
                registry.available_dynamic_names().join(", ")
            ),
            data: None,
        };
    }
    let lines: Vec<String> = matches
        .iter()
        .map(|name| {
            format!("- {}", registry.short_help_for(name).unwrap_or(name))
        })
        .collect();
    ToolResult {
        success: true,
        content: format!("Tools matching '{}':\n{}", query, lines.join("\n")),
        data: Some(serde_json::json!({ "matched_tools": matches })),
    }
}

/// help_tool — full usage info for a tool, read from tools.json via the
/// registry (bugs.md Bug 2: no hardcoded tool help).
fn execute_help_tool(call: &ToolCall) -> ToolResult {
    let tool_name = get_str_arg(call, "tool_name").unwrap_or_default();
    let registry = super::tool_registry::ToolRegistry::embedded();
    match registry.and_then(|r| r.help_tool(&tool_name)) {
        Some(text) => ToolResult { success: true, content: text, data: None },
        None => ToolResult {
            success: false,
            content: format!(
                "Unknown tool '{}'. Available: {}.",
                tool_name,
                registry
                    .map(|r| {
                        let mut names: Vec<&str> = r.static_tool_names();
                        names.extend(r.available_dynamic_names());
                        names.join(", ")
                    })
                    .unwrap_or_default()
            ),
            data: None,
        },
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

fn execute_run_shell(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
    review: Option<&mut ReviewSink>,
) -> ToolResult {
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

    let timeout_secs = get_int_arg(call, "timeout_secs")
        .unwrap_or(30)
        .clamp(1, perms.max_shell_timeout as i64) as u64;

    use super::shell_sandbox::{self, NetworkPolicy, SandboxMode};

    let mode = SandboxMode::from_setting(&perms.shell_sandbox);
    let allow_network = match NetworkPolicy::from_setting(&perms.shell_network) {
        NetworkPolicy::Allow => true,
        NetworkPolicy::Deny => false,
        // "ask": granted per command by the approval prompt's checkbox; with
        // approvals off nobody can grant it, so it behaves like deny.
        NetworkPolicy::Ask => perms.shell_network_once,
    };
    let cancel = perms.cancel_flag.clone();

    // Mode off: legacy direct execution against the real tree.
    if mode == SandboxMode::Off {
        return run_shell_direct(&command, project_root, timeout_secs, cancel.as_ref(), None);
    }

    // T1: overlay-backed sandbox — containment and diffing in one mechanism.
    if shell_sandbox::overlay_available() {
        match shell_sandbox::run_overlay(
            &command,
            project_root,
            timeout_secs,
            cancel.as_ref(),
            allow_network,
        ) {
            Ok(run) => return materialize_sandbox_run(run, project_root, state, review),
            Err(e) => {
                if mode == SandboxMode::Strict {
                    return ToolResult {
                        success: false,
                        content: format!(
                            "Sandbox error (shell_sandbox=strict, command NOT executed): {}",
                            e
                        ),
                        data: None,
                    };
                }
                eprintln!("[shell-sandbox] overlay run failed, falling back: {}", e);
            }
        }
    } else if mode == SandboxMode::Strict {
        return ToolResult {
            success: false,
            content: "shell_sandbox=strict, but the overlay sandbox is unavailable (needs \
                      bwrap with --overlay-src support and kernel ≥ 5.11 with unprivileged \
                      user namespaces). Command NOT executed."
                .into(),
            data: None,
        };
    }

    // T3: strace fallback — no containment, but mutated project files are
    // recovered from the syscall trace and still become visible events.
    if shell_sandbox::strace_available() {
        // The shell writes disk only, so open buffers still hold pre-run
        // content — snapshot them as the pre-image source.
        let pre_images: std::collections::HashMap<PathBuf, String> = state
            .open_files()
            .into_iter()
            .cloned()
            .collect::<Vec<PathBuf>>()
            .into_iter()
            .filter_map(|p| state.get_content(&p).map(|c| (p.clone(), c.to_string())))
            .collect();
        let lookup = move |rel: &Path| pre_images.get(rel).cloned();
        match shell_sandbox::run_strace(
            &command,
            project_root,
            timeout_secs,
            cancel.as_ref(),
            &lookup,
        ) {
            Ok(mut run) => {
                run.output = format!(
                    "[sandbox notice: overlay sandbox unavailable — ran via strace fallback; \
                     the command wrote the REAL project tree]\n{}",
                    run.output
                );
                return materialize_sandbox_run(run, project_root, state, review);
            }
            Err(e) => eprintln!("[shell-sandbox] strace run failed, falling back: {}", e),
        }
    }

    // Last resort in detect mode: unsandboxed, with a visible notice.
    run_shell_direct(
        &command,
        project_root,
        timeout_secs,
        cancel.as_ref(),
        Some(
            "[sandbox notice: no sandbox backend available (bwrap overlay and strace both \
             missing) — command ran UNSANDBOXED against the real tree]",
        ),
    )
}

/// Direct (unsandboxed) shell execution — sandbox mode `off` and the final
/// detect-mode fallback.
fn run_shell_direct(
    command: &str,
    project_root: &Path,
    timeout_secs: u64,
    cancel_flag: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
    notice: Option<&str>,
) -> ToolResult {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(project_root);
    match super::shell_sandbox::run_process(&mut cmd, timeout_secs, cancel_flag) {
        Err(e) => ToolResult {
            success: false,
            content: format!("Shell error: {}", e),
            data: None,
        },
        Ok(out) => {
            let mut content = match &out.killed_reason {
                Some(reason) => {
                    format!("Command killed ({}). Partial output:\n{}", reason, out.output)
                }
                None => out.output.clone(),
            };
            if let Some(n) = notice {
                content = format!("{}\n{}", n, content);
            }
            ToolResult {
                success: out.status_success.unwrap_or(false) && out.killed_reason.is_none(),
                content,
                data: None,
            }
        }
    }
}

/// T2: turn a sandbox run's captured mutations into the same visible, undoable
/// events an edit tool produces (R6). Overlay runs left the real tree
/// untouched, so changes are materialized through Command apply + save (or
/// staged into review); strace runs already hit disk, so they are only
/// recorded (buffers + undo tree brought in line with reality).
fn materialize_sandbox_run(
    run: super::shell_sandbox::SandboxRun,
    project_root: &Path,
    state: &mut AppState,
    mut review: Option<&mut ReviewSink>,
) -> ToolResult {
    use super::shell_sandbox::{Backend, MutationKind};

    let overlay = run.backend == Backend::Overlay;
    let mut notes: Vec<String> = Vec::new();
    let mut staged = 0usize;
    let mut batch: Vec<Command> = Vec::new();
    let mut to_save: Vec<PathBuf> = Vec::new();
    let mut to_remove: Vec<PathBuf> = Vec::new();

    for m in &run.mutations {
        let rel = m.path.clone();
        let path_str = rel.to_string_lossy().to_string();
        match m.kind {
            MutationKind::Created | MutationKind::Modified => {
                let post = m.post.clone().unwrap_or_default();
                let kind_str = if m.kind == MutationKind::Created { "created" } else { "modified" };
                // R2/R6: with the overlay, review mode routes shell writes
                // through the exact staging pipeline edit tools use — nothing
                // touches the real tree until the user accepts hunks.
                if overlay {
                    if let Some(sink) = review.as_deref_mut() {
                        let applied_message = format!("{} ({})", path_str, kind_str);
                        sink.stage(&path_str, m.pre.as_deref().unwrap_or(""), &post, &applied_message);
                        staged += 1;
                        notes.push(format!("{} ({}) → staged for review", path_str, kind_str));
                        continue;
                    }
                }
                let old = if let Some(cur) = state.get_content(&rel) {
                    cur.to_string()
                } else if let Some(pre) = &m.pre {
                    state.load_file(rel.clone(), pre.clone());
                    pre.clone()
                } else {
                    batch.push(Command::CreateFile { path: rel.clone() });
                    String::new()
                };
                if old != post {
                    batch.push(Command::Replace { file: rel.clone(), at: 0, old, new: post });
                }
                if overlay {
                    to_save.push(rel.clone());
                }
                notes.push(format!("{} ({})", path_str, kind_str));
            }
            MutationKind::Deleted => {
                let old = state
                    .get_content(&rel)
                    .map(|c| c.to_string())
                    .or_else(|| m.pre.clone())
                    .unwrap_or_default();
                batch.push(Command::DeleteFile { path: rel.clone(), content: old });
                if overlay {
                    to_remove.push(rel.clone());
                }
                notes.push(format!("{} (deleted — undoable in the undo tree)", path_str));
            }
        }
    }

    let mut apply_error: Option<String> = None;
    if !batch.is_empty() {
        let cmd = if batch.len() == 1 {
            batch.remove(0)
        } else {
            Command::Batch { commands: batch }
        };
        match state.apply(cmd) {
            Ok(_) => {
                for rel in &to_save {
                    if let Some(parent) = project_root.join(rel).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = save_eff(state, project_root, rel) {
                        apply_error = Some(format!("saving {}: {}", rel.display(), e));
                    }
                }
                for rel in &to_remove {
                    let _ = std::fs::remove_file(project_root.join(rel));
                }
            }
            Err(e) => apply_error = Some(e),
        }
    }

    let mut content = match &run.killed_reason {
        Some(reason) => format!("Command killed ({}). Partial output:\n{}", reason, run.output),
        None => run.output.clone(),
    };
    let mut sections: Vec<String> = Vec::new();
    if !notes.is_empty() {
        sections.push(format!(
            "[sandbox] project file changes ({}):\n  {}",
            notes.len(),
            notes.join("\n  ")
        ));
    }
    if !run.blocked.is_empty() {
        sections.push(format!(
            "[sandbox] writes under protected paths were discarded: {}",
            run.blocked.join(", ")
        ));
    }
    if !run.skipped.is_empty() {
        sections.push(format!(
            "[sandbox] changed but not captured as undoable text edits:\n  {}",
            run.skipped.join("\n  ")
        ));
    }
    if let Some(e) = &apply_error {
        sections.push(format!("[sandbox] ERROR materializing changes: {}", e));
    }
    if !sections.is_empty() {
        content = format!("{}\n\n{}", content.trim_end(), sections.join("\n"));
    }

    ToolResult {
        success: run.success && run.killed_reason.is_none() && apply_error.is_none(),
        content,
        data: Some(serde_json::json!({
            "sandbox_backend": match run.backend {
                Backend::Overlay => "overlay",
                Backend::Strace => "strace",
                Backend::None => "none",
            },
            "fs_mutations": notes.len(),
            "staged": staged,
            "blocked": run.blocked,
        })),
    }
}

// ─── web_search (bugs.md Feature 6) ─────────────────────────────────────────

/// Blocking HTTP GET on a dedicated thread (the executor runs inside tokio;
/// reqwest's blocking client must not be created/dropped on a runtime worker).
fn http_get(url: &str, timeout_secs: u64) -> Result<String, String> {
    let url_owned = url.to_string();
    std::thread::spawn(move || -> Result<String, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("Mozilla/5.0 (compatible; TraceLean/0.1)")
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(&url_owned).send().map_err(|e| e.to_string())?;
        let status = resp.status();
        let body = resp.text().map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("HTTP {} for {}", status, url_owned));
        }
        Ok(body)
    })
    .join()
    .map_err(|_| "fetch thread panicked".to_string())?
}

/// Strip HTML down to readable text: drop script/style, remove tags, decode
/// common entities, collapse blank lines. Crude but enough for grep/read.
fn html_to_text(html: &str) -> String {
    // (regex crate has no backreferences — spell each container out)
    let no_scripts = regex::Regex::new(
        r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>|<noscript[^>]*>.*?</noscript>|<svg[^>]*>.*?</svg>",
    )
    .map(|re| re.replace_all(html, " ").into_owned())
    .unwrap_or_else(|_| html.to_string());
    let with_breaks = regex::Regex::new(r"(?i)<(br|/p|/div|/li|/h[1-6]|/tr)[^>]*>")
        .map(|re| re.replace_all(&no_scripts, "\n").into_owned())
        .unwrap_or(no_scripts);
    let no_tags = regex::Regex::new(r"(?s)<[^>]+>")
        .map(|re| re.replace_all(&with_breaks, " ").into_owned())
        .unwrap_or(with_breaks);
    let decoded = no_tags
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'");
    // Collapse per-line whitespace and runs of blank lines.
    let mut out = String::with_capacity(decoded.len() / 2);
    let mut blank_run = 0;
    for line in decoded.lines() {
        let trimmed: Vec<&str> = line.split_whitespace().collect();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(&trimmed.join(" "));
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// Percent-decode a URL query component (enough for DuckDuckGo's uddg param).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// web_search — URL → fetch page as text into {project}/.tracelean/web/ for
/// the model to read/grep with its file tools; search terms → DuckDuckGo
/// results (title + URL) returned inline.
fn execute_web_search(call: &ToolCall, project_root: &Path) -> ToolResult {
    let query = match get_str_arg(call, "query").or_else(|| get_str_arg(call, "url")) {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => {
            return ToolResult {
                success: false,
                content: "Missing 'query' argument. Expected: web_search(query) — search terms or an http(s):// URL to fetch.".into(),
                data: None,
            }
        }
    };

    if query.starts_with("http://") || query.starts_with("https://") {
        return web_fetch_to_file(&query, project_root);
    }

    let max_results = get_int_arg(call, "max_results").unwrap_or(5).clamp(1, 20) as usize;
    let search_url = format!("https://html.duckduckgo.com/html/?q={}", url_encode(&query));
    let body = match http_get(&search_url, 20) {
        Ok(b) => b,
        Err(e) => {
            return ToolResult {
                success: false,
                content: format!("Web search failed: {}", e),
                data: None,
            }
        }
    };

    // Result links look like: <a class="result__a" href="//duckduckgo.com/l/?uddg=<encoded>&...">Title</a>
    let re = regex::Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .expect("static regex");
    let mut results = Vec::new();
    for cap in re.captures_iter(&body).take(max_results) {
        let href = &cap[1];
        let url = match href.split("uddg=").nth(1) {
            Some(enc) => url_decode(enc.split('&').next().unwrap_or(enc)),
            None => href.to_string(),
        };
        let title = html_to_text(&cap[2]);
        results.push(format!("- {} — {}", title, url));
    }

    if results.is_empty() {
        return ToolResult {
            success: false,
            content: format!("No web results for '{}'. Try different terms, or pass a URL directly to fetch it.", query),
            data: None,
        };
    }
    ToolResult {
        success: true,
        content: format!(
            "Web results for '{}':\n{}\n\nCall web_search with one of these URLs to fetch its content to a file.",
            query,
            results.join("\n")
        ),
        data: None,
    }
}

/// web_fetch — always fetches (never falls back to search). Accepts `url` or
/// `query`; a missing scheme gets https:// prepended.
fn execute_web_fetch(call: &ToolCall, project_root: &Path) -> ToolResult {
    let url = match get_str_arg(call, "url").or_else(|| get_str_arg(call, "query")) {
        Some(u) if !u.trim().is_empty() => u.trim().to_string(),
        _ => {
            return ToolResult {
                success: false,
                content: "Missing 'url' argument. Expected: web_fetch(url) — full http(s):// URL to fetch.".into(),
                data: None,
            }
        }
    };
    let url = if url.starts_with("http://") || url.starts_with("https://") {
        url
    } else {
        format!("https://{}", url)
    };
    web_fetch_to_file(&url, project_root)
}

/// Fetch a URL and save its readable text under {project}/.tracelean/web/.
fn web_fetch_to_file(url: &str, project_root: &Path) -> ToolResult {
    let body = match http_get(url, 30) {
        Ok(b) => b,
        Err(e) => {
            return ToolResult {
                success: false,
                content: format!("Fetch failed: {}", e),
                data: None,
            }
        }
    };
    let looks_html = body.trim_start().starts_with('<')
        || body.contains("<html")
        || body.contains("<body")
        || body.contains("</div>");
    let text = if looks_html { html_to_text(&body) } else { body };

    let dir = project_root.join(".tracelean").join("web");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return ToolResult {
            success: false,
            content: format!("Cannot create {}: {}", dir.display(), e),
            data: None,
        };
    }
    // Stable, readable file name derived from the URL.
    let slug: String = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(80)
        .collect();
    let rel_path = format!(".tracelean/web/{}.txt", slug);
    let abs_path = project_root.join(&rel_path);
    if let Err(e) = std::fs::write(&abs_path, &text) {
        return ToolResult {
            success: false,
            content: format!("Cannot write {}: {}", abs_path.display(), e),
            data: None,
        };
    }

    let lines = text.lines().count();
    let preview: String = text.chars().take(400).collect();
    ToolResult {
        success: true,
        content: format!(
            "Fetched {} → saved as '{}' ({} lines, {} chars). Use read_range/find_grep on that path to inspect it.\nPreview:\n{}",
            url, rel_path, lines, text.len(), preview
        ),
        data: None,
    }
}

/// Dispatcher for the `find` tool. Routes on the schema's `mode` parameter
/// (auto | regex | semantic) and implements the auto fallback chain:
/// exact regex → case-insensitive regex → semantic. Explicit modes that come
/// up empty suggest the other mode instead of failing silently.
fn execute_find(
    call: &ToolCall,
    project_root: &Path,
    embed_index: &Option<SharedIndex>,
) -> ToolResult {
    // Accept both 'pattern' (internal name) and 'query' (schema name from tools.json)
    let pattern = match get_str_arg(call, "pattern").or_else(|| get_str_arg(call, "query")) {
        Some(p) => p,
        None => {
            return ToolResult {
                success: false,
                content: "Missing 'query' argument. Expected: find(query, mode?, case_sensitive?, max_results?, offset?, path?, file_filter?)".into(),
                data: None,
            }
        }
    };
    let mode = get_str_arg(call, "mode").unwrap_or_else(|| "auto".to_string());

    match mode.as_str() {
        "semantic" => {
            let res = execute_find_embed(call, embed_index);
            if res.success && res.content.starts_with("No semantic matches") {
                ToolResult {
                    success: true,
                    content: format!(
                        "{} Try mode=\"regex\" if you are looking for an exact symbol, string, or pattern.",
                        res.content
                    ),
                    data: None,
                }
            } else {
                res
            }
        }
        "regex" => match regex_find(call, project_root, &pattern) {
            Err(err) => err,
            Ok(Some(content)) => ToolResult { success: true, content, data: None },
            Ok(None) => ToolResult {
                success: true,
                content: "No matches found (exact or case-insensitive). Try mode=\"semantic\" for a meaning-based search, or check the pattern — special chars like . * ( must be escaped for a literal match.".into(),
                data: None,
            },
        },
        // "auto" (and anything unrecognized): regex → case-insensitive → semantic
        _ => {
            match regex_find(call, project_root, &pattern) {
                Err(err) => return err,
                Ok(Some(content)) => return ToolResult { success: true, content, data: None },
                Ok(None) => {}
            }
            let sem = execute_find_embed(call, embed_index);
            if sem.success && !sem.content.starts_with("No semantic matches") {
                ToolResult {
                    success: true,
                    content: format!(
                        "regex and case-insensitive search found nothing; falling back to semantic search:\n{}",
                        sem.content
                    ),
                    data: None,
                }
            } else if !sem.success {
                ToolResult {
                    success: true,
                    content: format!(
                        "No matches found (exact or case-insensitive; semantic fallback unavailable: {})",
                        sem.content
                    ),
                    data: None,
                }
            } else {
                ToolResult {
                    success: true,
                    content: "No matches found — regex, case-insensitive, and semantic search all came up empty. Try a broader or simpler query.".into(),
                    data: None,
                }
            }
        }
    }
}

/// Hard cap on how many matches a single grep pass will count before stopping
/// the scan — bounds cost on pathological queries while still letting the
/// pagination hint report a meaningful total.
const GREP_SCAN_CAP: usize = 2000;

/// Collects grep matches for one `offset`/`max_results` window while counting
/// every match, so responses can say "N of M matches" and how to page.
struct GrepCollector {
    offset: usize,
    max_results: usize,
    /// Total matches seen so far (capped at GREP_SCAN_CAP).
    total: usize,
    results: Vec<String>,
}

impl GrepCollector {
    fn push(&mut self, entry: String) {
        if self.total >= self.offset && self.results.len() < self.max_results {
            self.results.push(entry);
        }
        self.total += 1;
    }
    fn scan_done(&self) -> bool {
        self.total >= GREP_SCAN_CAP
    }
}

/// Run the regex leg of `find`, including the case-insensitive retry and the
/// pagination hint. Returns:
/// - `Err(result)` — invalid regex, ready to return to the model;
/// - `Ok(Some(content))` — matches found (content includes any annotations);
/// - `Ok(None)` — genuinely zero matches, caller decides the fallback.
fn regex_find(
    call: &ToolCall,
    project_root: &Path,
    pattern: &str,
) -> Result<Option<String>, ToolResult> {
    let case_sensitive = call
        .arguments
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let offset = get_int_arg(call, "offset").unwrap_or(0).max(0) as usize;

    let re = regex::Regex::new(pattern).map_err(|e| ToolResult {
        success: false,
        content: format!(
            "Invalid regex '{}': {} — escape special chars like . * ( for a literal match.",
            pattern, e
        ),
        data: None,
    })?;

    let mut coll = run_grep(call, project_root, &re);
    let mut note = "";
    if coll.total == 0 && !case_sensitive {
        // Exact case found nothing — retry case-insensitively before giving up.
        if let Ok(ci_re) = regex::Regex::new(&format!("(?i){}", pattern)) {
            let ci_coll = run_grep(call, project_root, &ci_re);
            if ci_coll.total > 0 {
                note = "exact-case match found nothing; case-insensitive search found these:\n";
                coll = ci_coll;
            }
        }
    }

    if coll.total == 0 {
        return Ok(None);
    }
    if coll.results.is_empty() {
        // offset beyond the matches that exist
        return Ok(Some(format!(
            "offset {} is beyond the {} matches found — call again with a smaller offset.",
            offset, coll.total
        )));
    }

    let mut content = format!("{}{}", note, coll.results.join("\n"));
    let shown_through = offset + coll.results.len();
    if coll.total > shown_through {
        let total_str = if coll.scan_done() {
            format!("{}+", GREP_SCAN_CAP)
        } else {
            coll.total.to_string()
        };
        content.push_str(&format!(
            "\n{}",
            super::tool_registry::ToolRegistry::pagination_hint(
                coll.results.len(),
                &total_str,
                shown_through
            )
        ));
    }
    Ok(Some(content))
}

/// One full grep pass over the target path with the given (pre-compiled) regex.
fn run_grep(call: &ToolCall, project_root: &Path, re: &regex::Regex) -> GrepCollector {
    let search_path = get_str_arg(call, "path").unwrap_or_default();
    let recursive = call
        .arguments
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let max_results = get_int_arg(call, "max_results").unwrap_or(20) as usize;
    // Schema name is 'context'; 'context_lines' kept for internal callers.
    let context_lines = get_int_arg(call, "context_lines")
        .or_else(|| get_int_arg(call, "context"))
        .unwrap_or(0) as usize;
    let file_filter = get_str_arg(call, "file_filter").unwrap_or_default();
    let offset = get_int_arg(call, "offset").unwrap_or(0).max(0) as usize;

    let target = if search_path.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(&search_path)
    };

    let mut coll = GrepCollector {
        offset,
        max_results,
        total: 0,
        results: Vec::new(),
    };

    if target.is_file() {
        grep_file(&target, project_root, re, context_lines, &mut coll);
    } else {
        grep_dir(
            &target,
            project_root,
            re,
            &file_filter,
            recursive,
            context_lines,
            &mut coll,
            0,
        );
    }
    coll
}

fn grep_file(
    file_path: &Path,
    root: &Path,
    re: &regex::Regex,
    context_lines: usize,
    coll: &mut GrepCollector,
) {
    if coll.scan_done() {
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
                coll.push(format!(
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
                coll.push(block);
            }
            if coll.scan_done() {
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
    coll: &mut GrepCollector,
    depth: usize,
) {
    if depth > 15 || coll.scan_done() {
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
                    coll,
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
            grep_file(&path, root, re, context_lines, coll);
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

    // ===== schema-driven argument validation (P1) =====

    #[test]
    fn missing_required_arg_produces_schema_driven_message() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // "path" is required by read_file's tools.json schema; omit it.
        let call = make_call("read_file", json!({"max_results": 10}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("path"));
        assert!(result.content.contains("Usage:"));
        // Never reaches execute_read_file's own hand-written fallback text.
        assert!(!result.content.contains("Expected: read_file"));
    }

    #[test]
    fn wrong_type_arg_is_rejected_before_dispatch() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // "path" must be a string per schema; send a number instead.
        let call = make_call("read_file", json!({"path": 5}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("string"));
    }

    #[test]
    fn valid_args_pass_the_gate_unaffected() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hi\n").unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("read_file", json!({"path": "f.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
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

    // ===== replace_str tests =====

    #[test]
    fn test_replace_str_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hello world").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call(
            "replace_str",
            json!({"path": "f.txt", "old_str": "world", "new_str": "rust"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);

        let on_disk = std::fs::read_to_string(tmp.path().join("f.txt")).unwrap();
        assert_eq!(on_disk, "hello rust");
    }

    #[test]
    fn test_replace_str_not_found() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hello").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call(
            "replace_str",
            json!({"path": "f.txt", "old_str": "xyz", "new_str": "abc"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("not found"));
    }

    #[test]
    fn test_replace_str_nonunique() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "foo foo foo").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        state.load_file(PathBuf::from("f.txt"), "foo foo foo".into());
        let call = make_call(
            "replace_str",
            json!({"path": "f.txt", "old_str": "foo", "new_str": "bar"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("3 times"));
    }

    // ===== find (regex mode) tests =====

    #[test]
    fn test_find_regex_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() { todo!(); }").unwrap();
        std::fs::write(tmp.path().join("b.rs"), "// nothing").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find", json!({"query": "todo!"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("a.rs"));
        assert!(result.content.contains("todo!"));
    }

    #[test]
    fn test_find_regex_no_matches() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn main() {}").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find", json!({"query": "zzz_never_match"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert!(result.content.contains("No matches"));
    }

    #[test]
    fn test_find_regex_invalid_regex() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find", json!({"query": "[invalid"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("Invalid regex"));
    }

    // ===== find (semantic mode) tests =====

    #[test]
    fn test_find_semantic_no_index() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("find", json!({"query": "auth handler", "mode": "semantic"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        // No index passed via execute_tool (uses None), so returns error
        assert!(!result.success);
        assert!(result.content.contains("not initialized"));
    }

    // ===== list_directory tests =====

    #[test]
    fn test_list_directory_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("list_directory", json!({}));
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

    #[test]
    fn test_replace_str_permission_denied_for_read_only() {
        // read_only()'s denied_tools list must name the canonical
        // data/tools.json tool ("replace_str"), not the stale legacy alias
        // ("str_replace") — otherwise a read-only agent could still edit
        // files via replace_str.
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hello world").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = AgentPermissions::read_only("ro-agent");

        let call = make_call(
            "replace_str",
            json!({"path": "f.txt", "old_str": "world", "new_str": "rust"}),
        );
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.to_lowercase().contains("denied"));
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

    // ===== Dispatch <-> data/tools.json consistency (ai_module_cleanup_plan.md finding 7) =====
    //
    // The plan called this direction (an extra dispatch arm with no matching
    // tools.json entry, or vice versa) unenforceable "without reflection".
    // Parsing this file's own source with tree-sitter at test time gives us
    // exactly that reflection, so both directions are checked here — not
    // just "schema name silently falls through to Unknown tool", but also
    // "arm exists for a tool name tools.json doesn't know about".

    /// Extract every string-literal pattern (skipping the `_` fallback) from
    /// the `match call.name.as_str() { ... }` dispatch in this file's own
    /// source, by parsing it with tree-sitter-rust.
    fn dispatch_arm_names() -> Vec<String> {
        let source = include_str!("tool_executor.rs");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .expect("tree-sitter-rust language should load");
        let tree = parser.parse(source, None).expect("tool_executor.rs should parse");

        let mut names = Vec::new();
        let mut cursor = tree.root_node().walk();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if node.kind() == "match_expression" {
                if let Some(value) = node.child_by_field_name("value") {
                    if value.utf8_text(source.as_bytes()).unwrap_or("") == "call.name.as_str()" {
                        collect_arm_names(node, source, &mut names);
                    }
                }
            }
            for child in node.children(&mut cursor) {
                stack.push(child);
            }
        }
        names
    }

    fn collect_arm_names(match_expr: tree_sitter::Node, source: &str, out: &mut Vec<String>) {
        let Some(body) = match_expr.child_by_field_name("body") else { return };
        let mut cursor = body.walk();
        for arm in body.children(&mut cursor) {
            if arm.kind() != "match_arm" { continue }
            let Some(pattern) = arm.child_by_field_name("pattern") else { continue };
            // A single-literal arm's pattern is `match_pattern -> string_literal`;
            // an `a" | "b`-style or-pattern would nest further, but the dispatch
            // here only ever uses single string-literal arms.
            let mut inner_cursor = pattern.walk();
            for child in pattern.children(&mut inner_cursor) {
                if child.kind() == "string_literal" {
                    if let Ok(text) = child.utf8_text(source.as_bytes()) {
                        out.push(text.trim_matches('"').to_string());
                    }
                }
            }
        }
    }

    #[test]
    fn dispatch_arms_match_tools_json_exactly() {
        let arms = dispatch_arm_names();
        assert!(!arms.is_empty(), "tree-sitter should have found the dispatch match's string arms");

        let registry = super::super::tool_registry::ToolRegistry::embedded()
            .expect("embedded tools.json parses");
        let mut registry_names: Vec<String> = registry
            .all_tool_entries()
            .iter()
            .map(|e| e.name.clone())
            .collect();
        registry_names.sort();

        let mut arm_names = arms.clone();
        arm_names.sort();
        arm_names.dedup();

        let extra_arms: Vec<&String> = arm_names.iter().filter(|n| !registry_names.contains(n)).collect();
        assert!(
            extra_arms.is_empty(),
            "dispatch has arm(s) for tool name(s) not in data/tools.json: {:?}",
            extra_arms
        );

        let missing_arms: Vec<&String> = registry_names.iter().filter(|n| !arm_names.contains(n)).collect();
        assert!(
            missing_arms.is_empty(),
            "data/tools.json has tool(s) with no dispatch arm in tool_executor.rs: {:?}",
            missing_arms
        );
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::parser::SymbolTable;
    use crate::trace_graph::TraceGraph;
    use serde_json::json;
    use tempfile::TempDir;

    fn exec_reviewed(
        call: &super::super::tools::ToolCall,
        root: &Path,
        state: &mut AppState,
        pending: &mut Vec<super::super::diff_pipeline::PendingDiff>,
    ) -> ToolResult {
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = AgentPermissions::full_access("review-agent");
        let mut sink = ReviewSink { pending, agent: "review-agent".into() };
        execute_tool_reviewed(call, root, state, &symbols, &graph, &perms, &None, Some(&mut sink))
    }

    #[test]
    fn replace_str_stages_diff_instead_of_applying() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "hello world\nsecond line\n").unwrap();

        let mut state = AppState::new();
        let mut pending = Vec::new();
        let call = super::super::tools::ToolCall {
            name: "replace_str".into(),
            arguments: json!({"path": "a.txt", "old_str": "world", "new_str": "there"}),
        };
        let result = exec_reviewed(&call, tmp.path(), &mut state, &mut pending);

        assert!(result.success);
        assert!(result.content.contains("staged for user review"));
        // File untouched on disk
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "hello world\nsecond line\n"
        );
        // One pending diff with the proposed change
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].file, "a.txt");
        assert!(pending[0].proposed.contains("hello there"));
        assert!(!pending[0].hunks.is_empty());
        assert!(pending[0].hunks.iter().all(|h| !h.accepted));
    }

    #[test]
    fn edit_file_whole_stages_diff() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("b.txt"), "old content\n").unwrap();

        let mut state = AppState::new();
        let mut pending = Vec::new();
        let call = super::super::tools::ToolCall {
            name: "edit_file".into(),
            arguments: json!({"path": "b.txt", "text": "new content\n"}),
        };
        let result = exec_reviewed(&call, tmp.path(), &mut state, &mut pending);

        assert!(result.success);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("b.txt")).unwrap(),
            "old content\n"
        );
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].proposed, "new content\n");
    }

    #[test]
    fn read_tools_unaffected_by_review_mode() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("c.txt"), "data\n").unwrap();

        let mut state = AppState::new();
        let mut pending = Vec::new();
        let call = super::super::tools::ToolCall {
            name: "read_file".into(),
            arguments: json!({"path": "c.txt"}),
        };
        let result = exec_reviewed(&call, tmp.path(), &mut state, &mut pending);
        assert!(result.success);
        assert!(result.content.contains("data"));
        assert!(pending.is_empty());
    }
}
