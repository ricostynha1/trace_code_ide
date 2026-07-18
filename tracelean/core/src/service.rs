//! Service layer — all domain logic lives here.
//! Tauri commands are thin dispatchers that call into this module.
//! This module orchestrates AppState, SymbolTable, TraceGraph, persistence, etc.

use crate::commands::Command;
use crate::parser::{self, Symbol, SymbolTable};
use crate::persistence;
use crate::requirements::{self, LeanCheckResult, RequirementInfo, ReqStatus};
use crate::state::AppState;
use crate::trace_graph::{self, CodeElement, CodeElementKind, TraceGraph, Spec};
use std::path::{Path, PathBuf};

/// How many commands between auto-checkpoints
const CHECKPOINT_INTERVAL: usize = 100;

// --- Core Editor Operations ---

/// Result of applying a command: divergence-detection data for the frontend
/// plus checkpoint scheduling info.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApplyResult {
    /// Number of commands in the log after this apply.
    pub revision: u64,
    /// File the command touched (None for multi-file batches).
    pub file: Option<String>,
    /// FNV-1a 32 hash of that file's buffer after the apply.
    pub content_hash: Option<u32>,
    /// Derived cursor position after the edit.
    pub cursor: Option<crate::commands::CursorHint>,
    /// Set when a checkpoint should be saved (caller handles async).
    #[serde(skip)]
    pub checkpoint: Option<(PathBuf, usize)>,
}

/// Apply a command, auto-checkpoint if needed.
/// Fails without mutating if the command's witness doesn't match the buffer —
/// the caller must resync its view from backend state.
pub fn apply_command(state: &mut AppState, command: Command) -> Result<ApplyResult, String> {
    let file = crate::command_file(&command);
    let cursor = command.cursor_after();
    state.apply(command)?;

    let content_hash = file
        .as_ref()
        .and_then(|f| state.content_hash(&PathBuf::from(f)));

    let log_len = state.command_log().len();
    let mut checkpoint = None;
    if log_len > 0 && log_len % CHECKPOINT_INTERVAL == 0 {
        if let Some(root) = state.project_root().cloned() {
            checkpoint = Some((root, log_len));
        }
    }
    Ok(ApplyResult {
        revision: log_len as u64,
        file,
        content_hash,
        cursor,
        checkpoint,
    })
}

/// Open a project: restore state, parse files, return file listing.
pub fn open_project(
    state: &mut AppState,
    symbols: &mut SymbolTable,
    path: &str,
) -> Result<Vec<String>, String> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(format!("Not a directory: {}", path));
    }

    state.set_project_root(root.clone());

    // Restore persisted state
    if let Ok(restored) = persistence::restore_state(&root) {
        *state = restored;
        state.set_project_root(root.clone());
    }

    // Parse all source files in parallel
    let files = collect_source_files(&root);
    let file_contents: Vec<(PathBuf, String)> = files.iter().filter_map(|p| {
        let content = std::fs::read_to_string(root.join(p)).ok()?;
        Some((p.clone(), content))
    }).collect();

    let results = parser::parse_files_parallel(&file_contents);
    for result in results {
        symbols.files.insert(result.path, result.symbols);
    }

    list_files_recursive(&root, &root, 3).map_err(|e| e.to_string())
}

/// Open/load a file into buffer. Returns content.
pub fn open_file(state: &mut AppState, path: &str) -> Result<String, String> {
    let root = state.project_root().cloned().unwrap_or_default();
    let full_path = root.join(path);
    let rel_path = PathBuf::from(path);

    if let Some(content) = state.get_content(&rel_path) {
        return Ok(content.to_string());
    }

    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;
    state.load_file(rel_path.clone(), content.clone());
    state.record_file_open();
    Ok(content)
}

/// Save file to disk, re-parse symbols, persist command log.
pub fn save_file(state: &AppState, symbols: &mut SymbolTable, path: &str) -> Result<(), String> {
    let root = state.project_root().cloned().unwrap_or_default();
    let rel_path = PathBuf::from(path);
    let full_path = root.join(path);

    let content = state.get_content(&rel_path)
        .ok_or_else(|| format!("File not in buffer: {}", path))?;
    std::fs::write(&full_path, content)
        .map_err(|e| format!("Failed to write {}: {}", path, e))?;

    // Re-parse symbols
    symbols.parse_file(&rel_path, content);

    // Persist command log
    if let Err(e) = persistence::save_command_log(&root, state.command_log()) {
        eprintln!("Warning: failed to persist command log: {}", e);
    }

    Ok(())
}

/// Parse all project source files in parallel. Returns file count.
pub fn parse_project(state: &AppState, symbols: &mut SymbolTable) -> Result<usize, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;

    let files = collect_source_files(&root);
    let file_contents: Vec<(PathBuf, String)> = files.iter().filter_map(|p| {
        let content = std::fs::read_to_string(root.join(p)).ok()?;
        Some((p.clone(), content))
    }).collect();

    let results = parser::parse_files_parallel(&file_contents);
    let count = results.len();
    for result in results {
        symbols.files.insert(result.path, result.symbols);
    }
    Ok(count)
}

/// Parse a single file and return symbols.
pub fn parse_file_symbols(
    state: &AppState,
    symbols: &mut SymbolTable,
    path: &str,
) -> Result<Vec<Symbol>, String> {
    let root = state.project_root().cloned().unwrap_or_default();
    let rel_path = PathBuf::from(path);
    let full_path = root.join(path);

    let content = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Read error: {}", e))?;

    Ok(symbols.parse_file(&rel_path, &content).unwrap_or_default())
}

// --- Trace Graph Operations ---

/// Build trace graph from project files. Returns (node_count, edge_count).
pub fn build_trace_graph(
    state: &AppState,
    symbols: &SymbolTable,
    graph: &mut TraceGraph,
) -> Result<(usize, usize), String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    *graph = TraceGraph::new();
    graph.scan_project(&root, &symbols.files);
    Ok((graph.node_count(), graph.edge_count()))
}

/// Incremental graph update for a single changed file.
pub fn update_trace_graph_file(
    state: &AppState,
    symbols: &SymbolTable,
    graph: &mut TraceGraph,
    path: &str,
) -> Result<bool, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let rel_path = PathBuf::from(path);

    graph.remove_file(&rel_path);

    if path.starts_with("reqs/") && path.ends_with(".md") {
        let full_path = root.join(path);
        if full_path.exists() {
            if let Some(req) = trace_graph::parse_requirement_file_public(&full_path, &root) {
                graph.add_requirement(req);
            }
        }
    } else if path.starts_with("specs/") && path.ends_with(".lean") {
        let file_stem = rel_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        graph.add_spec(Spec {
            id: file_stem.clone(),
            req_id: file_stem,
            file: rel_path,
        });
    } else if let Some(syms) = symbols.get_symbols(&rel_path) {
        for sym in syms {
            let kind = match sym.kind {
                parser::SymbolKind::Function => CodeElementKind::Function,
                parser::SymbolKind::Method => CodeElementKind::Method,
                parser::SymbolKind::Class => CodeElementKind::Class,
                parser::SymbolKind::Struct => CodeElementKind::Struct,
                parser::SymbolKind::Module => CodeElementKind::Module,
                parser::SymbolKind::Trait => CodeElementKind::Trait,
                _ => continue,
            };
            graph.add_code_element(CodeElement {
                name: sym.name.clone(),
                kind,
                file: rel_path.clone(),
                start_line: sym.start_line,
                end_line: sym.end_line,
            });
        }
    }

    Ok(true)
}

// --- Requirements Operations ---

/// List all requirements.
pub fn list_requirements(state: &AppState) -> Result<Vec<RequirementInfo>, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    Ok(requirements::list_requirements(&root))
}

/// Update requirement status. Handles workflow transitions and spec auto-creation.
pub fn update_requirement_status(
    state: &mut AppState,
    req_id: &str,
    new_status_str: &str,
) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let new_status = ReqStatus::from_str(new_status_str);

    let req_file = root.join("reqs").join(format!("{}.md", req_id));
    if !req_file.exists() {
        return Err(format!("Requirement file not found: {}", req_id));
    }

    let content = std::fs::read_to_string(&req_file)
        .map_err(|e| format!("Read error: {}", e))?;

    let current = requirements::parse_requirement(&req_file, &root)
        .ok_or("Failed to parse requirement")?;

    if !current.status.can_transition_to(&new_status) {
        return Err(format!(
            "Invalid transition: {} → {}",
            current.status.as_str(),
            new_status.as_str()
        ));
    }

    // Update content
    let new_content = requirements::update_requirement_status(&content, &new_status);
    let rel_path = PathBuf::from(format!("reqs/{}.md", req_id));

    // Load into buffer if not already
    if state.get_content(&rel_path).is_none() {
        state.load_file(rel_path.clone(), content.clone());
    }

    // Apply as command
    state
        .apply(Command::replace(rel_path.clone(), 0, content, new_content.clone()))
        .map_err(|e| format!("Edit rejected: {}", e))?;

    // Write to disk
    std::fs::write(&req_file, &new_content)
        .map_err(|e| format!("Write error: {}", e))?;

    // Auto-create spec on approval
    if new_status == ReqStatus::Approved {
        let spec_path = root.join("specs").join(format!("{}.lean", req_id));
        if !spec_path.exists() {
            let req_info = RequirementInfo {
                id: req_id.to_string(),
                title: current.title,
                status: new_status.clone(),
                file: rel_path,
                description: current.description,
                has_spec: false,
            };
            let spec_content = requirements::generate_spec_template(&req_info);

            let specs_dir = root.join("specs");
            if !specs_dir.exists() {
                std::fs::create_dir_all(&specs_dir)
                    .map_err(|e| format!("Failed to create specs/: {}", e))?;
            }

            std::fs::write(&spec_path, &spec_content)
                .map_err(|e| format!("Failed to create spec: {}", e))?;

            let spec_rel = PathBuf::from(format!("specs/{}.lean", req_id));
            let _ = state.apply(Command::CreateFile { path: spec_rel });

            return Ok(format!(
                "Status updated to approved. Spec file created: specs/{}.lean",
                req_id
            ));
        }
    }

    Ok(format!("Status updated to {}", new_status.as_str()))
}

/// Create a new requirement.
pub fn create_requirement(
    state: &mut AppState,
    req_id: &str,
    title: &str,
) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;

    let reqs_dir = root.join("reqs");
    if !reqs_dir.exists() {
        std::fs::create_dir_all(&reqs_dir)
            .map_err(|e| format!("Failed to create reqs/: {}", e))?;
    }

    let req_file = reqs_dir.join(format!("{}.md", req_id));
    if req_file.exists() {
        return Err(format!("Requirement {} already exists", req_id));
    }

    let content = format!("# {}: {}\nStatus: draft\n\n", req_id, title);
    std::fs::write(&req_file, &content)
        .map_err(|e| format!("Write error: {}", e))?;

    let rel_path = PathBuf::from(format!("reqs/{}.md", req_id));
    state.load_file(rel_path.clone(), content);
    let _ = state.apply(Command::CreateFile { path: rel_path });

    Ok(format!("Created requirement: {}", req_id))
}

/// Check a Lean spec file. Returns compiler diagnostics.
pub fn check_lean_spec(state: &AppState, path: &str) -> Result<LeanCheckResult, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let full_path = root.join(path);
    if !full_path.exists() {
        return Err(format!("File not found: {}", path));
    }
    Ok(requirements::check_lean_file(&full_path))
}

/// Resolve navigation link: requirement ↔ spec.
pub fn navigate_trace_link(state: &AppState, from_path: &str) -> Result<Option<String>, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;

    if from_path.starts_with("reqs/") && from_path.ends_with(".md") {
        let req_id = from_path
            .strip_prefix("reqs/")
            .and_then(|s| s.strip_suffix(".md"))
            .ok_or("Invalid requirement path")?;
        let spec_path = format!("specs/{}.lean", req_id);
        if root.join(&spec_path).exists() {
            return Ok(Some(spec_path));
        }
        return Ok(None);
    }

    if from_path.starts_with("specs/") && from_path.ends_with(".lean") {
        let req_id = from_path
            .strip_prefix("specs/")
            .and_then(|s| s.strip_suffix(".lean"))
            .ok_or("Invalid spec path")?;
        let req_path = format!("reqs/{}.md", req_id);
        if root.join(&req_path).exists() {
            return Ok(Some(req_path));
        }
        return Ok(None);
    }

    Ok(None)
}

/// Determine editor mode from file path.
pub fn get_editor_mode(path: &str) -> &'static str {
    if path.ends_with(".lean") {
        "lean"
    } else if path.starts_with("reqs/") && path.ends_with(".md") {
        "requirement"
    } else {
        "code"
    }
}

// --- File System Helpers ---

/// List files in a directory for the file tree panel.
pub fn list_directory_files(root: &Path, rel_path: &str) -> Vec<crate::FileEntry> {
    let target = if rel_path.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel_path)
    };

    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&target) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let rel = entry.path().strip_prefix(root)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .to_string();
            entries.push(crate::FileEntry { name, path: rel, is_dir });
        }
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    entries
}

/// Collect all parseable source files recursively.
pub fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_source_files_inner(root, root, &mut files);
    files
}

fn collect_source_files_inner(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_source_files_inner(&path, root, files);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if parser::Lang::from_extension(ext).is_some() {
                if let Ok(rel) = path.strip_prefix(root) {
                    files.push(rel.to_path_buf());
                }
            }
        }
    }
}

fn list_files_recursive(
    dir: &Path,
    root: &Path,
    max_depth: usize,
) -> std::io::Result<Vec<String>> {
    let mut result = Vec::new();
    list_files_inner(dir, root, max_depth, 0, &mut result)?;
    Ok(result)
}

fn list_files_inner(
    dir: &Path,
    root: &Path,
    max_depth: usize,
    current_depth: usize,
    result: &mut Vec<String>,
) -> std::io::Result<()> {
    if current_depth >= max_depth {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let rel = entry.path().strip_prefix(root)
            .unwrap_or(&entry.path())
            .to_string_lossy()
            .to_string();
        if entry.file_type()?.is_dir() {
            result.push(format!("{}/", rel));
            list_files_inner(&entry.path(), root, max_depth, current_depth + 1, result)?;
        } else {
            result.push(rel);
        }
    }
    Ok(())
}
