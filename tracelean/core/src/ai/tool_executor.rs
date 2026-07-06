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
            denied_tools: vec!["write_file".into(), "str_replace".into(), "insert_lines".into(), "emit_command".into(), "run_shell".into()],
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
    if !permissions.is_tool_allowed(&call.name) {
        return ToolResult {
            success: false,
            content: format!("Permission denied: tool '{}' not allowed for agent '{}'.",
                call.name, permissions.agent_id),
            data: None,
        };
    }

    match call.name.as_str() {
        "read_file" => execute_read_file(call, project_root, permissions),
        "write_file" => execute_write_file(call, project_root, state, permissions),
        "str_replace" => execute_str_replace(call, project_root, state, permissions),
        "insert_lines" => execute_insert_lines(call, project_root, state, permissions),
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
            content: format!("Unknown tool: {}", call.name),
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

/// Writes `rel_path`'s current buffer content (read from `state`, not a
/// caller-supplied copy) to disk under `project_root`. Side-effecting;
/// name carries `_eff` suffix by convention.
fn save_eff(state: &AppState, project_root: &Path, rel_path: &Path) -> std::io::Result<()> {
    let content = state.get_content(&rel_path.to_path_buf()).unwrap_or_default();
    std::fs::write(project_root.join(rel_path), content)
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

    // Use buffer if loaded (preserves undo chain), otherwise read from disk
    let existing = if let Some(buf_content) = state.get_content(&rel_path) {
        buf_content.to_string()
    } else {
        std::fs::read_to_string(&full_path).unwrap_or_default()
    };
    let file_exists = full_path.exists() || state.get_content(&rel_path).is_some();

    if !file_exists {
        // Ensure parent dir exists
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        state.apply(Command::CreateFile { path: rel_path.clone() });
    }

    // Load into buffer only if not already there
    if state.get_content(&rel_path).is_none() {
        state.load_file(rel_path.clone(), existing.clone());
    }

    // Apply as Delete+Insert batch (whole file replacement)
    let content_len = content.len();
    state.apply(Command::replace(rel_path.clone(), 0, existing, content));

    // Persist buffer to disk via save_eff (reads from state, not local var).
    // On error the Command stays in the log (Req 1.3) — only tool result reports failure.
    match save_eff(state, project_root, &rel_path) {
        Ok(_) => ToolResult { success: true, content: format!("Wrote {} bytes to '{}'.", content_len, path), data: None },
        Err(e) => ToolResult { success: false, content: format!("Write error: {}", e), data: None },
    }
}

fn execute_str_replace(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => return ToolResult { success: false, content: "Missing 'path' argument.".into(), data: None },
    };
    let old_str = match get_str_arg(call, "old_str") {
        Some(s) => s,
        None => return ToolResult { success: false, content: "Missing 'old_str' argument.".into(), data: None },
    };
    let new_str = match get_str_arg(call, "new_str") {
        Some(s) => s,
        None => return ToolResult { success: false, content: "Missing 'new_str' argument.".into(), data: None },
    };

    if !perms.can_write(&path) {
        return ToolResult { success: false, content: format!("Permission denied: cannot write '{}'.", path), data: None };
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
            Err(e) => return ToolResult { success: false, content: format!("Cannot read '{}': {}", path, e), data: None },
        }
    };

    // Find the occurrence — must be unique
    let matches: Vec<_> = content.match_indices(&old_str).collect();
    if matches.is_empty() {
        return ToolResult { success: false, content: "old_str not found in file.".into(), data: None };
    }
    if matches.len() > 1 {
        return ToolResult {
            success: false,
            content: format!("old_str matches {} times — must be unique. Add more context.", matches.len()),
            data: None,
        };
    }

    let offset = matches[0].0;
    state.apply(Command::replace(rel_path.clone(), offset, old_str.clone(), new_str.clone()));

    match save_eff(state, project_root, &rel_path) {
        Ok(_) => ToolResult {
            success: true,
            content: format!("Replaced {} chars at offset {} in '{}'.", old_str.len(), offset, path),
            data: None,
        },
        Err(e) => ToolResult { success: false, content: format!("Write error: {}", e), data: None },
    }
}

fn execute_insert_lines(
    call: &ToolCall,
    project_root: &Path,
    state: &mut AppState,
    perms: &AgentPermissions,
) -> ToolResult {
    let path = match get_str_arg(call, "path") {
        Some(p) => p,
        None => return ToolResult { success: false, content: "Missing 'path' argument.".into(), data: None },
    };
    let line = match get_int_arg(call, "line") {
        Some(l) => l as usize,
        None => return ToolResult { success: false, content: "Missing 'line' argument.".into(), data: None },
    };
    let text = match get_str_arg(call, "text") {
        Some(t) => t,
        None => return ToolResult { success: false, content: "Missing 'text' argument.".into(), data: None },
    };

    if !perms.can_write(&path) {
        return ToolResult { success: false, content: format!("Permission denied: cannot write '{}'.", path), data: None };
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
            Err(e) => return ToolResult { success: false, content: format!("Cannot read '{}': {}", path, e), data: None },
        }
    };

    // Find byte offset of the target line
    let mut offset = 0;
    for (i, line_content) in content.split('\n').enumerate() {
        if i == line {
            break;
        }
        offset += line_content.len() + 1; // +1 for '\n'
    }
    // Clamp to end
    if offset > content.len() {
        offset = content.len();
    }

    let insert_text = if text.ends_with('\n') { text.clone() } else { format!("{}\n", text) };

    state.apply(Command::Insert {
        file: rel_path.clone(),
        offset,
        text: insert_text.clone(),
    });

    match save_eff(state, project_root, &rel_path) {
        Ok(_) => ToolResult {
            success: true,
            content: format!("Inserted {} chars at line {} in '{}'.", insert_text.len(), line, path),
            data: None,
        },
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
        | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => Some(file.to_string_lossy().to_string()),
        Command::CreateFile { path }
        | Command::DeleteFile { path, .. } => Some(path.to_string_lossy().to_string()),
        Command::RenameFile { from, .. } => Some(from.to_string_lossy().to_string()),
        Command::Batch { commands } => commands.first().and_then(|c| command_file_path(c)),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;
    use crate::parser::{Symbol, SymbolKind, SymbolTable};
    use crate::trace_graph::{
        CodeElement, CodeElementKind, Requirement, ReqStatus, Spec, Test, TestKind, TraceGraph,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper: full-access perms
    fn full_perms() -> AgentPermissions {
        AgentPermissions::full_access("test-agent")
    }

    /// Helper: make a ToolCall
    fn make_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            name: name.into(),
            arguments: args,
        }
    }

    #[test]
    fn test_read_file_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "world").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("read_file", json!({"path": "hello.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert_eq!(result.content, "world");
    }

    #[test]
    fn test_write_file_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("write_file", json!({"path": "out.txt", "content": "hello"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        // Verify file on disk
        let on_disk = std::fs::read_to_string(tmp.path().join("out.txt")).unwrap();
        assert_eq!(on_disk, "hello");
    }

    #[test]
    fn test_list_files_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "").unwrap();
        std::fs::write(tmp.path().join("b.txt"), "").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("list_files", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert!(result.content.contains("a.txt"));
        assert!(result.content.contains("b.txt"));
        assert!(result.content.contains("subdir/"));
    }

    #[test]
    fn test_emit_command_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        // Pre-load a buffer so Replace has something to work with
        state.load_file(PathBuf::from("f.txt"), "old".into());

        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let cmd_json = json!({
            "Insert": {"file": "f.txt", "offset": 3, "text": " stuff"}
        });
        let call = make_call("emit_command", json!({"command": cmd_json}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert_eq!(result.content, "Command applied.");
        assert_eq!(state.get_content(&PathBuf::from("f.txt")).unwrap(), "old stuff");
    }

    #[test]
    fn test_query_trace_graph_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let mut graph = TraceGraph::new();
        let perms = full_perms();

        // Add a requirement to the graph
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

    #[test]
    fn test_query_code_element_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let mut graph = TraceGraph::new();
        let perms = full_perms();

        graph.add_code_element(CodeElement {
            name: "do_thing".into(),
            kind: CodeElementKind::Function,
            file: PathBuf::from("src/lib.rs"),
            start_line: 1,
            end_line: 10,
        });

        let call = make_call("query_code_element", json!({"file": "src/lib.rs", "name": "do_thing"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert!(result.content.contains("do_thing"));
    }

    #[test]
    fn test_list_requirements_success() {
        let tmp = TempDir::new().unwrap();
        // Create reqs/ with a valid requirement file
        let reqs_dir = tmp.path().join("reqs");
        std::fs::create_dir(&reqs_dir).unwrap();
        std::fs::write(
            reqs_dir.join("REQ-01.md"),
            "# REQ-01: Authentication\nStatus: draft\n\nUsers must log in.",
        ).unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("list_requirements", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert!(result.content.contains("REQ-01"));
    }

    #[test]
    fn test_get_symbols_success() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let mut symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // Manually insert symbols into the table
        let path = PathBuf::from("src/main.rs");
        symbols.files.insert(path.clone(), vec![
            Symbol {
                name: "main".into(),
                kind: SymbolKind::Function,
                file: path.clone(),
                start_line: 1,
                end_line: 5,
                start_col: 0,
            },
        ]);

        let call = make_call("get_symbols", json!({"path": "src/main.rs"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert!(result.content.contains("main"));
    }

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
        assert!(result.content.trim().contains("hello"));
    }

    #[test]
    fn test_search_files_success() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("foo.rs"), "fn main() { search_target(); }").unwrap();
        std::fs::write(tmp.path().join("bar.rs"), "// nothing here").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("search_files", json!({"pattern": "search_target"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(result.success);
        assert!(result.content.contains("search_target"));
        assert!(result.content.contains("foo.rs"));
    }

    // ===== Permission-denied tests (Req 3.1, 3.2) =====

    #[test]
    fn test_read_file_permission_denied() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("secret.txt"), "top secret").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();

        // Path-level denial: only "other/**" is readable
        let perms = AgentPermissions {
            agent_id: "restricted".into(),
            readable_paths: vec!["other/**".into()],
            ..Default::default()
        };

        let call = make_call("read_file", json!({"path": "secret.txt"}));
        let log_before = state.command_log().len();
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        let lower = result.content.to_lowercase();
        assert!(lower.contains("permission denied") || lower.contains("denied"));
        assert_eq!(state.command_log().len(), log_before, "no state mutation");
    }

    #[test]
    fn test_write_file_permission_denied() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();

        // Tool-level denial via denied_tools
        let perms = AgentPermissions {
            agent_id: "restricted".into(),
            denied_tools: vec!["write_file".into()],
            ..Default::default()
        };

        let call = make_call("write_file", json!({"path": "out.txt", "content": "bad"}));
        let log_before = state.command_log().len();
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        let lower = result.content.to_lowercase();
        assert!(lower.contains("permission denied") || lower.contains("denied") || lower.contains("not allowed"));
        assert_eq!(state.command_log().len(), log_before, "no state mutation");
        // File should NOT exist on disk
        assert!(!tmp.path().join("out.txt").exists());
    }

    #[test]
    fn test_emit_command_permission_denied() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        state.load_file(PathBuf::from("f.txt"), "original".into());

        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();

        // Tool-level denial
        let perms = AgentPermissions {
            agent_id: "restricted".into(),
            denied_tools: vec!["emit_command".into()],
            ..Default::default()
        };

        let cmd_json = json!({"Insert": {"file": "f.txt", "offset": 0, "text": "bad"}});
        let call = make_call("emit_command", json!({"command": cmd_json}));
        let log_before = state.command_log().len();
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        let lower = result.content.to_lowercase();
        assert!(lower.contains("permission denied") || lower.contains("denied") || lower.contains("not allowed"));
        assert_eq!(state.command_log().len(), log_before, "no state mutation");
        // Buffer unchanged
        assert_eq!(state.get_content(&PathBuf::from("f.txt")).unwrap(), "original");
    }

    #[test]
    fn test_run_shell_permission_denied() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();

        // Shell denied via allow_shell: false
        let perms = AgentPermissions {
            agent_id: "restricted".into(),
            allow_shell: false,
            ..Default::default()
        };

        let call = make_call("run_shell", json!({"command": "echo pwned"}));
        let log_before = state.command_log().len();
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        let lower = result.content.to_lowercase();
        assert!(lower.contains("permission denied") || lower.contains("not allowed"));
        assert_eq!(state.command_log().len(), log_before, "no state mutation");
    }

    // ===== Error-path tests for ungated tools (Req 3.3) =====

    #[test]
    fn test_list_files_error_nonexistent_dir() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("list_files", json!({"path": "nonexistent_subdir"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        assert!(result.content.to_lowercase().contains("error"));
    }

    #[test]
    fn test_query_trace_graph_error_not_found() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new(); // empty graph
        let perms = full_perms();

        let call = make_call("query_trace_graph", json!({"req_id": "REQ-NONEXIST"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        assert!(result.content.contains("not found"));
    }

    #[test]
    fn test_query_code_element_error_not_found() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new(); // empty graph
        let perms = full_perms();

        let call = make_call("query_code_element", json!({"file": "no/such.rs", "name": "missing_fn"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        assert!(result.content.contains("not found"));
    }

    #[test]
    fn test_list_requirements_error_no_reqs_dir() {
        let tmp = TempDir::new().unwrap();
        // No reqs/ directory created — project root is empty
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("list_requirements", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        // list_requirements returns success:true with empty content when no reqs dir
        assert!(result.success);
        assert!(result.content.is_empty());
    }

    #[test]
    fn test_get_symbols_error_no_symbols() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new(); // empty symbol table
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("get_symbols", json!({"path": "src/nonexistent.rs"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        assert!(!result.success);
        assert!(result.content.contains("No symbols parsed for"));
    }

    #[test]
    fn test_search_files_error_no_matches() {
        let tmp = TempDir::new().unwrap();
        // Create a file that does NOT contain the search pattern
        std::fs::write(tmp.path().join("hello.txt"), "just some text").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("search_files", json!({"pattern": "zzz_no_match_ever_xyz"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

        // search_files returns success:true with "No matches found." when nothing matches
        assert!(result.success);
        assert!(result.content.contains("No matches found"));
    }

    // ===== Missing required arg tests (Req 4) — no panic, success:false =====

    #[test]
    fn test_read_file_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("read_file", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_write_file_missing_path() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("write_file", json!({"content": "hello"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_write_file_missing_content() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("write_file", json!({"path": "out.txt"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_list_files_empty_args_no_panic() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // list_files has no required args — should succeed (not panic)
        let call = make_call("list_files", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        // No panic occurred; it may succeed or fail depending on dir contents
        // The key invariant: no panic.
        assert!(result.success || !result.success); // always true — proves no panic
    }

    #[test]
    fn test_emit_command_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("emit_command", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_query_trace_graph_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("query_trace_graph", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_query_code_element_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("query_code_element", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_list_requirements_empty_args_no_panic() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // list_requirements has no required args — should not panic
        let call = make_call("list_requirements", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        // No panic — that's the invariant
        assert!(result.success || !result.success);
    }

    #[test]
    fn test_get_symbols_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("get_symbols", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_run_shell_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("run_shell", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    #[test]
    fn test_search_files_missing_args() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let call = make_call("search_files", json!({}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
    }

    // ===== Property test: write_file buffer/disk parity (Design Property 1) =====
    // **Validates: Requirements 1.1, 1.2**

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]
        #[test]
        fn prop_write_file_buffer_disk_parity(
            filename in "[a-z]{1,8}\\.txt",
            content in "[ -~\\n]{0,500}",
        ) {
            let tmp = TempDir::new().unwrap();
            let mut state = AppState::new();
            let symbols = SymbolTable::new();
            let graph = TraceGraph::new();
            let perms = full_perms();

            let call = make_call("write_file", json!({"path": filename.clone(), "content": content.clone()}));
            let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

            if result.success {
                // Buffer matches what was written
                let rel_path = PathBuf::from(&filename);
                let buf_content = state.get_content(&rel_path).unwrap();
                prop_assert_eq!(&buf_content, &content, "buffer != written content");

                // Disk matches buffer
                let disk_content = std::fs::read_to_string(tmp.path().join(&filename)).unwrap();
                prop_assert_eq!(&disk_content, &content, "disk != written content");
            }
        }
    }

    // ===== Property test: permission denial for gated tools (Design Property 2) =====
    // **Validates: Requirements 3.1, 3.2**

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]
        #[test]
        fn prop_permission_denial_gated_tools(
            tool_idx in 0usize..4,
        ) {
            let gated_tools = ["read_file", "write_file", "emit_command", "run_shell"];
            let tool_name = gated_tools[tool_idx];

            // Build permissions that deny this specific tool
            let perms = match tool_name {
                "run_shell" => AgentPermissions {
                    agent_id: "prop-agent".into(),
                    allow_shell: false,
                    ..Default::default()
                },
                other => AgentPermissions {
                    agent_id: "prop-agent".into(),
                    denied_tools: vec![other.into()],
                    ..Default::default()
                },
            };

            // Build a valid ToolCall for the tool
            let call = match tool_name {
                "read_file" => make_call("read_file", json!({"path": "any.txt"})),
                "write_file" => make_call("write_file", json!({"path": "any.txt", "content": "x"})),
                "emit_command" => make_call("emit_command", json!({"command": {"Insert": {"file": "f.txt", "offset": 0, "text": "x"}}})),
                "run_shell" => make_call("run_shell", json!({"command": "echo hi"})),
                _ => unreachable!(),
            };

            let tmp = TempDir::new().unwrap();
            let mut state = AppState::new();
            let symbols = SymbolTable::new();
            let graph = TraceGraph::new();

            let log_before = state.command_log().len();
            let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);

            // Must be denied
            prop_assert!(!result.success, "tool '{}' should be denied", tool_name);

            // Content must mention denial/permission/not allowed
            let lower = result.content.to_lowercase();
            prop_assert!(
                lower.contains("denied") || lower.contains("permission") || lower.contains("not allowed"),
                "denial msg missing for '{}': {}", tool_name, result.content
            );

            // No state mutation
            prop_assert_eq!(
                state.command_log().len(), log_before,
                "state mutated for denied tool '{}'", tool_name
            );
        }
    }

    // ===== Property test: undo restores prior state (Design Property 4) =====
    // **Validates: Requirements 5.1, 5.3**

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]
        #[test]
        fn prop_undo_restores_prior_state(
            initial in "[ -~]{0,200}",
            num_writes in 1usize..=5,
            contents in prop::collection::vec("[ -~]{0,200}", 1..=5),
        ) {
            let tmp = TempDir::new().unwrap();
            let rel_path = PathBuf::from("target.txt");

            // Write initial content to disk
            std::fs::write(tmp.path().join("target.txt"), &initial).unwrap();

            let mut state = AppState::new();
            let symbols = SymbolTable::new();
            let graph = TraceGraph::new();
            let perms = full_perms();

            // Load initial content into state buffer
            state.load_file(rel_path.clone(), initial.clone());

            let actual_writes = num_writes.min(contents.len());

            // Execute N write_file calls sequentially
            for i in 0..actual_writes {
                let call = make_call(
                    "write_file",
                    json!({"path": "target.txt", "content": contents[i]}),
                );
                let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
                prop_assert!(result.success, "write_file #{} failed: {}", i, result.content);
            }

            // Undo all writes in reverse order
            for i in 0..actual_writes {
                let undone = state.undo();
                prop_assert!(undone, "undo #{} returned false", i);
            }

            // Buffer should equal initial content
            let final_content = state.get_content(&rel_path).unwrap_or_default();
            prop_assert_eq!(
                final_content, initial,
                "after {} undos, buffer != initial",
                actual_writes
            );
        }
    }

    // ===== Mock-provider-free ToolCall undo tests (Req 5.1, 5.2) =====

    #[test]
    fn test_write_file_undo_restores_buffer() {
        let tmp = TempDir::new().unwrap();
        // Create existing file on disk
        std::fs::write(tmp.path().join("test.txt"), "original content").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // Load file into buffer (simulates opening the file)
        let rel = PathBuf::from("test.txt");
        state.load_file(rel.clone(), "original content".into());

        // Call write_file to overwrite with new content
        let call = make_call("write_file", json!({"path": "test.txt", "content": "new content"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);

        // Buffer should now be "new content"
        assert_eq!(state.get_content(&rel).unwrap(), "new content");

        // Undo the Replace (load_file inside execute_write_file re-sets buffer
        // to "original content", then apply(Replace) changes to "new content" — one undo reverts Replace)
        let undone = state.undo();
        assert!(undone);

        // Buffer restored to "original content"
        assert_eq!(state.get_content(&rel).unwrap(), "original content");
    }

    #[test]
    fn test_emit_command_undo_restores_buffer() {
        let tmp = TempDir::new().unwrap();
        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // Load file into buffer
        let rel = PathBuf::from("f.txt");
        state.load_file(rel.clone(), "hello world".into());

        // emit_command with Batch{Delete, Insert} to replace "world" -> "rust"
        let cmd_json = json!({
            "Batch": {"commands": [
                {"Delete": {"file": "f.txt", "offset": 6, "len": 5, "deleted_text": "world"}},
                {"Insert": {"file": "f.txt", "offset": 6, "text": "rust"}}
            ]}
        });
        let call = make_call("emit_command", json!({"command": cmd_json}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);

        // Buffer should be "hello rust"
        assert_eq!(state.get_content(&rel).unwrap(), "hello rust");

        // Undo
        let undone = state.undo();
        assert!(undone);

        // Buffer restored to "hello world"
        assert_eq!(state.get_content(&rel).unwrap(), "hello world");
    }

    // ===== Multi-call undo test (Req 5.3) =====

    // ===== Property test 3: missing-arg never panics (Req 4) =====
    // **Validates: Requirements 4**

    mod prop_missing_arg {
        use super::*;
        #[allow(unused_imports)]
        use proptest::prelude::*;

        /// Tools with required args: (tool_name, required_args)
        const TOOLS_WITH_REQUIRED: &[(&str, &[&str])] = &[
            ("read_file", &["path"]),
            ("write_file", &["path", "content"]),
            ("str_replace", &["path", "old_str", "new_str"]),
            ("insert_lines", &["path", "line", "text"]),
            ("emit_command", &["command"]),
            ("query_trace_graph", &["req_id"]),
            ("query_code_element", &["file", "name"]),
            ("get_symbols", &["path"]),
            ("run_shell", &["command"]),
            ("search_files", &["pattern"]),
        ];

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(100))]
            #[test]
            fn prop_missing_arg_never_panics(
                tool_idx in 0usize..10,
                arg_idx_seed in 0usize..10,
            ) {
                let (tool_name, required_args) = TOOLS_WITH_REQUIRED[tool_idx];
                // Pick which required arg to drop
                let drop_idx = arg_idx_seed % required_args.len();

                // Build args with all required present EXCEPT the dropped one
                let mut args = serde_json::Map::new();
                for (i, &arg_name) in required_args.iter().enumerate() {
                    if i == drop_idx {
                        continue; // drop this arg
                    }
                    // Put a dummy value for the arg
                    if arg_name == "command" && tool_name == "emit_command" {
                        // emit_command expects a JSON command object
                        args.insert(
                            arg_name.into(),
                            serde_json::json!({"Insert": {"file": "f.txt", "offset": 0, "text": "x"}}),
                        );
                    } else {
                        args.insert(arg_name.into(), serde_json::Value::String("dummy".into()));
                    }
                }

                let call = ToolCall {
                    name: tool_name.into(),
                    arguments: serde_json::Value::Object(args),
                };

                let tmp = tempfile::TempDir::new().unwrap();
                let mut state = AppState::new();
                let symbols = SymbolTable::new();
                let graph = TraceGraph::new();
                let perms = AgentPermissions::full_access("prop-test");

                // Must not panic
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms)
                }));

                prop_assert!(result.is_ok(), "execute_tool panicked for tool={} dropping arg={}", tool_name, required_args[drop_idx]);
                let result = result.unwrap();
                prop_assert!(!result.success, "expected success==false for tool={} missing arg={}", tool_name, required_args[drop_idx]);
            }
        }
    }

    #[test]
    fn test_multi_call_undo_restores_pre_first_call_state() {
        let tmp = TempDir::new().unwrap();
        // Pre-existing file on disk
        std::fs::write(tmp.path().join("multi.txt"), "AAAA").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        // Load file into buffer (simulates opening the file)
        let rel = PathBuf::from("multi.txt");
        state.load_file(rel.clone(), "AAAA".into());

        // First write_file call: overwrites with "BBBB"
        let call1 = make_call("write_file", json!({"path": "multi.txt", "content": "BBBB"}));
        let r1 = execute_tool(&call1, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(r1.success);
        assert_eq!(state.get_content(&rel).unwrap(), "BBBB");

        // Second write_file call: overwrites with "CCCC"
        let call2 = make_call("write_file", json!({"path": "multi.txt", "content": "CCCC"}));
        let r2 = execute_tool(&call2, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(r2.success);
        assert_eq!(state.get_content(&rel).unwrap(), "CCCC");

        // Undo second call (reverse order)
        let u1 = state.undo();
        assert!(u1);
        assert_eq!(state.get_content(&rel).unwrap(), "BBBB");

        // Undo first call
        let u2 = state.undo();
        assert!(u2);
        assert_eq!(state.get_content(&rel).unwrap(), "AAAA");
    }

    // ===== Undo tests for all agent tool operations =====

    #[test]
    fn test_str_replace_undo_restores_buffer() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("code.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("code.rs");
        let original = "fn main() {\n    println!(\"hello\");\n}\n";
        state.load_file(rel.clone(), original.into());

        let call = make_call("str_replace", json!({
            "path": "code.rs",
            "old_str": "println!(\"hello\")",
            "new_str": "println!(\"world\")"
        }));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success, "str_replace failed: {}", result.content);

        assert_eq!(state.get_content(&rel).unwrap(), "fn main() {\n    println!(\"world\");\n}\n");

        // Undo should restore original
        let undone = state.undo();
        assert!(undone);
        assert_eq!(state.get_content(&rel).unwrap(), original);
    }

    #[test]
    fn test_str_replace_multiple_undo_redo() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "aaa bbb ccc").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("f.txt");
        state.load_file(rel.clone(), "aaa bbb ccc".into());

        // First replace
        let call1 = make_call("str_replace", json!({"path": "f.txt", "old_str": "aaa", "new_str": "xxx"}));
        let r1 = execute_tool(&call1, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(r1.success);
        assert_eq!(state.get_content(&rel).unwrap(), "xxx bbb ccc");

        // Second replace
        let call2 = make_call("str_replace", json!({"path": "f.txt", "old_str": "bbb", "new_str": "yyy"}));
        let r2 = execute_tool(&call2, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(r2.success);
        assert_eq!(state.get_content(&rel).unwrap(), "xxx yyy ccc");

        // Undo second
        assert!(state.undo());
        assert_eq!(state.get_content(&rel).unwrap(), "xxx bbb ccc");

        // Undo first
        assert!(state.undo());
        assert_eq!(state.get_content(&rel).unwrap(), "aaa bbb ccc");

        // Redo first
        assert!(state.redo());
        assert_eq!(state.get_content(&rel).unwrap(), "xxx bbb ccc");

        // Redo second
        assert!(state.redo());
        assert_eq!(state.get_content(&rel).unwrap(), "xxx yyy ccc");
    }

    #[test]
    fn test_str_replace_nonunique_fails() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "foo foo foo").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        state.load_file(PathBuf::from("f.txt"), "foo foo foo".into());

        let call = make_call("str_replace", json!({"path": "f.txt", "old_str": "foo", "new_str": "bar"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("3 times"));
    }

    #[test]
    fn test_str_replace_not_found_fails() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "hello world").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        state.load_file(PathBuf::from("f.txt"), "hello world".into());

        let call = make_call("str_replace", json!({"path": "f.txt", "old_str": "xyz", "new_str": "abc"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(!result.success);
        assert!(result.content.contains("not found"));
    }

    #[test]
    fn test_insert_lines_undo_restores_buffer() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("f.txt");
        let original = "line1\nline2\nline3\n";
        state.load_file(rel.clone(), original.into());

        // Insert at line 1 (between line1 and line2)
        let call = make_call("insert_lines", json!({"path": "f.txt", "line": 1, "text": "inserted"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success, "insert_lines failed: {}", result.content);

        assert_eq!(state.get_content(&rel).unwrap(), "line1\ninserted\nline2\nline3\n");

        // Undo should restore original
        let undone = state.undo();
        assert!(undone);
        assert_eq!(state.get_content(&rel).unwrap(), original);
    }

    #[test]
    fn test_insert_lines_at_beginning() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "existing\n").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("f.txt");
        state.load_file(rel.clone(), "existing\n".into());

        let call = make_call("insert_lines", json!({"path": "f.txt", "line": 0, "text": "header"}));
        let result = execute_tool(&call, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(result.success);
        assert_eq!(state.get_content(&rel).unwrap(), "header\nexisting\n");

        assert!(state.undo());
        assert_eq!(state.get_content(&rel).unwrap(), "existing\n");
    }

    #[test]
    fn test_write_file_then_str_replace_undo_chain() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("f.txt"), "ORIGINAL").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("f.txt");
        state.load_file(rel.clone(), "ORIGINAL".into());

        // write_file replaces whole content
        let call1 = make_call("write_file", json!({"path": "f.txt", "content": "AAA BBB CCC"}));
        let r1 = execute_tool(&call1, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(r1.success);
        assert_eq!(state.get_content(&rel).unwrap(), "AAA BBB CCC");

        // str_replace does a surgical edit
        let call2 = make_call("str_replace", json!({"path": "f.txt", "old_str": "BBB", "new_str": "XXX"}));
        let r2 = execute_tool(&call2, tmp.path(), &mut state, &symbols, &graph, &perms);
        assert!(r2.success);
        assert_eq!(state.get_content(&rel).unwrap(), "AAA XXX CCC");

        // Undo str_replace
        assert!(state.undo());
        assert_eq!(state.get_content(&rel).unwrap(), "AAA BBB CCC");

        // Undo write_file
        assert!(state.undo());
        assert_eq!(state.get_content(&rel).unwrap(), "ORIGINAL");
    }

    #[test]
    fn test_all_tools_undo_full_chain() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("chain.txt"), "start content").unwrap();

        let mut state = AppState::new();
        let symbols = SymbolTable::new();
        let graph = TraceGraph::new();
        let perms = full_perms();

        let rel = PathBuf::from("chain.txt");
        state.load_file(rel.clone(), "start content".into());

        // 1. write_file → complete overwrite
        let c1 = make_call("write_file", json!({"path": "chain.txt", "content": "alpha beta gamma"}));
        assert!(execute_tool(&c1, tmp.path(), &mut state, &symbols, &graph, &perms).success);
        assert_eq!(state.get_content(&rel).unwrap(), "alpha beta gamma");

        // 2. str_replace → surgical patch
        let c2 = make_call("str_replace", json!({"path": "chain.txt", "old_str": "beta", "new_str": "BETA"}));
        assert!(execute_tool(&c2, tmp.path(), &mut state, &symbols, &graph, &perms).success);
        assert_eq!(state.get_content(&rel).unwrap(), "alpha BETA gamma");

        // 3. insert_lines → insert at line 0
        let c3 = make_call("insert_lines", json!({"path": "chain.txt", "line": 0, "text": "// header"}));
        assert!(execute_tool(&c3, tmp.path(), &mut state, &symbols, &graph, &perms).success);
        assert_eq!(state.get_content(&rel).unwrap(), "// header\nalpha BETA gamma");

        // 4. emit_command → raw Insert
        let cmd_json = json!({"Insert": {"file": "chain.txt", "offset": 0, "text": "!"}});
        let c4 = make_call("emit_command", json!({"command": cmd_json}));
        assert!(execute_tool(&c4, tmp.path(), &mut state, &symbols, &graph, &perms).success);
        assert_eq!(state.get_content(&rel).unwrap(), "!// header\nalpha BETA gamma");

        // Undo all in reverse order
        assert!(state.undo()); // undo emit_command
        assert_eq!(state.get_content(&rel).unwrap(), "// header\nalpha BETA gamma");

        assert!(state.undo()); // undo insert_lines
        assert_eq!(state.get_content(&rel).unwrap(), "alpha BETA gamma");

        assert!(state.undo()); // undo str_replace
        assert_eq!(state.get_content(&rel).unwrap(), "alpha beta gamma");

        assert!(state.undo()); // undo write_file
        assert_eq!(state.get_content(&rel).unwrap(), "start content");

        // No more undos
        assert!(!state.undo());
    }

    // ===== Static Dockerfile hardening assertions (Req 6) =====

    #[test]
    fn test_dockerfile_has_nonroot_user_before_entrypoint() {
        let dockerfile = include_str!("../../../Dockerfile");

        // Has useradd or adduser
        assert!(
            dockerfile.contains("useradd") || dockerfile.contains("adduser"),
            "Dockerfile must create a non-root user via useradd/adduser"
        );

        // Has chown on relevant dirs
        assert!(
            dockerfile.contains("chown"),
            "Dockerfile must chown relevant dirs to non-root user"
        );

        // USER appears before ENTRYPOINT
        let user_pos = dockerfile.find("USER tracelean")
            .or_else(|| dockerfile.find("USER "))
            .expect("Dockerfile must have a USER directive");
        let entrypoint_pos = dockerfile.find("ENTRYPOINT")
            .expect("Dockerfile must have an ENTRYPOINT");
        assert!(
            user_pos < entrypoint_pos,
            "USER must appear before ENTRYPOINT in Dockerfile"
        );
    }

    #[test]
    fn test_dockerfile_dev_has_nonroot_user_before_cmd() {
        let dockerfile = include_str!("../../../Dockerfile.dev");

        // Has useradd or adduser
        assert!(
            dockerfile.contains("useradd") || dockerfile.contains("adduser"),
            "Dockerfile.dev must create a non-root user via useradd/adduser"
        );

        // Has chown on relevant dirs (/app and elan)
        assert!(
            dockerfile.contains("chown"),
            "Dockerfile.dev must chown relevant dirs to non-root user"
        );

        // USER appears before CMD
        let user_pos = dockerfile.find("USER tracelean")
            .or_else(|| dockerfile.find("USER "))
            .expect("Dockerfile.dev must have a USER directive");
        let cmd_pos = dockerfile.rfind("\nCMD")
            .expect("Dockerfile.dev must have a CMD");
        assert!(
            user_pos < cmd_pos,
            "USER must appear before CMD in Dockerfile.dev"
        );
    }
}
