//! AI IPC commands — chat, settings, models, agents, diff pipeline.

use crate::ai;
use crate::ai::tracking::SessionStats;
use crate::ai::tools::ToolDefinition;
use crate::{
    AppStateWrapper, AiSettingsWrapper, AiLogWrapper, AiSessionStatsWrapper,
    MockProviderWrapper, PendingDiffsWrapper, TraceGraphWrapper, UndoTreeCacheWrapper,
    SymbolTableWrapper, AiSettings, McpClientWrapper, invalidate_undo_cache, get_provider,
    ToolLoopResumeWrapper,
};
use tauri::{AppHandle, Emitter, State};
use std::sync::Arc;

// --- Settings & Models ---

#[tauri::command]
pub fn get_ai_settings(settings: State<'_, AiSettingsWrapper>) -> Result<AiSettings, String> {
    let s = settings.0.lock().map_err(|e| e.to_string())?;
    Ok(s.clone())
}

/// Check which API keys are available via environment variables.
/// Returns a map of provider -> env var name if set.
#[tauri::command]
pub fn detect_env_keys() -> std::collections::HashMap<String, String> {
    let mut detected = std::collections::HashMap::new();
    if std::env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok() {
        detected.insert("bedrock".into(), "AWS_BEARER_TOKEN_BEDROCK".into());
    }
    if std::env::var("OPENROUTER_API_KEY").is_ok() {
        detected.insert("openrouter".into(), "OPENROUTER_API_KEY".into());
    }
    detected
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

    let (or_key, br_token, br_region) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        (
            s.openrouter_api_key.clone(),
            s.bedrock_api_key.clone(),
            s.bedrock_region.clone(),
        )
    };

    let mut models = Vec::new();

    let mock = ai::mock::MockProvider::new().0;
    if let Ok(m) = mock.list_models().await {
        models.extend(m);
    }

    if let Some(key) = or_key.or_else(|| std::env::var("OPENROUTER_API_KEY").ok()) {
        let or = ai::openrouter::OpenRouterProvider::new(key);
        match or.list_models().await {
            Ok(m) => models.extend(m),
            Err(e) => eprintln!("OpenRouter list_models failed: {}", e.message),
        }
    }

    if let Some(token) = br_token.or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok()) {
        let br = ai::bedrock::BedrockProvider::new(token, br_region);
        match br.list_models().await {
            Ok(m) => models.extend(m),
            Err(e) => eprintln!("Bedrock list_models failed: {}", e.message),
        }
    }

    // Enrich with catalog data (coding_index, rank, caching, tools)
    for model in &mut models {
        ai::model_catalog::enrich(model);
    }

    // Sort by coding_rank (ranked models first, unranked last)
    models.sort_by(|a, b| {
        match (a.coding_rank, b.coding_rank) {
            (Some(ra), Some(rb)) => ra.cmp(&rb),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.display_name.cmp(&b.display_name),
        }
    });

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

#[tauri::command]
pub async fn ai_chat(
    app: AppHandle,
    settings: State<'_, AiSettingsWrapper>,
    log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    _mock_provider: State<'_, MockProviderWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    resume_state: State<'_, ToolLoopResumeWrapper>,
    messages: Vec<ai::provider::ChatMessage>,
) -> Result<ai::AiResponse, String> {
    use tracelean_core::agent::{AgentContext, run_agent_turn};

    // Snapshot project root
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().unwrap_or_default()
    };

    // Collect MCP tool definitions (read-only snapshot)
    let extra_tools: Vec<ai::tools::ToolDefinition> = {
        let mgr = mcp_client.0.lock().await;
        mgr.all_tools().into_iter().map(|(_server, tool)| tool).collect()
    };

    let spend_cap = settings.0.lock().map_err(|e| e.to_string())?.spend_cap_usd;

    let ctx = AgentContext {
        state: state.0.clone(),
        symbols: symbols_state.0.clone(),
        graph: graph_state.0.clone(),
        settings: settings.0.clone(),
        stats: stats.0.clone(),
        log: log_state.0.clone(),
        extra_tools,
        permissions: tracelean_core::AgentPermissions::full_access("chat"),
        project_root,
        event_sink: Arc::new(crate::TauriEventSink { app_handle: app.clone() }),
        spend_cap_usd: spend_cap,
        pause_handler: Some(Arc::new(TauriPauseHandler {
            app_handle: app.clone(),
            resume_state: resume_state.0.clone(),
        })),
        verbose: false,
        retention_engine: Arc::new(std::sync::Mutex::new(tracelean_core::ai::RetentionEngine::with_defaults())),
        timing_tracker: Arc::new(std::sync::Mutex::new(tracelean_core::ai::TurnTimingTracker::new())),
    };

    let result = run_agent_turn(&ctx, messages).await.map_err(|e| e.to_string())?;

    // Invalidate undo cache after tool execution
    invalidate_undo_cache(&cache);

    Ok(result.response)
}

// --- Session-Based Chat (persistent model_view across calls) ---

/// Chat with persistent session state. Compaction evolves across calls (preserves caching).
/// Frontend sends only session_id + new user message. Backend owns the conversation state.
#[tauri::command]
pub async fn ai_chat_session(
    app: AppHandle,
    settings: State<'_, AiSettingsWrapper>,
    log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    _mock_provider: State<'_, MockProviderWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    resume_state: State<'_, ToolLoopResumeWrapper>,
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    session_id: String,
    user_message: String,
) -> Result<ai::AiResponse, String> {
    use tracelean_core::agent::{AgentContext, run_agent_turn_session};
    use tracelean_core::ai::provider::{ChatMessage, MessageRole};
    use tracelean_core::ChatSession;

    // Snapshot project root
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().unwrap_or_default()
    };

    // Collect MCP tool definitions
    let extra_tools: Vec<ai::tools::ToolDefinition> = {
        let mgr = mcp_client.0.lock().await;
        mgr.all_tools().into_iter().map(|(_server, tool)| tool).collect()
    };

    let spend_cap = settings.0.lock().map_err(|e| e.to_string())?.spend_cap_usd;

    let ctx = AgentContext {
        state: state.0.clone(),
        symbols: symbols_state.0.clone(),
        graph: graph_state.0.clone(),
        settings: settings.0.clone(),
        stats: stats.0.clone(),
        log: log_state.0.clone(),
        extra_tools,
        permissions: tracelean_core::AgentPermissions::full_access("chat"),
        project_root,
        event_sink: Arc::new(crate::TauriEventSink { app_handle: app.clone() }),
        spend_cap_usd: spend_cap,
        pause_handler: Some(Arc::new(TauriPauseHandler {
            app_handle: app.clone(),
            resume_state: resume_state.0.clone(),
        })),
        verbose: false,
        retention_engine: Arc::new(std::sync::Mutex::new(tracelean_core::ai::RetentionEngine::with_defaults())),
        timing_tracker: Arc::new(std::sync::Mutex::new(tracelean_core::ai::TurnTimingTracker::new())),
    };

    // Get or create session
    let mut store = session_store.0.lock().await;
    let session = store.entry(session_id.clone())
        .or_insert_with(|| ChatSession::new(session_id.clone()));

    // Append user message to both views
    let user_msg = ChatMessage {
        role: MessageRole::User,
        content: user_message,
        tool_call_id: None,
        tool_calls: Vec::new(),
    };
    session.append(user_msg);

    // Run agent turn with persistent session
    let result = run_agent_turn_session(&ctx, session).await.map_err(|e| e.to_string())?;

    // Drop lock before other operations
    drop(store);

    invalidate_undo_cache(&cache);

    Ok(result.response)
}

/// Reset a chat session (new conversation). Clears both user_view and model_view.
#[tauri::command]
pub async fn reset_chat_session(
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    session_id: String,
) -> Result<(), String> {
    let mut store = session_store.0.lock().await;
    store.remove(&session_id);
    Ok(())
}

/// Get the raw user_view messages for a session (what user sees in chat panel).
#[tauri::command]
pub async fn get_chat_session_messages(
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    session_id: String,
) -> Result<Vec<ai::provider::ChatMessage>, String> {
    let store = session_store.0.lock().await;
    match store.get(&session_id) {
        Some(session) => Ok(session.user_view.clone()),
        None => Ok(Vec::new()),
    }
}

/// PauseHandler implementation for Tauri — emits event and waits for user response.
pub struct TauriPauseHandler {
    pub app_handle: AppHandle,
    pub resume_state: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<bool>>>>,
}

#[async_trait::async_trait]
impl tracelean_core::agent::PauseHandler for TauriPauseHandler {
    async fn should_continue(&self, tool_calls_so_far: usize) -> bool {
        let _ = self.app_handle.emit("tool-loop-pause", serde_json::json!({
            "loops_completed": tool_calls_so_far,
            "message": format!("{} tool calls executed. Continue?", tool_calls_so_far)
        }));

        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        {
            let mut guard = self.resume_state.lock().await;
            *guard = Some(tx);
        }

        match rx.await {
            Ok(cont) => cont,
            _ => false,
        }
    }
}

// --- Tool Loop Resume ---

/// Called by frontend when user clicks "Continue" or "Stop" on tool loop pause prompt.
#[tauri::command]
pub async fn resume_tool_loop(
    resume_state: State<'_, ToolLoopResumeWrapper>,
    should_continue: bool,
) -> Result<(), String> {
    let mut guard = resume_state.0.lock().await;
    if let Some(tx) = guard.take() {
        let _ = tx.send(should_continue);
    }
    Ok(())
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

/// Get provider + model config (needed for cost calculation).
async fn get_provider_and_model(
    settings: &tauri::State<'_, AiSettingsWrapper>,
    mock_provider: &tauri::State<'_, MockProviderWrapper>,
) -> Result<(Box<dyn ai::provider::AiProvider + Send + Sync>, ai::ModelConfig), String> {
    let model = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        s.selected_model.clone().ok_or("No model selected")?
    };
    let provider = get_provider(settings, mock_provider).await?;
    Ok((provider, model))
}

/// Record token usage + cost from agent result into session stats, emit event.
fn record_agent_stats(
    stats: &tauri::State<'_, AiSessionStatsWrapper>,
    model: &ai::ModelConfig,
    result: &ai::agents::AgentResult,
    app: &AppHandle,
) {
    if let Some(ref usage) = result.usage {
        let cost = usage.estimate_cost(
            model.input_cost_per_m,
            model.output_cost_per_m,
            model.cached_input_cost_per_m,
        );
        if let Ok(mut s) = stats.0.lock() {
            s.record(usage, &cost);
        }
        // Emit cost-updated event so frontend can refresh display
        let _ = app.emit("ai-stats-updated", ());
    }
}

#[tauri::command]
pub async fn run_agent_elicitation(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    _log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    user_goal: String,
) -> Result<ai::agents::AgentResult, String> {
    let (provider, model) = get_provider_and_model(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    let result = ai::agents::run_elicitation_standalone(provider.as_ref(), &project_root, &user_goal, &extra_tools).await?;
    record_agent_stats(&stats, &model, &result, &app);
    Ok(result)
}

#[tauri::command]
pub async fn run_agent_formalisation(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    req_id: String,
) -> Result<ai::agents::AgentResult, String> {
    let (provider, model) = get_provider_and_model(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    let result = ai::agents::run_formalisation_standalone(provider.as_ref(), &project_root, &req_id, &extra_tools).await?;
    record_agent_stats(&stats, &model, &result, &app);
    Ok(result)
}

#[tauri::command]
pub async fn run_agent_implementation(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    spec_path: String,
    language: String,
) -> Result<ai::agents::AgentResult, String> {
    let (provider, model) = get_provider_and_model(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    let result = ai::agents::run_implementation_standalone(provider.as_ref(), &project_root, &spec_path, &language, &extra_tools).await?;
    record_agent_stats(&stats, &model, &result, &app);
    Ok(result)
}

#[tauri::command]
pub async fn run_agent_repair(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    _graph_state: State<'_, TraceGraphWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    mock_provider: State<'_, MockProviderWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    file_path: String,
    violation: String,
    language: String,
) -> Result<ai::agents::AgentResult, String> {
    let (provider, model) = get_provider_and_model(&settings, &mock_provider).await?;
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let extra_tools = get_mcp_client_tools(&mcp_client).await;
    let result = ai::agents::run_repair_standalone(provider.as_ref(), &project_root, &file_path, &violation, &language, &extra_tools).await?;
    record_agent_stats(&stats, &model, &result, &app);
    Ok(result)
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
