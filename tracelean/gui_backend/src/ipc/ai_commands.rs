//! AI IPC commands — chat, settings, models, agents, diff pipeline.

use crate::ai;
use crate::ai::tracking::SessionStats;
use crate::ai::tools::ToolDefinition;
use crate::{
    AppStateWrapper, AiSettingsWrapper, AiLogWrapper, AiSessionStatsWrapper,
    MockProviderWrapper, PendingDiffsWrapper, TraceGraphWrapper, UndoTreeCacheWrapper,
    SymbolTableWrapper, AiSettings, McpClientWrapper, invalidate_undo_cache, get_provider,
    ToolCallEvent, ToolCallStatus,
};
use tauri::{AppHandle, Emitter, State};

// --- Settings & Models ---

#[tauri::command]
pub fn get_ai_settings(settings: State<'_, AiSettingsWrapper>) -> Result<AiSettings, String> {
    let s = settings.0.lock().map_err(|e| e.to_string())?;
    Ok(s.clone())
}

#[tauri::command]
pub fn update_ai_settings(settings: State<'_, AiSettingsWrapper>, new_settings: AiSettings) -> Result<(), String> {
    let mut s = settings.0.lock().map_err(|e| e.to_string())?;
    *s = new_settings;
    Ok(())
}

#[tauri::command]
pub async fn get_ai_models(settings: State<'_, AiSettingsWrapper>) -> Result<Vec<ai::ModelConfig>, String> {
    use ai::provider::AiProvider;

    let (or_key, br_access, br_secret, br_region) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        (
            s.openrouter_api_key.clone(),
            s.bedrock_access_key.clone(),
            s.bedrock_secret_key.clone(),
            s.bedrock_region.clone(),
        )
    };

    let mut models = Vec::new();

    let mock = ai::mock::MockProvider::new().0;
    if let Ok(m) = mock.list_models().await {
        models.extend(m);
    }

    if let Some(key) = or_key {
        let or = ai::openrouter::OpenRouterProvider::new(key);
        match or.list_models().await {
            Ok(m) => models.extend(m),
            Err(e) => eprintln!("OpenRouter list_models failed: {}", e.message),
        }
    }

    if let Some(access) = br_access {
        let secret = br_secret.unwrap_or_default();
        let region = br_region.unwrap_or_else(|| "us-east-1".into());
        let br = ai::bedrock::BedrockProvider::new(access, secret, region);
        match br.list_models().await {
            Ok(m) => models.extend(m),
            Err(e) => eprintln!("Bedrock list_models failed: {}", e.message),
        }
    }

    Ok(models)
}

#[tauri::command]
pub fn get_ai_session_stats(stats: State<'_, AiSessionStatsWrapper>) -> Result<SessionStats, String> {
    let s = stats.0.lock().map_err(|e| e.to_string())?;
    Ok(s.clone())
}

// --- Interaction Log ---

#[tauri::command]
pub fn get_ai_interaction_log(
    log_state: State<'_, AiLogWrapper>,
    limit: Option<usize>,
) -> Result<Vec<ai::InteractionEntry>, String> {
    let log = log_state.0.lock().map_err(|e| e.to_string())?;
    let entries: Vec<ai::InteractionEntry> = log.recent(limit.unwrap_or(50))
        .into_iter().cloned().collect();
    Ok(entries)
}

#[tauri::command]
pub fn get_ai_interaction_detail(
    log_state: State<'_, AiLogWrapper>,
    id: String,
) -> Result<Option<ai::InteractionEntry>, String> {
    let log = log_state.0.lock().map_err(|e| e.to_string())?;
    Ok(log.get(&id).cloned())
}

// --- Mock Provider ---

#[tauri::command]
pub async fn get_mock_pending(
    mock_provider: State<'_, MockProviderWrapper>,
) -> Result<Vec<ai::mock::MockPendingRequest>, String> {
    let provider = {
        let guard = mock_provider.0.lock().await;
        guard.clone()
    };
    if let Some(ref p) = provider {
        Ok(p.get_pending().await)
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn mock_submit_response(
    mock_provider: State<'_, MockProviderWrapper>,
    request_id: String,
    response: String,
) -> Result<(), String> {
    let provider = {
        let guard = mock_provider.0.lock().await;
        guard.clone()
    };
    if let Some(ref p) = provider {
        p.submit_response(&request_id, response).await
    } else {
        Err("Mock provider not initialized".into())
    }
}

// --- Chat ---

/// If no system message exists in the conversation, prepend one with tool definitions
/// (builtin + MCP client tools) so the model knows how to call tools.
async fn inject_tools_system_prompt(
    mut messages: Vec<ai::provider::ChatMessage>,
    mcp_client: &tauri::State<'_, McpClientWrapper>,
) -> Vec<ai::provider::ChatMessage> {
    let has_system = messages.iter().any(|m| m.role == ai::provider::MessageRole::System);
    if !has_system {
        let mut tools = ai::tools::builtin_tool_definitions();
        let mgr = mcp_client.0.lock().await;
        let mcp_tools: Vec<ToolDefinition> = mgr.all_tools().into_iter().map(|(_s, t)| t).collect();
        tools.extend(mcp_tools);

        let system_content = format!(
            "You are an AI assistant inside the TraceLean IDE. You help with requirements engineering, \
             formal specification, implementation, and repair.\n\n{}",
            ai::tools::tools_as_system_prompt(&tools)
        );
        messages.insert(0, ai::provider::ChatMessage {
            role: ai::provider::MessageRole::System,
            content: system_content,
        });
    }
    messages
}

/// Parse tool_call code blocks from an AI response.
/// Parses AgentToolCall (MCP call + optional ui metadata).
fn extract_tool_calls(content: &str) -> Vec<ai::tools::AgentToolCall> {
    let mut calls = Vec::new();
    let blocks = ai::templates::extract_code_blocks(content);
    for block in &blocks {
        if block.language == "tool_call" {
            if let Ok(tc) = serde_json::from_str::<ai::tools::AgentToolCall>(&block.content) {
                calls.push(tc);
            }
        }
    }
    calls
}

#[tauri::command]
pub async fn ai_chat(
    app: AppHandle,
    settings: State<'_, AiSettingsWrapper>,
    log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    messages: Vec<ai::provider::ChatMessage>,
) -> Result<ai::AiResponse, String> {
    use ai::provider::{AiProvider, MessageRole};

    let (model, provider_kind, or_key, br_access, br_secret, br_region) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        let model = s.selected_model.clone().ok_or("No model selected")?;
        (
            model,
            s.active_provider.clone(),
            s.openrouter_api_key.clone(),
            s.bedrock_access_key.clone(),
            s.bedrock_secret_key.clone(),
            s.bedrock_region.clone(),
        )
    };

    // Inject system prompt with tool definitions if no system message present
    let mut messages = inject_tools_system_prompt(messages, &mcp_client).await;

    let max_tool_loops = 10;
    let mut consecutive_failures: u32 = 0;

    for _loop_i in 0..max_tool_loops {
        let request = ai::AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            stop: None,
        };

        let start = std::time::Instant::now();

        let result: Result<ai::AiResponse, ai::provider::AiError> = match provider_kind {
            ai::ProviderKind::OpenRouter => {
                let key = or_key.clone().ok_or("OpenRouter API key not set")?;
                let provider = ai::openrouter::OpenRouterProvider::new(key);
                provider.complete(&request).await
            }
            ai::ProviderKind::Bedrock => {
                let access = br_access.clone().ok_or("Bedrock access key not set")?;
                let secret = br_secret.clone().ok_or("Bedrock secret key not set")?;
                let region = br_region.clone().unwrap_or_else(|| "us-east-1".into());
                let provider = ai::bedrock::BedrockProvider::new(access, secret, region);
                provider.complete(&request).await
            }
            ai::ProviderKind::Mock => {
                let provider = {
                    let mut guard = mock_provider.0.lock().await;
                    if guard.is_none() {
                        let (p, _rx) = ai::mock::MockProvider::new();
                        *guard = Some(std::sync::Arc::new(p));
                    }
                    guard.as_ref().unwrap().clone()
                };
                provider.complete(&request).await
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                let cost = response.usage.estimate_cost(
                    model.input_cost_per_m,
                    model.output_cost_per_m,
                    model.cached_input_cost_per_m,
                );
                {
                    let mut log = log_state.0.lock().map_err(|e| e.to_string())?;
                    log.record_success("chat", &request, &response, duration_ms);
                }
                {
                    let mut s = stats.0.lock().map_err(|e| e.to_string())?;
                    s.record(&response.usage, &cost);
                }

                // Check for tool_call blocks in the response
                let tool_calls = extract_tool_calls(&response.content);
                if tool_calls.is_empty() {
                    // No tool calls — final response
                    return Ok(response);
                }

                // Execute tool calls and build results
                messages.push(ai::provider::ChatMessage {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                });

                let mut tool_results = String::new();
                for tc in &tool_calls {
                    let reason = tc.reason().map(|s| s.to_string());
                    let mcp_call = tc.to_mcp_call();

                    // Emit tool call message to frontend chat
                    let reason_text = reason.as_deref().unwrap_or("");
                    let _ = app.emit("ai-chat-message", serde_json::json!({
                        "role": "system",
                        "content": format!("call {} {}", mcp_call.name, reason_text)
                    }));

                    // Emit "running" event
                    let _ = app.emit("tool-call", ToolCallEvent {
                        tool_name: mcp_call.name.clone(),
                        status: ToolCallStatus::Running,
                        duration_ms: None,
                        depth: 0,
                        reason: reason.clone(),
                    });

                    let start_tool = std::time::Instant::now();
                    let result = {
                        let mut s = state.0.lock().map_err(|e| e.to_string())?;
                        let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
                        let g = graph_state.0.lock().map_err(|e| e.to_string())?;
                        let root = s.project_root().cloned().unwrap_or_default();
                        let perms = ai::tool_executor::AgentPermissions::full_access("chat");
                        ai::tool_executor::execute_tool(&mcp_call, &root, &mut s, &sym, &g, &perms)
                    };
                    let tool_duration = start_tool.elapsed().as_millis() as u64;

                    // Emit tool response message to frontend chat
                    let _ = app.emit("ai-chat-message", serde_json::json!({
                        "role": "system",
                        "content": format!("rsp {}", mcp_call.name)
                    }));

                    // Emit "completed" or "failed" event
                    let status = if result.success {
                        ToolCallStatus::Completed
                    } else {
                        ToolCallStatus::Failed { error: result.content.clone() }
                    };
                    let _ = app.emit("tool-call", ToolCallEvent {
                        tool_name: mcp_call.name.clone(),
                        status,
                        duration_ms: Some(tool_duration),
                        depth: 0,
                        reason,
                    });

                    // P3-T1: MCP-standard JSON tool response
                    let tool_response = serde_json::json!({
                        "type": "tool_result",
                        "tool_name": mcp_call.name,
                        "is_error": !result.success,
                        "content": [{
                            "type": "text",
                            "text": result.content
                        }]
                    });
                    tool_results.push_str(&serde_json::to_string(&tool_response).unwrap_or_default());
                    tool_results.push_str("\n");

                    // P3-T2: Track consecutive failures
                    if result.success {
                        consecutive_failures = 0;
                    } else {
                        consecutive_failures += 1;
                    }
                    if consecutive_failures >= 6 {
                        break;
                    }
                }

                // P3-T2: Stop if too many consecutive failures
                if consecutive_failures >= 6 {
                    let failure_msg = format!(
                        "{}\n\n[Tool execution stopped: {} consecutive failures. Please provide additional guidance.]",
                        response.content, consecutive_failures
                    );
                    return Ok(ai::AiResponse {
                        content: failure_msg,
                        usage: response.usage.clone(),
                        raw_response: response.raw_response.clone(),
                        truncated: response.truncated,
                    });
                }

                // Invalidate undo cache and notify frontend after tool execution
                invalidate_undo_cache(&cache);
                let _ = app.emit("undo-tree-changed", ());

                messages.push(ai::provider::ChatMessage {
                    role: MessageRole::User,
                    content: tool_results.clone(),
                });
                // Loop to send results back to AI
            }
            Err(e) => {
                let mut log = log_state.0.lock().map_err(|e2| e2.to_string())?;
                log.record_failure("chat", &request, &e, duration_ms);
                return Err(e.message);
            }
        }
    }

    Err("Max tool call loops exceeded (10).".into())
}

// --- Templates & Context ---

#[tauri::command]
pub fn get_prompt_templates() -> Vec<ai::templates::PromptTemplate> {
    vec![
        ai::templates::elicitation_template(),
        ai::templates::formalisation_template(),
        ai::templates::implementation_template(),
        ai::templates::repair_template(),
    ]
}

#[tauri::command]
pub fn assemble_context_for_requirement(
    state: State<'_, AppStateWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    req_id: String,
    token_budget: Option<u32>,
) -> Result<ai::templates::AssembledContext, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(ai::context::assemble_for_requirement(&s, &g, &req_id, token_budget))
}

#[tauri::command]
pub fn assemble_context_for_file(
    state: State<'_, AppStateWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    file_path: String,
    token_budget: Option<u32>,
) -> Result<ai::templates::AssembledContext, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    Ok(ai::context::assemble_for_file(&s, &g, &file_path, token_budget))
}

// --- Agent Commands ---

/// Collect tool definitions from connected MCP client servers.
async fn get_mcp_client_tools(client: &tauri::State<'_, McpClientWrapper>) -> Vec<ToolDefinition> {
    let mgr = client.0.lock().await;
    mgr.all_tools().into_iter().map(|(_server, tool)| tool).collect()
}

#[tauri::command]
pub async fn run_agent_elicitation(
    _app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    _log_state: State<'_, AiLogWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    user_goal: String,
) -> Result<ai::agents::AgentResult, String> {
    let provider = get_provider(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    ai::agents::run_elicitation_standalone(provider.as_ref(), &project_root, &user_goal, &extra_tools).await
}

#[tauri::command]
pub async fn run_agent_formalisation(
    _app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    req_id: String,
) -> Result<ai::agents::AgentResult, String> {
    let provider = get_provider(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    ai::agents::run_formalisation_standalone(provider.as_ref(), &project_root, &req_id, &extra_tools).await
}

#[tauri::command]
pub async fn run_agent_implementation(
    _app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    spec_path: String,
    language: String,
) -> Result<ai::agents::AgentResult, String> {
    let provider = get_provider(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    ai::agents::run_implementation_standalone(provider.as_ref(), &project_root, &spec_path, &language, &extra_tools).await
}

#[tauri::command]
pub async fn run_agent_repair(
    _app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    file_path: String,
    violation: String,
    language: String,
) -> Result<ai::agents::AgentResult, String> {
    let provider = get_provider(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    ai::agents::run_repair_standalone(provider.as_ref(), &project_root, &file_path, &violation, &language, &extra_tools).await
}

// --- Diff Pipeline ---

#[tauri::command]
pub fn get_pending_diffs(
    diffs: State<'_, PendingDiffsWrapper>,
) -> Result<Vec<ai::diff_pipeline::PendingDiff>, String> {
    let d = diffs.0.lock().map_err(|e| e.to_string())?;
    Ok(d.clone())
}

#[tauri::command]
pub fn accept_diff_hunk(
    diffs: State<'_, PendingDiffsWrapper>,
    diff_id: String,
    hunk_id: String,
) -> Result<(), String> {
    let mut d = diffs.0.lock().map_err(|e| e.to_string())?;
    for diff in d.iter_mut() {
        if diff.id == diff_id {
            for hunk in diff.hunks.iter_mut() {
                if hunk.id == hunk_id {
                    hunk.accepted = true;
                    return Ok(());
                }
            }
        }
    }
    Err("Hunk not found".into())
}

#[tauri::command]
pub fn reject_diff_hunk(
    diffs: State<'_, PendingDiffsWrapper>,
    diff_id: String,
    hunk_id: String,
) -> Result<(), String> {
    let mut d = diffs.0.lock().map_err(|e| e.to_string())?;
    for diff in d.iter_mut() {
        if diff.id == diff_id {
            for hunk in diff.hunks.iter_mut() {
                if hunk.id == hunk_id {
                    hunk.accepted = false;
                    return Ok(());
                }
            }
        }
    }
    Err("Hunk not found".into())
}

#[tauri::command]
pub fn apply_accepted_hunks(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    diffs: State<'_, PendingDiffsWrapper>,
    diff_id: String,
) -> Result<String, String> {
    let mut d = diffs.0.lock().map_err(|e| e.to_string())?;
    let diff_idx = d.iter().position(|x| x.id == diff_id).ok_or("Diff not found")?;
    let diff = &d[diff_idx];

    let commands = ai::diff_pipeline::accepted_hunks_to_commands(diff);
    let accepted_count = diff.hunks.iter().filter(|h| h.accepted).count();

    if commands.is_empty() {
        return Err("No accepted hunks to apply".into());
    }

    {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        for cmd in commands {
            s.apply(cmd);
        }
    }

    d.remove(diff_idx);

    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());

    Ok(format!("Applied {} accepted hunks.", accepted_count))
}

#[tauri::command]
pub fn discard_pending_diff(
    diffs: State<'_, PendingDiffsWrapper>,
    diff_id: String,
) -> Result<(), String> {
    let mut d = diffs.0.lock().map_err(|e| e.to_string())?;
    d.retain(|x| x.id != diff_id);
    Ok(())
}
