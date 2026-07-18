//! AI Agents — Elicitation, Formalisation, Implementation, Repair.
//! Each agent uses a prompt template + context assembler + response parser.
//! Triggered by button press in the UI.
//! 
//! "Standalone" variants take only project_root (no AppState/TraceGraph references)
//! to avoid holding mutex guards across async boundaries.

use super::diff_pipeline::{self, PendingDiff};
use super::provider::{AiProvider, AiRequest, ChatMessage, MessageRole, ModelConfig, ProviderKind};
use super::response_parser;
use super::templates::{self, AssembledContext};
use super::tools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::tools::ToolDefinition;

/// Result from an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    pub agent: String,
    pub success: bool,
    pub message: String,
    /// If the agent produced file diffs, they go here for review.
    pub pending_diffs: Vec<PendingDiff>,
    /// Created file paths (for elicitation).
    pub created_files: Vec<String>,
    /// Raw AI response content.
    pub raw_response: Option<String>,
    /// Token usage from the AI call (for cost tracking).
    pub usage: Option<super::tracking::TokenUsage>,
}

// --- 4.11: Elicitation Agent ---

/// User describes goal → AI suggests structured requirements.
pub async fn run_elicitation_standalone(
    provider: &dyn AiProvider,
    project_root: &Path,
    user_goal: &str,
    extra_tools: &[ToolDefinition],
) -> Result<AgentResult, String> {
    let template = templates::elicitation_template();

    // Assemble context: existing requirements (so AI doesn't duplicate)
    let mut ctx = AssembledContext::new();
    let reqs_dir = project_root.join("reqs");
    if reqs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&reqs_dir) {
            let mut existing = String::new();
            for entry in entries.flatten() {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    let first_line = content.lines().next().unwrap_or("");
                    existing.push_str(&format!("- {}\n", first_line));
                }
            }
            if !existing.is_empty() {
                ctx.extra.push(format!("## Existing Requirements\n{}", existing));
            }
        }
    }

    let mut all_tools = tools::builtin_tool_definitions();
    all_tools.extend_from_slice(extra_tools);

    let mut vars = HashMap::new();
    vars.insert("goal".to_string(), user_goal.to_string());
    vars.insert("context".to_string(), ctx.format_for_prompt());
    vars.insert("tools_prompt".to_string(), tools::tools_as_system_prompt(&all_tools));

    let system = template.render_system(&vars);
    let user_content = template.render(&vars);

    let request = build_request(&system, &user_content);
    let response = provider.complete(&request).await.map_err(|e| e.message)?;

    let parsed = response_parser::parse_response(&response.content);

    Ok(AgentResult {
        agent: "elicitation".to_string(),
        success: true,
        message: format!("Generated requirements from goal. {} text sections, {} code blocks.",
            parsed.text_sections.len(), parsed.code_blocks.len()),
        pending_diffs: Vec::new(),
        created_files: Vec::new(),
        raw_response: Some(response.content),
        usage: Some(response.usage),
    })
}

// --- 4.12: Formalisation Agent ---

/// Requirement → Lean spec draft.
pub async fn run_formalisation_standalone(
    provider: &dyn AiProvider,
    project_root: &Path,
    req_id: &str,
    extra_tools: &[ToolDefinition],
) -> Result<AgentResult, String> {
    let template = templates::formalisation_template();

    // Load requirement
    let req_path = project_root.join("reqs").join(format!("{}.md", req_id));
    let requirement = std::fs::read_to_string(&req_path)
        .map_err(|e| format!("Cannot read requirement {}: {}", req_id, e))?;

    // Load existing types from specs/
    let mut existing_types = String::new();
    let specs_dir = project_root.join("specs");
    if specs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&specs_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().map(|e| e == "lean").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if trimmed.starts_with("structure ")
                                || trimmed.starts_with("inductive ")
                                || trimmed.starts_with("def ")
                                || trimmed.starts_with("theorem ")
                            {
                                existing_types.push_str(line);
                                existing_types.push('\n');
                            }
                        }
                    }
                }
            }
        }
    }

    let mut all_tools = tools::builtin_tool_definitions();
    all_tools.extend_from_slice(extra_tools);

    let mut vars = HashMap::new();
    vars.insert("requirement".to_string(), requirement);
    vars.insert("existing_types".to_string(), if existing_types.is_empty() { "(none)".to_string() } else { existing_types });
    vars.insert("tools_prompt".to_string(), tools::tools_as_system_prompt(&all_tools));

    let system = template.render_system(&vars);
    let user_content = template.render(&vars);

    let request = build_request(&system, &user_content);
    let response = provider.complete(&request).await.map_err(|e| e.message)?;

    // Extract Lean code block
    let parsed = response_parser::parse_response(&response.content);
    let lean_content = parsed.code_blocks.iter()
        .find(|b| b.language == "lean4" || b.language == "lean")
        .map(|b| b.content.clone());

    let spec_file = format!("specs/{}.lean", req_id);
    let spec_full = project_root.join(&spec_file);
    let mut pending_diffs = Vec::new();

    if let Some(new_content) = lean_content {
        if spec_full.exists() {
            let original = std::fs::read_to_string(&spec_full).unwrap_or_default();
            pending_diffs.push(diff_pipeline::create_pending_diff(
                &spec_file, &original, &new_content, "formalisation",
            ));
        } else {
            pending_diffs.push(diff_pipeline::create_pending_diff(
                &spec_file, "", &new_content, "formalisation",
            ));
        }
    }

    Ok(AgentResult {
        agent: "formalisation".to_string(),
        success: true,
        message: format!("Generated Lean spec for {}.", req_id),
        pending_diffs,
        created_files: Vec::new(),
        raw_response: Some(response.content),
        usage: Some(response.usage),
    })
}

// --- 4.13: Implementation Agent ---

/// Lean spec → code in target language.
pub async fn run_implementation_standalone(
    provider: &dyn AiProvider,
    project_root: &Path,
    spec_path: &str,
    language: &str,
    extra_tools: &[ToolDefinition],
) -> Result<AgentResult, String> {
    let template = templates::implementation_template();

    // Load spec
    let spec_full = project_root.join(spec_path);
    let spec_content = std::fs::read_to_string(&spec_full)
        .map_err(|e| format!("Cannot read spec {}: {}", spec_path, e))?;

    let mut all_tools = tools::builtin_tool_definitions();
    all_tools.extend_from_slice(extra_tools);

    let mut vars = HashMap::new();
    vars.insert("spec".to_string(), spec_content);
    vars.insert("language".to_string(), language.to_string());
    vars.insert("context".to_string(), "(no additional context)".to_string());
    vars.insert("tools_prompt".to_string(), tools::tools_as_system_prompt(&all_tools));

    let system = template.render_system(&vars);
    let user_content = template.render(&vars);

    let request = build_request(&system, &user_content);
    let response = provider.complete(&request).await.map_err(|e| e.message)?;

    // Extract code block in target language
    let parsed = response_parser::parse_response(&response.content);
    let code_content = parsed.code_blocks.iter()
        .find(|b| b.language == language || b.language.is_empty())
        .map(|b| b.content.clone());

    let mut pending_diffs = Vec::new();

    if let Some(new_code) = code_content {
        let ext = match language {
            "rust" => "rs",
            "python" => "py",
            "cpp" | "c++" => "cpp",
            "javascript" => "js",
            "typescript" => "ts",
            _ => language,
        };
        let req_id = PathBuf::from(spec_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("impl")
            .to_string();
        let output_file = format!("src/{}.{}", req_id.to_lowercase(), ext);
        let output_full = project_root.join(&output_file);

        let original = if output_full.exists() {
            std::fs::read_to_string(&output_full).unwrap_or_default()
        } else {
            String::new()
        };

        pending_diffs.push(diff_pipeline::create_pending_diff(
            &output_file, &original, &new_code, "implementation",
        ));
    }

    Ok(AgentResult {
        agent: "implementation".to_string(),
        success: true,
        message: format!("Generated {} implementation from spec.", language),
        pending_diffs,
        created_files: Vec::new(),
        raw_response: Some(response.content),
        usage: Some(response.usage),
    })
}

// --- 4.14: Repair Agent ---

/// Given violation → suggest code fix or spec update.
pub async fn run_repair_standalone(
    provider: &dyn AiProvider,
    project_root: &Path,
    file_path: &str,
    violation: &str,
    language: &str,
    extra_tools: &[ToolDefinition],
) -> Result<AgentResult, String> {
    let template = templates::repair_template();

    // Load current code
    let full_path = project_root.join(file_path);
    let code = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("Cannot read {}: {}", file_path, e))?;

    // Try to find linked spec (by filename convention)
    let spec_content = {
        let stem = PathBuf::from(file_path).file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_uppercase();
        let spec_path = project_root.join("specs").join(format!("{}.lean", stem));
        std::fs::read_to_string(&spec_path).unwrap_or_else(|_| "(no spec linked)".to_string())
    };

    let mut all_tools = tools::builtin_tool_definitions();
    all_tools.extend_from_slice(extra_tools);

    let mut vars = HashMap::new();
    vars.insert("violation".to_string(), violation.to_string());
    vars.insert("language".to_string(), language.to_string());
    vars.insert("code".to_string(), code.clone());
    vars.insert("spec".to_string(), spec_content);
    vars.insert("tools_prompt".to_string(), tools::tools_as_system_prompt(&all_tools));

    let system = template.render_system(&vars);
    let user_content = template.render(&vars);

    let request = build_request(&system, &user_content);
    let response = provider.complete(&request).await.map_err(|e| e.message)?;

    // Extract fix from response
    let parsed = response_parser::parse_response(&response.content);
    let mut pending_diffs = Vec::new();

    if let Some(fix_block) = parsed.code_blocks.iter()
        .find(|b| b.language == language || b.language.is_empty())
    {
        pending_diffs.push(diff_pipeline::create_pending_diff(
            file_path, &code, &fix_block.content, "repair",
        ));
    }

    Ok(AgentResult {
        agent: "repair".to_string(),
        success: true,
        message: "Repair suggestion generated.".to_string(),
        pending_diffs,
        created_files: Vec::new(),
        raw_response: Some(response.content),
        usage: Some(response.usage),
    })
}

// --- Helpers ---

/// Build an AiRequest with system + user messages.
fn build_request(system_prompt: &str, user_content: &str) -> AiRequest {
    let model = ModelConfig {
        provider: ProviderKind::Mock,
        model_id: "agent-default".to_string(),
        display_name: "Agent Default".to_string(),
        max_tokens: 4096,
        temperature: 0.2,
        input_cost_per_m: 0.0,
        output_cost_per_m: 0.0,
        cached_input_cost_per_m: 0.0,
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: false,
        ..Default::default()
    };

    AiRequest {
        model,
        messages: vec![
            ChatMessage { role: MessageRole::System, content: system_prompt.to_string(), tool_call_id: None, tool_calls: Vec::new() },
            ChatMessage { role: MessageRole::User, content: user_content.to_string(), tool_call_id: None, tool_calls: Vec::new() },
        ],
        stop: None,
        tools: None,
        cache_breakpoints: Vec::new(),
    }
}
