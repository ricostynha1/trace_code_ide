//! Context assembler: what to put in front of the model when the subject is a
//! requirement or a file.
//!
//! It reads the annotation index, not a second graph and not the file system's
//! shape. The previous version did both: it asked the trace graph for linked
//! code, and it found the requirement and its model by *building paths* --
//! `reqs/REQ-01.md`, `specs/REQ-01.lean`. That is the filename convention this
//! project exists to replace. On a project laid out any other way, which is
//! every project, it silently assembled nothing and the model was asked to
//! reason about a requirement it had never been shown.

use crate::state::AppState;
use crate::trace::{Role, TraceIndex};
use super::templates::AssembledContext;
use std::path::Path;

/// Default token budget (characters / 4 estimate).
const DEFAULT_TOKEN_BUDGET: u32 = 8000;

/// Assemble context for a requirement: its own text, then the files annotated
/// as its model, implementation and tests.
///
/// Ordered by role rather than by file: the model is what the requirement
/// *means*, so it goes in before an implementation detail when the budget is
/// tight.
pub fn assemble_for_requirement(
    state: &AppState,
    index: &TraceIndex,
    req_id: &str,
    token_budget: Option<u32>,
) -> AssembledContext {
    let budget = token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
    let mut ctx = AssembledContext::new();
    let root = state.project_root().cloned().unwrap_or_default();

    if let Some(requirement) = index.requirements.get(req_id) {
        if let Ok(content) = std::fs::read_to_string(root.join(&requirement.file)) {
            ctx.estimated_tokens += content.len() as u32 / 4;
            ctx.requirement = Some(content);
        }
    }

    let mut seen: Vec<std::path::PathBuf> = Vec::new();
    for role in [Role::Models, Role::Implements, Role::Tests, Role::Drt, Role::Proves] {
        for link in index.links.iter().filter(|l| l.req_id == req_id && l.role == role) {
            if ctx.estimated_tokens >= budget || seen.contains(&link.anchor.file) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(root.join(&link.anchor.file)) else {
                continue;
            };
            seen.push(link.anchor.file.clone());
            let display = link.anchor.file.to_string_lossy().to_string();
            let language = extension_to_language(&display);
            let truncated = truncate_to_budget(&content, budget - ctx.estimated_tokens);
            ctx.estimated_tokens += truncated.len() as u32 / 4;
            // The Lean model is the requirement made executable, so it goes in
            // the slot the templates treat as the specification.
            if role == Role::Models && ctx.spec.is_none() {
                ctx.spec = Some(truncated);
            } else {
                ctx.add_file(display, truncated, language);
            }
        }
    }

    ctx
}

/// Assemble context for a file: the file itself, then the requirements
/// annotated in it and the models those requirements have.
pub fn assemble_for_file(
    state: &AppState,
    index: &TraceIndex,
    file_path: &str,
    token_budget: Option<u32>,
) -> AssembledContext {
    let budget = token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
    let mut ctx = AssembledContext::new();
    let root = state.project_root().cloned().unwrap_or_default();

    if let Ok(content) = std::fs::read_to_string(root.join(file_path)) {
        let language = extension_to_language(file_path);
        let truncated = truncate_to_budget(&content, budget);
        ctx.estimated_tokens += truncated.len() as u32 / 4;
        ctx.add_file(file_path.to_string(), truncated, language);
    }

    // Which requirements this file claims to serve -- from the annotations in
    // it, which is the only thing that knows.
    let here: Vec<&str> = index
        .links
        .iter()
        .filter(|l| l.anchor.file == Path::new(file_path))
        .map(|l| l.req_id.as_str())
        .collect();

    for req_id in here {
        if ctx.estimated_tokens >= budget {
            break;
        }
        if ctx.requirement.is_none() {
            if let Some(requirement) = index.requirements.get(req_id) {
                if let Ok(content) = std::fs::read_to_string(root.join(&requirement.file)) {
                    let truncated = truncate_to_budget(&content, budget - ctx.estimated_tokens);
                    ctx.estimated_tokens += truncated.len() as u32 / 4;
                    ctx.requirement = Some(truncated);
                }
            }
        }
        if ctx.spec.is_none() {
            let model = index
                .links
                .iter()
                .find(|l| l.req_id == req_id && l.role == Role::Models);
            if let Some(model) = model {
                if let Ok(content) = std::fs::read_to_string(root.join(&model.anchor.file)) {
                    let truncated = truncate_to_budget(&content, budget - ctx.estimated_tokens);
                    ctx.estimated_tokens += truncated.len() as u32 / 4;
                    ctx.spec = Some(truncated);
                }
            }
        }
    }

    ctx
}

/// Truncate content to fit within remaining token budget.
fn truncate_to_budget(content: &str, remaining_tokens: u32) -> String {
    let max_chars = (remaining_tokens * 4) as usize;
    if content.len() <= max_chars {
        content.to_string()
    } else {
        let truncated = &content[..max_chars.min(content.len())];
        // Find last newline to avoid cutting mid-line
        if let Some(pos) = truncated.rfind('\n') {
            format!("{}\n\n... [truncated — {} chars omitted]", &truncated[..pos], content.len() - pos)
        } else {
            format!("{}\n\n... [truncated]", truncated)
        }
    }
}

/// Map file extension to language name.
fn extension_to_language(path: &str) -> String {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "rs" => "rust",
        "py" => "python",
        "c" | "cpp" | "cc" | "h" | "hpp" => "cpp",
        "lean" => "lean4",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "html" | "htm" => "html",
        "css" => "css",
        "json" => "json",
        "md" => "markdown",
        _ => ext,
    }.to_string()
}
