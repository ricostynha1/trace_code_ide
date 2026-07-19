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
use tracelean_core::ai::AiService;

/// P8 (D8.1): assemble the core AiService from Tauri-managed state. All AI
/// orchestration lives in core; commands below are argument marshalling.
#[allow(clippy::too_many_arguments)]
fn ai_service(
    app: &AppHandle,
    settings: &State<'_, AiSettingsWrapper>,
    log_state: &State<'_, AiLogWrapper>,
    stats: &State<'_, AiSessionStatsWrapper>,
    mcp_client: &State<'_, McpClientWrapper>,
    state: &State<'_, AppStateWrapper>,
    symbols_state: &State<'_, SymbolTableWrapper>,
    graph_state: &State<'_, TraceGraphWrapper>,
    session_store: &State<'_, crate::ChatSessionStoreWrapper>,
    diffs: &State<'_, crate::PendingDiffsWrapper>,
) -> AiService {
    AiService {
        state: state.0.clone(),
        symbols: symbols_state.0.clone(),
        graph: graph_state.0.clone(),
        settings: settings.0.clone(),
        stats: stats.0.clone(),
        log: log_state.0.clone(),
        sessions: session_store.0.clone(),
        mcp_client: mcp_client.0.clone(),
        event_sink: Arc::new(crate::TauriEventSink { app_handle: app.clone() }),
        pending_diffs: diffs.0.clone(),
    }
}

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
pub fn update_ai_settings(
    settings: State<'_, AiSettingsWrapper>,
    state: State<'_, AppStateWrapper>,
    new_settings: AiSettings,
) -> Result<(), String> {
    {
        let mut s = settings.0.lock().map_err(|e| e.to_string())?;
        *s = new_settings.clone();
    }
    // Persist so the selected model/provider survives restarts. Project open →
    // {project}/.tracelean/ai_settings.json; otherwise the home fallback.
    let root = state.0.lock().ok().and_then(|s| s.project_root().cloned());
    let result = match &root {
        Some(root) => tracelean_core::ai::service::save_settings_to(root, &new_settings),
        None => tracelean_core::ai::service::save_settings(&new_settings),
    };
    if let Err(e) = result {
        eprintln!("failed to persist ai settings: {}", e);
    }
    Ok(())
}

#[tauri::command]
pub async fn get_ai_models(settings: State<'_, AiSettingsWrapper>) -> Result<Vec<ai::ModelConfig>, String> {
    let snapshot = settings.0.lock().map_err(|e| e.to_string())?.clone();
    Ok(tracelean_core::ai::service::list_models_for(&snapshot).await)
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
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    diffs: State<'_, crate::PendingDiffsWrapper>,
    messages: Vec<ai::provider::ChatMessage>,
) -> Result<ai::AiResponse, String> {
    let svc = ai_service(
        &app, &settings, &log_state, &stats, &mcp_client,
        &state, &symbols_state, &graph_state, &session_store, &diffs,
    );
    let pause = Arc::new(TauriPauseHandler {
        app_handle: app.clone(),
        resume_state: resume_state.0.clone(),
    });
    let response = svc.one_shot_turn(messages, Some(pause), false).await?;
    invalidate_undo_cache(&cache);
    Ok(response)
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
    diffs: State<'_, crate::PendingDiffsWrapper>,
    session_id: String,
    user_message: String,
) -> Result<ai::AiResponse, String> {
    let svc = ai_service(
        &app, &settings, &log_state, &stats, &mcp_client,
        &state, &symbols_state, &graph_state, &session_store, &diffs,
    );
    let pause = Arc::new(TauriPauseHandler {
        app_handle: app.clone(),
        resume_state: resume_state.0.clone(),
    });
    let result = svc.chat_turn(&session_id, &user_message, Some(pause), false).await?;
    invalidate_undo_cache(&cache);
    Ok(result.response)
}

/// Reset a chat session's model context (P7, D7.4). Clears model_view only —
/// the visible transcript (user_view) is kept; the frontend inserts a
/// "— context reset —" divider. Stats/cost totals are not touched.
#[tauri::command]
pub async fn reset_chat_session(
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    session_id: String,
) -> Result<(), String> {
    let mut store = session_store.0.lock().await;
    if let Some(session) = store.get_mut(&session_id) {
        session.reset_context();
    }
    Ok(())
}

/// Context-utilization snapshot for the chat cost bar (P7, D7.2).
#[tauri::command]
pub async fn get_chat_session_info(
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    session_id: String,
) -> Result<tracelean_core::ChatSessionInfo, String> {
    let (context_window, known) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        match &s.selected_model {
            Some(m) => (m.context_window, m.context_window_known),
            None => (128_000, false),
        }
    };
    println!("Context window {}, known {}", context_window, known);
    let store = session_store.0.lock().await;
    Ok(match store.get(&session_id) {
        Some(session) => {
            println!("known session context {:?}",session.info(context_window, known));
            session.info(context_window, known)
        }
            ,
        None => {
            println!("Print new session");
       
            tracelean_core::ChatSession::new(session_id).info(context_window, known)
        },
    })
}

/// bugs.md Feature 2: force-summarize a session's model context now
/// (the button next to context reset in the cost bar).
#[tauri::command]
pub async fn summarize_chat_session(
    app: AppHandle,
    settings: State<'_, AiSettingsWrapper>,
    log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    diffs: State<'_, crate::PendingDiffsWrapper>,
    session_id: String,
) -> Result<(), String> {
    let svc = ai_service(
        &app, &settings, &log_state, &stats, &mcp_client,
        &state, &symbols_state, &graph_state, &session_store, &diffs,
    );
    svc.summarize_session(&session_id).await
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

/// P11: list persisted + live chat sessions for the switcher (hydrates the
/// in-memory store from .tracelean/sessions/ on first call after startup).
#[tauri::command]
pub async fn list_chat_sessions(
    app: AppHandle,
    settings: State<'_, AiSettingsWrapper>,
    log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    mcp_client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    diffs: State<'_, crate::PendingDiffsWrapper>,
) -> Result<Vec<tracelean_core::agent::ChatSessionSummary>, String> {
    let svc = ai_service(
        &app, &settings, &log_state, &stats, &mcp_client,
        &state, &symbols_state, &graph_state, &session_store, &diffs,
    );
    Ok(svc.list_sessions().await)
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

    /// bugs.md Feature 4: reuses the tool-loop pause UI (Continue/Stop) and
    /// resume channel for per-command approval.
    async fn approve_command(&self, command: &str) -> bool {
        let _ = self.app_handle.emit("tool-loop-pause", serde_json::json!({
            "loops_completed": 0,
            "message": format!("Agent wants to run shell command:\n$ {}\nAllow?", command)
        }));

        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        {
            let mut guard = self.resume_state.lock().await;
            *guard = Some(tx);
        }

        matches!(rx.await, Ok(true))
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

/// Accept every hunk of a pending diff in one call (P10 "accept all").
#[tauri::command]
pub fn accept_all_hunks(
    diffs: State<'_, PendingDiffsWrapper>,
    diff_id: String,
) -> Result<(), String> {
    let mut d = diffs.0.lock().map_err(|e| e.to_string())?;
    let diff = d
        .iter_mut()
        .find(|x| x.id == diff_id)
        .ok_or("Diff not found")?;
    for hunk in diff.hunks.iter_mut() {
        hunk.accepted = true;
    }
    Ok(())
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

    let file = diff.file.clone();
    {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        for cmd in commands {
            s.apply(cmd).map_err(|e| format!("hunk apply failed: {}", e))?;
        }
    }

    // Persist the updated buffer to disk — parity with direct agent edits,
    // which save after every apply.
    {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        let rel = std::path::PathBuf::from(&file);
        if let (Some(root), Some(content)) = (s.project_root().cloned(), s.get_content(&rel)) {
            let full = root.join(&rel);
            if let Some(parent) = full.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&full, content).map_err(|e| format!("disk write failed: {}", e))?;
        }
    }

    d.remove(diff_idx);

    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());
    let _ = app.emit("files-changed", serde_json::json!({ "files": [file] }));

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
