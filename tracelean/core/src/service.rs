//! Service layer — all domain logic lives here.
//! Tauri commands are thin dispatchers that call into this module.
//! This module orchestrates AppState, SymbolTable, the trace index, persistence, etc.

use crate::commands::Command;
use crate::parser::{self, Symbol, SymbolTable};
use crate::persistence;
use crate::requirements::{self, LeanCheckResult, RequirementInfo, ReqStatus};
use crate::state::AppState;
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

    // Bug 4: every opened project gets a baseline commit point so the
    // initial state is never lost — but only if nothing survived restore
    // (a restored project already has its own history).
    if state.undo_tree().is_empty() {
        state.record_file_open();
        state.mark_commit_point("Initial snapshot".to_string());
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
    state.load_file(rel_path, content.clone());
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

    // Sandboxed workspace: mirror the save into the active session's work
    // dir too, and record the write so its watcher recognises the echo
    // instead of re-applying its own content back (`sandbox::mirror`).
    if let Some(link) = state.sandbox_link() {
        let sandbox_path = link.work_dir.join(&rel_path);
        if let Some(parent) = sandbox_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::write(&sandbox_path, content).is_ok() {
            link.self_writes.mark(rel_path.clone(), content);
        }
    }

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

// --- Annotation-based traceability (trace module) ---

/// Rebuild the whole traceability index from the filesystem and write the
/// lockfile.
///
/// Cheap enough to run on demand: it parses markdown and walks comment nodes,
/// but runs no prover and no tests.
pub fn trace_scan(state: &AppState) -> Result<crate::trace::TraceIndex, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    crate::trace::lockfile::save(&root, &index)?;
    Ok(index)
}

/// Findings only — what a CI gate or the problems panel wants.
pub fn trace_findings(state: &AppState) -> Result<Vec<crate::trace::Finding>, String> {
    Ok(trace_scan(state)?.findings)
}

/// One requirement with its links and per-clause assurance.
pub fn trace_requirement(
    state: &AppState,
    req_id: &str,
) -> Result<crate::trace::RequirementView, String> {
    let index = trace_scan(state)?;
    index
        .requirement_view(req_id)
        .ok_or_else(|| format!("No such requirement: {req_id}"))
}

/// Every requirement, summarized for the panel.
pub fn trace_overview(state: &AppState) -> Result<Vec<crate::trace::RequirementSummary>, String> {
    Ok(trace_scan(state)?.overview())
}

/// Coverage and assurance over the last `limit` commits, oldest first.
///
/// Returns the problems alongside the points: a commit that could not be
/// materialized leaves a gap in the chart, and the gap has to be explainable.
pub fn trace_history(
    state: &AppState,
    limit: usize,
) -> Result<(Vec<crate::trace::HistoryPoint>, Vec<String>), String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    crate::trace::history(&root, limit.clamp(1, 200))
}

// --- Differential testing ---

/// Run differential testing for one requirement clause and record the result.
///
/// The evidence written is L3 only when nothing diverged *and* the run covered
/// enough of the space to have had a chance of finding something — a case
/// count on its own is not evidence.
pub fn drt_run(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
    seed: u64,
    cases: usize,
) -> Result<crate::drt::DrtResult, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;

    let config = crate::drt::DrtConfig::load(&root)?;
    let binding = config.binding(req_id, clause).cloned().ok_or_else(|| {
        format!(
            "No differential-testing binding for {req_id}{}. Add one to .tracelean/drt.json \
             (`drt_scaffold` writes a starting point).",
            clause.map(|c| format!(".{c}")).unwrap_or_default()
        )
    })?;

    // `.tracelean/drt.json` is written in project-relative paths, because that
    // is what makes it reviewable and portable between machines. Nothing
    // guarantees the process running it is sitting in the project root, so the
    // paths are resolved here rather than left to the spawn to get wrong.
    let mut binding = binding;
    binding.model = rebase_runner(&binding.model, &root);
    let implementation_spec = rebase_runner(
        &binding.implementation.spec(&root, &config.bindings)?,
        &root,
    );

    let options = crate::drt::RunOptions {
        seed,
        cases,
        seeds_corpus: crate::drt::run::load_seeds(&root, req_id),
        ..crate::drt::RunOptions::default()
    };

    let result = crate::drt::run::run(&binding, &implementation_spec, &options)?;

    // A divergence that was found once is worth replaying forever.
    let witnesses: Vec<serde_json::Value> =
        result.divergences.iter().map(|d| d.input.clone()).collect();
    crate::drt::run::append_seeds(&root, req_id, &witnesses)?;

    let index = crate::trace::build(&root);
    let lean_version = crate::drt::lean_runner::toolchain()
        .map(|t| t.version)
        .unwrap_or_else(|_| "unavailable".into());
    let record = crate::drt::run::to_evidence(
        &result,
        index.current_hashes(req_id, clause),
        lean_version,
        None,
    );
    crate::trace::lockfile::put_evidence(&root, record)?;

    Ok(result)
}

/// Resolve a runner's program and working directory against the project root.
///
/// The program is made absolute only when the project actually contains it, so
/// `python3` or `cargo` still come from `PATH`; the working directory defaults
/// to the project root, which is what every relative argument in the config is
/// written against.
fn rebase_runner(spec: &crate::drt::RunnerSpec, root: &std::path::Path) -> crate::drt::RunnerSpec {
    let mut out = spec.clone();
    if let Some(program) = out.cmd.first_mut() {
        let candidate = root.join(&*program);
        if std::path::Path::new(&*program).is_relative() && candidate.exists() {
            *program = candidate.to_string_lossy().into_owned();
        }
    }
    out.cwd = match out.cwd {
        Some(cwd) if cwd.is_relative() => Some(root.join(cwd)),
        Some(cwd) => Some(cwd),
        None => Some(root.to_path_buf()),
    };
    out
}

/// Write a starting-point binding for a requirement.
pub fn drt_scaffold(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
    lang: &str,
) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;

    let mut config = crate::drt::DrtConfig::load(&root)?;
    if config.binding(req_id, clause).is_none() {
        config
            .bindings
            .push(crate::drt::config::scaffold_binding(req_id, clause, lang));
        config.save(&root)?;
    }

    Ok(format!(
        "Wrote a starting-point binding for {req_id} to .tracelean/drt.json. Point its \
         `entry` at the function that implements this clause, then run differential \
         testing -- there is no harness to write."
    ))
}

/// What binding this clause to a differential test would do — without doing it.
pub fn drt_bind_preview(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
) -> Result<crate::drt::BindProposal, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    crate::drt::bind::propose(&root, &index, req_id, clause)
}

/// The commands that carry out a binding proposal.
///
/// Returned rather than applied, so the caller can push them through the same
/// undo tree every other edit goes through. That is what makes an accidental
/// binding one `Ctrl+Z` away instead of a manual cleanup across three files.
pub fn drt_bind_commands(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
) -> Result<(crate::drt::BindProposal, crate::commands::Command), String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    let proposal = crate::drt::bind::propose(&root, &index, req_id, clause)?;
    let command = crate::drt::bind::apply(&root, &proposal)?;
    Ok((proposal, command))
}

// --- Requirement ↔ model judge ---

/// Judge whether the Lean model still formalizes a requirement clause.
///
/// The verdict is recorded as evidence either way: agreement earns L2, and
/// anything else records what was said without claiming consistency. A drift
/// verdict only becomes a finding once its witness has been executed against
/// the compiled model and behaved as claimed.
pub async fn judge_clause(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
) -> Result<crate::judge::Judgement, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;

    let settings = crate::ai::service::load_settings_from(&root)
        .unwrap_or_else(crate::ai::service::load_settings);
    let (model, provider) = crate::judge::provider_from_settings(&settings)?;

    let index = crate::trace::build(&root);
    let (input, model_runner) = crate::judge::prompt_input_for(&root, &index, req_id, clause)?;

    let options = crate::judge::JudgeOptions::default();
    let judgement =
        crate::judge::judge_clause_checked(&*provider, &model, &input, model_runner.as_ref(), &options)
            .await
            .map_err(|e| e.to_string())?;

    let record = judgement.to_evidence(
        req_id,
        clause,
        index.current_hashes(req_id, clause),
        None,
    );
    crate::trace::lockfile::put_evidence(&root, record)?;

    // A witness the judge got right is worth replaying on every future
    // differential run, so a disagreement once found is never lost.
    if let crate::judge::WitnessCheck::Confirmed { .. } = judgement.check {
        if let Some(w) = &judgement.reply.witness {
            let _ = crate::drt::run::append_seeds(&root, req_id, std::slice::from_ref(&w.input));
        }
    }

    Ok(judgement)
}

/// A self-contained judge prompt, for running the judgement somewhere else.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JudgePrompt {
    pub req_id: String,
    pub clause: Option<String>,
    /// System prompt and task, concatenated. Paste this anywhere.
    pub text: String,
    pub version: String,
    /// True when a compiled model runner exists, so a pasted drift verdict can
    /// have its witness executed. False means any drift claim will come back
    /// degraded, and the UI should say so *before* the user spends time on it.
    pub model_runner_available: bool,
}

/// Build the judge prompt without calling any provider.
///
/// The judgement is about whether an English clause and a Lean definition still
/// agree. An agent handed this prompt can open the surrounding files, which a
/// one-shot API call cannot — and it costs nothing. What makes the resulting
/// verdict worth the same is that the reply goes back through
/// `judge_apply_reply`, which parses and *executes* it exactly as the API path
/// does.
pub fn judge_prompt(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
) -> Result<JudgePrompt, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    let (input, model_runner) = crate::judge::prompt_input_for(&root, &index, req_id, clause)?;
    Ok(JudgePrompt {
        req_id: req_id.to_string(),
        clause: clause.map(|c| c.to_string()),
        text: crate::judge::prompt::render_standalone(&input),
        version: crate::judge::prompt::VERSION.to_string(),
        model_runner_available: model_runner.is_some(),
    })
}

/// Take a judge reply produced elsewhere and record it as evidence.
///
/// Deliberately the same pipeline as the API path, to the letter: the reply is
/// parsed by `verdict::parse` (so a drift verdict with no witness is rejected
/// before it can mean anything), and any witness is executed against the
/// compiled model (so a claim about Lean that does not hold is discarded). A
/// verdict typed in by hand earns exactly what one bought over HTTPS earns, and
/// not a level more.
pub fn judge_apply_reply(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
    reply_text: &str,
) -> Result<crate::judge::Judgement, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    let (_input, model_runner) = crate::judge::prompt_input_for(&root, &index, req_id, clause)?;

    let reply = crate::judge::verdict::parse(reply_text, false).map_err(|e| e.to_string())?;

    let options = crate::judge::JudgeOptions::default();
    let check = match (&reply.witness, reply.verdict.is_drift()) {
        (Some(w), true) => {
            crate::judge::witness::check(w, model_runner.as_ref(), options.witness_timeout)
        }
        _ => crate::judge::WitnessCheck::Inexecutable("no witness to check".into()),
    };
    let degraded = model_runner.is_none() && reply.verdict.is_drift();

    let judgement = crate::judge::Judgement {
        reply,
        source: "pasted".to_string(),
        check,
        degraded,
        // No provider was involved, so there is no model id to record and no
        // cost to attribute. Naming it plainly beats inventing one.
        model_id: "(external)".to_string(),
        prompt_version: crate::judge::prompt::VERSION.to_string(),
        cost_usd: 0.0,
        unreliable: false,
    };

    let record = judgement.to_evidence(req_id, clause, index.current_hashes(req_id, clause), None);
    crate::trace::lockfile::put_evidence(&root, record)?;

    if let crate::judge::WitnessCheck::Confirmed { .. } = judgement.check {
        if let Some(w) = &judgement.reply.witness {
            let _ = crate::drt::run::append_seeds(&root, req_id, std::slice::from_ref(&w.input));
        }
    }

    Ok(judgement)
}

/// Re-judge every clause whose requirement↔model bond has gone stale.
///
/// Serial on purpose: each call costs money, and a burst of parallel requests
/// is the fastest way to blow through a spend cap without noticing.
pub async fn judge_stale(state: &AppState, limit: usize) -> Result<Vec<String>, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);

    let mut targets: Vec<(String, Option<String>)> = Vec::new();
    for finding in &index.findings {
        if finding.kind != crate::trace::FindingKind::Stale {
            continue;
        }
        let Some(req_id) = finding.req_id.clone() else { continue };
        let assurance = index.assurance(&req_id, finding.clause.as_deref());
        if assurance.stale.contains(&crate::trace::Bond::RequirementModel) {
            targets.push((req_id, finding.clause.clone()));
        }
    }
    targets.sort();
    targets.dedup();
    targets.truncate(limit);

    let mut summary = Vec::new();
    for (req_id, clause) in targets {
        match judge_clause(state, &req_id, clause.as_deref()).await {
            Ok(j) => summary.push(format!(
                "{req_id}{}: {}",
                clause.map(|c| format!(".{c}")).unwrap_or_default(),
                j.reply.verdict.as_str()
            )),
            Err(e) => summary.push(format!(
                "{req_id}{}: {e}",
                clause.map(|c| format!(".{c}")).unwrap_or_default()
            )),
        }
    }
    Ok(summary)
}

/// Generate the Lean model runner for a requirement clause from its `@models`
/// anchors, and build it if a toolchain is available.
///
/// This is what closes the loop between the annotation index and differential
/// testing: the model side of a binding is derived from the same `@models`
/// links the checker reads, rather than being configured twice.
pub fn drt_generate_runner(
    state: &AppState,
    req_id: &str,
    clause: Option<&str>,
) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);

    let model_links: Vec<crate::trace::Link> = index
        .links_for_clause(req_id, clause)
        .into_iter()
        .filter(|l| l.role == crate::trace::Role::Models)
        .cloned()
        .collect();

    if model_links.is_empty() {
        return Err(format!(
            "Nothing carries `@models` for {req_id}{} — there is no model to run.",
            clause.map(|c| format!(".{c}")).unwrap_or_default()
        ));
    }

    let config = crate::drt::DrtConfig::load(&root)?;
    let op = config
        .binding(req_id, clause)
        .map(|b| b.op.clone())
        .unwrap_or_else(|| "default".into());

    let mut entries = Vec::new();
    let mut imports = Vec::new();
    for link in &model_links {
        let source = std::fs::read_to_string(root.join(&link.anchor.file))
            .map_err(|e| format!("reading {}: {e}", link.anchor.file.display()))?;
        let entry = crate::drt::lean_runner::entry_from_link(link, &source, &op)?;
        if !imports.contains(&entry.module) {
            imports.push(entry.module.clone());
        }
        entries.push(entry);
    }

    let dispatch = crate::drt::lean_runner::dispatch_for(&entries);
    let opens = namespaces_of(&entries);

    // The model package is wherever the lakefile above the model source lives.
    let model_dir = model_links
        .first()
        .and_then(|l| root.join(&l.anchor.file).parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| root.clone());
    let package_root = find_lake_root(&model_dir).unwrap_or(model_dir);
    let package_name = crate::drt::lean_runner::lake_package_name(&package_root)
        .or_else(|| package_root.file_name().and_then(|n| n.to_str()).map(str::to_string))
        .unwrap_or_else(|| "model".to_string());

    let generated = crate::drt::lean_runner::generate(
        &root,
        req_id,
        &relative_from(&crate::drt::lean_runner::package_dir(&root), &package_root),
        &package_name,
        &imports,
        &opens,
        &dispatch,
    )?;

    match crate::drt::lean_runner::build(&root) {
        Ok(binary) => Ok(format!(
            "Generated and built the model runner at {}.",
            binary.display()
        )),
        Err(e) => Ok(format!(
            "Generated the runner package at {} but could not build it: {e}",
            generated.display()
        )),
    }
}

/// Generate and build **one** model runner covering every binding in
/// `.tracelean/drt.json`.
///
/// This is the project-wide counterpart of `drt_generate_runner`, and the one
/// that makes `model.cmd` honest: every binding points at the same
/// `.tracelean/drt/.lake/build/bin/drtRunner`, so that binary has to answer for
/// all of them. Generating per requirement wrote each requirement's module over
/// the last, leaving a binary that knew about exactly one.
///
/// Bindings whose model cannot be read are reported by name rather than
/// aborting the build: a project with one unreadable model still deserves a
/// runner for the rest.
pub fn drt_build_runner(state: &AppState) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    let config = crate::drt::DrtConfig::load(&root)?;

    if config.bindings.is_empty() {
        return Err(
            "No bindings in .tracelean/drt.json — bind a clause to its model first.".to_string(),
        );
    }

    let mut entries: Vec<crate::drt::lean_runner::ModelEntry> = Vec::new();
    let mut imports: Vec<String> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let mut package_root: Option<PathBuf> = None;

    for binding in &config.bindings {
        let clause = binding.clause.as_deref();
        let label = crate::drt::config::qualified_op(&binding.req_id, clause);

        let model_link = index
            .links_for_clause(&binding.req_id, clause)
            .into_iter()
            .find(|l| l.role == crate::trace::Role::Models)
            .cloned();
        let Some(link) = model_link else {
            problems.push(format!("{label}: nothing carries `@models`, so it has no model to run"));
            continue;
        };

        let source = match std::fs::read_to_string(root.join(&link.anchor.file)) {
            Ok(s) => s,
            Err(e) => {
                problems.push(format!("{label}: reading {}: {e}", link.anchor.file.display()));
                continue;
            }
        };
        // Module names are relative to the lake package, so the package root
        // has to be known before the entry can be built.
        let absolute = root.join(&link.anchor.file);
        let link_package_root = absolute
            .parent()
            .and_then(find_lake_root)
            .unwrap_or_else(|| root.clone());
        let relative = absolute.strip_prefix(&link_package_root).unwrap_or(&link.anchor.file);

        let entry = match crate::drt::lean_runner::entry_from_link_relative(
            &link,
            &source,
            &binding.op,
            relative,
        ) {
            Ok(entry) => entry,
            Err(e) => {
                problems.push(format!("{label}: {e}"));
                continue;
            }
        };

        // A lone argument whose fields the schema carries directly is the
        // flattened-structure case: the whole input *is* that argument.
        let mut entry = entry;
        entry.whole_input = entry.arguments.len() == 1
            && !schema_has_field(&binding.input, &entry.arguments[0]);

        // Two bindings may name the same op only if they mean the same model
        // function; otherwise one of them would be answered by the other's.
        if let Some(clash) = entries.iter().find(|e| e.op == entry.op) {
            if clash.function != entry.function {
                problems.push(format!(
                    "{label}: op `{}` is already bound to `{}` — give this binding its own `op` \
                     in .tracelean/drt.json",
                    entry.op, clash.function
                ));
            }
            continue;
        }

        if package_root.is_none() {
            package_root = Some(link_package_root);
        }
        if !imports.contains(&entry.module) {
            imports.push(entry.module.clone());
        }
        entries.push(entry);
    }

    if entries.is_empty() {
        return Err(format!(
            "No binding has a readable Lean model, so there is nothing to build:\n  {}",
            problems.join("\n  ")
        ));
    }

    let package_root = package_root.unwrap_or_else(|| root.clone());
    let package_name = crate::drt::lean_runner::lake_package_name(&package_root)
        .or_else(|| package_root.file_name().and_then(|n| n.to_str()).map(str::to_string))
        .unwrap_or_else(|| "model".to_string());

    let dispatch = crate::drt::lean_runner::dispatch_for(&entries);
    let opens = namespaces_of(&entries);
    let generated = crate::drt::lean_runner::generate_with_module(
        &root,
        crate::drt::lean_runner::PROJECT_MODULE,
        &relative_from(&crate::drt::lean_runner::package_dir(&root), &package_root),
        &package_name,
        &imports,
        &opens,
        &dispatch,
    )?;

    let tail = if problems.is_empty() {
        String::new()
    } else {
        format!("\nNot covered:\n  {}", problems.join("\n  "))
    };

    match crate::drt::lean_runner::build(&root) {
        Ok(binary) => Ok(format!(
            "Built the model runner at {} covering {} entry point(s).{tail}",
            binary.display(),
            entries.len()
        )),
        Err(e) => Ok(format!(
            "Generated the runner package at {} but could not build it: {e}{tail}",
            generated.display()
        )),
    }
}

/// Does this input schema declare a top-level field with that name?
fn schema_has_field(schema: &crate::drt::Schema, name: &str) -> bool {
    match schema {
        crate::drt::Schema::Struct { fields } => fields.contains_key(name),
        _ => false,
    }
}

/// The distinct namespaces the entries' functions live in, for the runner's
/// `open` lines.
fn namespaces_of(entries: &[crate::drt::lean_runner::ModelEntry]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for entry in entries {
        let Some((prefix, _)) = entry.function.rsplit_once('.') else { continue };
        if !out.iter().any(|n| n == prefix) {
            out.push(prefix.to_string());
        }
    }
    out
}

/// Nearest ancestor containing a lakefile.
fn find_lake_root(from: &std::path::Path) -> Option<PathBuf> {
    let mut current = Some(from);
    while let Some(dir) = current {
        if dir.join("lakefile.lean").exists() || dir.join("lakefile.toml").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// A `../`-style path from one directory to another, for the generated
/// lakefile's `require … from` clause.
fn relative_from(from: &std::path::Path, to: &std::path::Path) -> String {
    let from_parts: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let shared = from_parts
        .iter()
        .zip(to_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let mut parts: Vec<String> = vec!["..".into(); from_parts.len().saturating_sub(shared)];
    for component in &to_parts[shared.min(to_parts.len())..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

/// The requirement DAG as a tree, to an optional depth.
pub fn trace_tree(
    state: &AppState,
    depth: Option<usize>,
) -> Result<Vec<crate::trace::TreeNode>, String> {
    Ok(trace_scan(state)?.tree(depth))
}

/// The project laid out by which requirement each part serves, including the
/// untraced parts — which are the point of the view.
/// The project graph: code nodes, their containment and references, and the
/// requirements, assurance and findings overlaid on them.
///
/// The findings are computed here rather than by the caller so a node's badge
/// and the findings list can never disagree about what is wrong.
pub fn trace_project_graph(
    state: &AppState,
    declaration_cap: Option<usize>,
) -> Result<crate::trace::graph::ProjectGraph, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    Ok(crate::trace::graph::build(
        &root,
        &index,
        &index.findings,
        declaration_cap.unwrap_or(crate::trace::graph::DEFAULT_DECLARATION_CAP),
    ))
}

/// Ask the toolchain whether each `@pins` obligation is actually finished.
///
/// Separate from `trace_scan` because it elaborates Lean, which costs seconds
/// rather than milliseconds. The panel shows the declared state continuously
/// and calls this when somebody asks -- the same split as differential testing,
/// where the binding is always visible and the run is a decision.
pub fn trace_strength_check(
    state: &AppState,
) -> Result<Vec<(String, Option<String>, crate::trace::strength::Strength)>, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    Ok(crate::trace::strength::check(&root, &index)
        .into_iter()
        .map(|((req, clause), state)| (req, clause, state))
        .collect())
}

/// Write the spec-strength obligations a project owes but has not been given.
///
/// A scaffold, never an overwrite: the proofs somebody adds to this file are
/// the entire point of it, so an existing file is left alone and the caller is
/// told what was already there.
pub fn trace_strength_scaffold(state: &AppState, relative_path: &str) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    let obligations = crate::trace::strength::obligations(&root, &index);
    if obligations.is_empty() {
        return Err(
            "nothing to ask: spec strength is a question about proved properties, and no \
             clause has a `@proves` link yet"
                .to_string(),
        );
    }
    let path = root.join(relative_path);
    if path.exists() {
        return Err(format!(
            "{relative_path} already exists, and it is not overwritten — the proofs in it are \
             the point. Delete it first if you want it regenerated."
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let text = crate::trace::strength::render_file(&obligations);
    std::fs::write(&path, text).map_err(|e| format!("writing {relative_path}: {e}"))?;
    let problems: Vec<String> = obligations.iter().flat_map(|o| o.problems.clone()).collect();
    Ok(format!(
        "Wrote {} obligation(s) to {relative_path}. Each arrives as `sorry`: TraceLean can \
         state the question, only you can answer it.{}",
        obligations.len(),
        if problems.is_empty() {
            String::new()
        } else {
            format!(" Problems: {}", problems.join("; "))
        }
    ))
}

/// Import a coverage report from the language's own tool.
///
/// TraceLean measures nothing itself: it reads `cargo llvm-cov --json` or
/// `coverage json` and normalizes it, so adding a language means adding an
/// importer rather than teaching the graph about another tool.
pub fn trace_import_coverage(state: &AppState, path: &str) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let source = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        root.join(path)
    };
    let coverage = crate::trace::coverage::import(&root, &source)?;
    let files = coverage.files.len();
    let written = coverage.save(&root)?;
    Ok(format!(
        "Imported line coverage for {files} file(s) into {}.",
        written.strip_prefix(&root).unwrap_or(&written).display()
    ))
}

/// Import test results from the runner's own output.
pub fn trace_import_test_results(state: &AppState, path: &str) -> Result<String, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let source = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        root.join(path)
    };
    let results = crate::trace::test_results::import(&root, &source)?;
    let total = results.results.len();
    let passing = results
        .results
        .values()
        .filter(|o| **o == crate::trace::test_results::Outcome::Passed)
        .count();
    results.save(&root)?;
    Ok(format!("Imported {total} test result(s), {passing} passing."))
}

/// The role graph: requirements, what models them, what implements them, and
/// what stands behind that.
pub fn trace_role_graph(state: &AppState) -> Result<crate::trace::graph::RoleGraph, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = crate::trace::build(&root);
    let findings = index.findings.clone();
    let coverage = crate::trace::coverage::LineCoverage::load(&root);
    let tests = crate::trace::test_results::TestResults::load(&root);
    let bindings: Vec<(String, Option<String>)> = crate::drt::DrtConfig::load(&root)
        .map(|c| c.bindings.iter().map(|b| (b.req_id.clone(), b.clause.clone())).collect())
        .unwrap_or_default();
    Ok(crate::trace::graph::role_graph(&index, &findings, &coverage, &tests, &bindings))
}

pub fn trace_coverage_map(state: &AppState) -> Result<crate::trace::CoverageMap, String> {
    let root = state.project_root().cloned().ok_or("No project open")?;
    let index = trace_scan(state)?;
    Ok(crate::trace::map::build(&root, &index))
}

/// Links anchored in one file, for the editor gutter.
pub fn trace_links_in_file(
    state: &AppState,
    path: &str,
) -> Result<Vec<crate::trace::Link>, String> {
    let index = trace_scan(state)?;
    Ok(crate::trace::map::links_in_file(&index, std::path::Path::new(path))
        .into_iter()
        .cloned()
        .collect())
}
