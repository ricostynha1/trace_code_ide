//! Tool executor — runs tool calls from agents against the project environment.
//! Respects permission model (4.17). Used by both internal agents and MCP host.

use super::tools::{ToolCall, ToolResult};
use crate::commands::Command;
use crate::parser::SymbolTable;
use crate::state::AppState;
use crate::trace_graph::TraceGraph;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
            denied_tools: vec!["write_file".into(), "emit_command".into(), "run_shell".into()],
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
    // Permission check
    if !permissions.is_tool_allowed(&call.tool_name) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: tool '{}' not allowed for agent '{}'.",
                call.tool_name, permissions.agent_id),
            data: None,
        };
    }

    match call.tool_name.as_str() {
        "read_file" => execute_read_file(call, project_root, permissions),
        "write_file" => execute_write_file(call, project_root, state, permissions),
        "list_files" => execute_list_files(call, project_root),
        "emit_command" => execute_emit_command(call, state, permissions),
        "query_trace_graph" => execute_query_trace(call, graph),
        "query_code_element" => execute_query_code(call, graph),
        "list_requirements" => execute_list_requirements(project_root),
        "get_symbols" => execute_get_symbols(call, symbols),
        "run_shell" => execute_run_shell(call, project_root, permissions),
        "search_files" => execute_search_files(call, project_root),
        _ => ToolResult {
            success: false,
            content: format!("Unknown tool: {}", call.tool_name),
            data: None,
        },
    }
}

fn get_str_arg(call: &ToolCall, key: &str) -> Option<String> {
    call.arguments.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn get_int_arg(call: &ToolCall, key: &str) -> Option<i64> {
    call.arguments.get(key).and_then(|v| v.as_i64())
}

fn execute_read_file(call: &ToolCall, project_root: &Path, perms: &AgentPermissions) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => return ToolResult { success: false, content: "Missing 'path' argument.".into(), data: None },
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
        Ok(content) => ToolResult { success: true, content, data: None },
        Err(e) => ToolResult { success: false, content: format!("Error reading '{}': {}", path, e), data: None },
    }
}

fn execute_write_file(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => return ToolResult { success: false, content: "Missing 'path' argument.".into(), data: None },
    };
    let content = match get_str_arg(call, "content") {
        Some(c) => c,
        None => return ToolResult { success: false, content: "Missing 'content' argument.".into(), data: None },
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

    // Read existing content (for Replace command) or create
    let existing = std::fs::read_to_string(&full_path).unwrap_or_default();
    let file_exists = full_path.exists();

    if !file_exists {
        // Ensure parent dir exists
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        state.apply(Command::CreateFile { path: rel_path.clone() });
    }

    // Load into buffer
    state.load_file(rel_path.clone(), existing.clone());

    // Apply as Replace command (whole file replacement)
    state.apply(Command::Replace {
        file: rel_path,
        offset: 0,
        old_text: existing,
        new_text: content.clone(),
    });

    // Write to disk
    match std::fs::write(&full_path, &content) {
        Ok(_) => ToolResult { success: true, content: format!("Wrote {} bytes to '{}'.", content.len(), path), data: None },
        Err(e) => ToolResult { success: false, content: format!("Write error: {}", e), data: None },
    }
}

fn execute_list_files(call: &ToolCall, project_root: &Path) -> ToolResult {
    let rel_path = get_str_arg(call, "path").unwrap_or_default();
    let target = if rel_path.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(&rel_path)
    };

    match std::fs::read_dir(&target) {
        Ok(entries) => {
            let mut listing: Vec<String> = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    listing.push(format!("{}/", name));
                } else {
                    listing.push(name);
                }
            }
            listing.sort();
            ToolResult {
                success: true,
                content: listing.join("\n"),
                data: Some(serde_json::to_value(&listing).unwrap_or_default()),
            }
        }
        Err(e) => ToolResult { success: false, content: format!("Error listing '{}': {}", rel_path, e), data: None },
    }
}

fn execute_emit_command(call: &ToolCall, state: &mut AppState, perms: &AgentPermissions) -> ToolResult {
    let cmd_value = match call.arguments.get("command") {
        Some(v) => v.clone(),
        None => return ToolResult { success: false, content: "Missing 'command' argument.".into(), data: None },
    };

    let cmd: Command = match serde_json::from_value(cmd_value) {
        Ok(c) => c,
        Err(e) => return ToolResult { success: false, content: format!("Invalid command JSON: {}", e), data: None },
    };

    // Check write permissions on affected files
    if let Some(file) = command_file_path(&cmd) {
        if !perms.can_write(&file) {
            return ToolResult {
                success: false,
                content: format!("Permission denied: cannot modify '{}'.", file),
                data: None,
            };
        }
    }

    state.apply(cmd);
    ToolResult { success: true, content: "Command applied.".into(), data: None }
}

fn execute_query_trace(call: &ToolCall, graph: &TraceGraph) -> ToolResult {
    let req_id = match get_str_arg(call, "req_id") {
        Some(id) => id,
        None => return ToolResult { success: false, content: "Missing 'req_id' argument.".into(), data: None },
    };

    match graph.query_requirement_owned(&req_id) {
        Some(trace) => ToolResult {
            success: true,
            content: serde_json::to_string_pretty(&trace).unwrap_or_default(),
            data: Some(serde_json::to_value(&trace).unwrap_or_default()),
        },
        None => ToolResult { success: false, content: format!("Requirement '{}' not found in trace graph.", req_id), data: None },
    }
}

fn execute_query_code(call: &ToolCall, graph: &TraceGraph) -> ToolResult {
    let file = match get_str_arg(call, "file") {
        Some(f) => f,
        None => return ToolResult { success: false, content: "Missing 'file' argument.".into(), data: None },
    };
    let name = match get_str_arg(call, "name") {
        Some(n) => n,
        None => return ToolResult { success: false, content: "Missing 'name' argument.".into(), data: None },
    };

    match graph.query_code_element_owned(&PathBuf::from(&file), &name) {
        Some(trace) => ToolResult {
            success: true,
            content: serde_json::to_string_pretty(&trace).unwrap_or_default(),
            data: Some(serde_json::to_value(&trace).unwrap_or_default()),
        },
        None => ToolResult { success: false, content: format!("Code element '{}' in '{}' not found.", name, file), data: None },
    }
}

fn execute_list_requirements(project_root: &Path) -> ToolResult {
    let reqs = crate::requirements::list_requirements(project_root);
    let content = reqs.iter()
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
        None => return ToolResult { success: false, content: "Missing 'path' argument.".into(), data: None },
    };

    match symbols.get_symbols(&PathBuf::from(&path)) {
        Some(syms) => {
            let summary: Vec<String> = syms.iter()
                .map(|s| format!("{} {:?} L{}-L{}", s.name, s.kind, s.start_line, s.end_line))
                .collect();
            ToolResult {
                success: true,
                content: summary.join("\n"),
                data: Some(serde_json::to_value(syms).unwrap_or_default()),
            }
        }
        None => ToolResult { success: false, content: format!("No symbols parsed for '{}'.", path), data: None },
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
        None => return ToolResult { success: false, content: "Missing 'command' argument.".into(), data: None },
    };

    let timeout = get_int_arg(call, "timeout_secs")
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
        Err(e) => ToolResult { success: false, content: format!("Shell error: {}", e), data: None },
    }
}

fn execute_search_files(call: &ToolCall, project_root: &Path) -> ToolResult {
    let pattern = match get_str_arg(call, "pattern") {
        Some(p) => p,
        None => return ToolResult { success: false, content: "Missing 'pattern' argument.".into(), data: None },
    };
    let file_pattern = get_str_arg(call, "file_pattern").unwrap_or_default();

    let mut matches: Vec<String> = Vec::new();
    search_recursive(project_root, project_root, &pattern, &file_pattern, &mut matches, 0);

    if matches.is_empty() {
        ToolResult { success: true, content: "No matches found.".into(), data: None }
    } else {
        let truncated = matches.len() > 100;
        matches.truncate(100);
        let mut content = matches.join("\n");
        if truncated {
            content.push_str("\n... (truncated, >100 matches)");
        }
        ToolResult { success: true, content, data: None }
    }
}

fn search_recursive(
    dir: &Path,
    root: &Path,
    pattern: &str,
    file_pattern: &str,
    results: &mut Vec<String>,
    depth: usize,
) {
    if depth > 10 || results.len() > 100 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return; };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" { continue; }
        let path = entry.path();
        if path.is_dir() {
            search_recursive(&path, root, pattern, file_pattern, results, depth + 1);
        } else {
            // Apply file pattern filter
            if !file_pattern.is_empty() {
                if let Some(ext) = file_pattern.strip_prefix("*.") {
                    if !name.ends_with(&format!(".{}", ext)) { continue; }
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                for (line_num, line) in content.lines().enumerate() {
                    if line.contains(pattern) {
                        results.push(format!("{}:{}: {}", rel.display(), line_num + 1, line.trim()));
                        if results.len() > 100 { return; }
                    }
                }
            }
        }
    }
}

/// Extract the file path affected by a command (for permission checks).
fn command_file_path(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Insert { file, .. }
        | Command::Delete { file, .. }
        | Command::Replace { file, .. }
        | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => Some(file.to_string_lossy().to_string()),
        Command::CreateFile { path }
        | Command::DeleteFile { path, .. } => Some(path.to_string_lossy().to_string()),
        Command::RenameFile { from, .. } => Some(from.to_string_lossy().to_string()),
        Command::Batch { commands } => commands.first().and_then(|c| command_file_path(c)),
    }
}
