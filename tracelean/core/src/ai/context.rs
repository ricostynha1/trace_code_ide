//! Context assembler — pulls from trace graph to build AI request context.
//! Respects token budget, ranks by relevance to target requirement/file.

use crate::state::AppState;
use crate::trace_graph::TraceGraph;
use super::templates::AssembledContext;
use std::path::{Path, PathBuf};

/// Default token budget (characters / 4 estimate).
const DEFAULT_TOKEN_BUDGET: u32 = 8000;

/// Assemble context for a requirement target.
/// Pulls: requirement text → linked spec → linked code → linked tests.
pub fn assemble_for_requirement(
    state: &AppState,
    graph: &TraceGraph,
    req_id: &str,
    token_budget: Option<u32>,
) -> AssembledContext {
    let budget = token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
    let mut ctx = AssembledContext::new();
    let root = state.project_root().cloned().unwrap_or_default();

    // 1. Requirement text (highest priority)
    let req_path = root.join("reqs").join(format!("{}.md", req_id));
    if let Ok(content) = std::fs::read_to_string(&req_path) {
        ctx.requirement = Some(content.clone());
        ctx.estimated_tokens += content.len() as u32 / 4;
    }

    // 2. Linked spec
    let spec_path = root.join("specs").join(format!("{}.lean", req_id));
    if let Ok(content) = std::fs::read_to_string(&spec_path) {
        ctx.spec = Some(content.clone());
        ctx.estimated_tokens += content.len() as u32 / 4;
    }

    // 3. Linked code elements (via trace graph)
    if let Some(trace) = graph.query_requirement_owned(req_id) {
        for code_elem in &trace.code {
            if ctx.estimated_tokens >= budget {
                break;
            }
            let code_path = code_elem.file.to_string_lossy().to_string();
            let full_path = root.join(&code_elem.file);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let lang = extension_to_language(&code_path);
                ctx.add_file(code_path, truncate_to_budget(&content, budget - ctx.estimated_tokens), lang);
            }
        }

        // 4. Linked tests
        for test in &trace.tests {
            if ctx.estimated_tokens >= budget {
                break;
            }
            let test_path = test.file.to_string_lossy().to_string();
            let full_path = root.join(&test.file);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let lang = extension_to_language(&test_path);
                ctx.add_file(test_path, truncate_to_budget(&content, budget - ctx.estimated_tokens), lang);
            }
        }
    }

    ctx
}

/// Assemble context for a specific file (code element target).
/// Pulls: the file itself → linked requirement → linked spec.
pub fn assemble_for_file(
    state: &AppState,
    graph: &TraceGraph,
    file_path: &str,
    token_budget: Option<u32>,
) -> AssembledContext {
    let budget = token_budget.unwrap_or(DEFAULT_TOKEN_BUDGET);
    let mut ctx = AssembledContext::new();
    let root = state.project_root().cloned().unwrap_or_default();

    // 1. The file itself
    let full_path = root.join(file_path);
    if let Ok(content) = std::fs::read_to_string(&full_path) {
        let lang = extension_to_language(file_path);
        ctx.add_file(file_path.to_string(), truncate_to_budget(&content, budget), lang);
    }

    // 2. Try to find linked requirement via code trace
    // Use empty name to match any code element in this file
    if let Some(trace) = graph.query_code_element_owned(&PathBuf::from(file_path), "") {
        if let Some(req) = trace.requirements.first() {
            let req_id = &req.id;
            let req_path = root.join("reqs").join(format!("{}.md", req_id));
            if let Ok(content) = std::fs::read_to_string(&req_path) {
                if ctx.estimated_tokens < budget {
                    let truncated = truncate_to_budget(&content, budget - ctx.estimated_tokens);
                    ctx.estimated_tokens += truncated.len() as u32 / 4;
                    ctx.requirement = Some(truncated);
                }
            }
            // Linked spec
            let spec_path = root.join("specs").join(format!("{}.lean", req_id));
            if let Ok(content) = std::fs::read_to_string(&spec_path) {
                if ctx.estimated_tokens < budget {
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
