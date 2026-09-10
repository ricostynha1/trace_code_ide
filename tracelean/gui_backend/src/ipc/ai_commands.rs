//! AI IPC commands — chat, settings, models, diff pipeline.

use crate::ai;
use crate::ai::tracking::SessionStats;
use crate::{
    AppStateWrapper, AiSettingsWrapper, AiLogWrapper, AiSessionStatsWrapper,
    MockProviderWrapper, PendingDiffsWrapper, TraceGraphWrapper, UndoTreeCacheWrapper,
    SymbolTableWrapper, AiSettings, McpClientWrapper, invalidate_undo_cache,
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
    use tauri::Manager;
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
        // T12: shared hard-stop token, resolved from managed state so every
        // service instance controls (and is controlled by) the same token.
        cancel: app.state::<crate::AgentCancelWrapper>().0.clone(),
        // Bug 3: shared across every service instance, same as `cancel` above,
        // so a mid-turn write here is visible to a concurrent session_info() read.
        live_context: app.state::<crate::LiveContextWrapper>().0.clone(),
        // Bug 2: shared across every service instance, same pattern as above.
        resolved_diffs: app.state::<crate::ResolvedDiffsWrapper>().0.clone(),
        diff_notify: app.state::<crate::DiffResolvedNotifyWrapper>().0.clone(),
        // Auto-built on project open; shared so find_semantic can query it.
        embed_index: app.state::<crate::EmbedIndexWrapper>().0.clone(),
        // Same pattern as `cancel`/`live_context` above — must be the one
        // Tauri-managed instance, not rebuilt per call, or the session's
        // observed chars-per-token ratio never accumulates.
        calibrated_chars_per_token: app.state::<crate::CacheCalibrationWrapper>().0.clone(),
    }
}

// --- Settings & Models ---

#[tauri::command]
pub fn get_ai_settings(settings: State<'_, AiSettingsWrapper>) -> Result<AiSettings, String> {
    let s = settings.0.lock().map_err(|e| e.to_string())?;
    Ok(s.clone())
}

/// T14: characteristics of the currently selected model — pricing, context
/// window, tool-call dialect, and the cache config the cost model will use
/// (with its source) — so cache-config mispricing is visible in the UI
/// instead of only in bad compaction decisions.
#[tauri::command]
pub fn get_model_characteristics(
    settings: State<'_, AiSettingsWrapper>,
) -> Result<serde_json::Value, String> {
    let s = settings.0.lock().map_err(|e| e.to_string())?;
    let model = s
        .selected_model
        .clone()
        .ok_or("No model selected")?;
    let (cache_cfg, cache_source) =
        tracelean_core::ai::provider_cache::resolve_cache_config(&model);
    Ok(serde_json::json!({
        "model": model,
        "cache_config": cache_cfg,
        "cache_source": cache_source,
        "summary_model": s.summary_model,
    }))
}

/// bugs.md Bug 1: the sandbox dropdown otherwise picks silently between
/// overlay/strace/none — surface which backend this machine actually gets
/// so "Detect" and "Strict" aren't a guessing game.
#[tauri::command]
pub fn get_sandbox_capabilities() -> serde_json::Value {
    use tracelean_core::ai::shell_sandbox::{overlay_available, strace_available};
    let overlay = overlay_available();
    let strace = strace_available();
    let detect_backend = if overlay {
        "overlay"
    } else if strace {
        "strace"
    } else {
        "none"
    };
    serde_json::json!({
        "overlay_available": overlay,
        "strace_available": strace,
        "detect_backend": detect_backend,
    })
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
        pending_diffs: svc.pending_diffs.clone(),
        resolved_diffs: svc.resolved_diffs.clone(),
        diff_notify: svc.diff_notify.clone(),
        cancel: svc.cancel.clone(),
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
        pending_diffs: svc.pending_diffs.clone(),
        resolved_diffs: svc.resolved_diffs.clone(),
        diff_notify: svc.diff_notify.clone(),
        cancel: svc.cancel.clone(),
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
/// Bug 3: prefers the live, mid-turn token counts over the session store's
/// last-turn-end values, so this reflects an in-flight multi-iteration turn
/// instead of only the previous one.
#[tauri::command]
pub async fn get_chat_session_info(
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    settings: State<'_, AiSettingsWrapper>,
    live_context: State<'_, crate::LiveContextWrapper>,
    session_id: String,
) -> Result<tracelean_core::ChatSessionInfo, String> {
    let (context_window, known) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        match &s.selected_model {
            Some(m) => (m.context_window, m.context_window_known),
            None => (128_000, false),
        }
    };
    let live = live_context.0.lock().ok().and_then(|m| m.get(&session_id).copied());
    let store = session_store.0.lock().await;
    let mut info = match store.get(&session_id) {
        Some(session) => session.info(context_window, known),
        None => tracelean_core::ChatSession::new(session_id).info(context_window, known),
    };
    if let Some((prompt, completion)) = live {
        info.estimated_context_tokens = prompt + completion;
    }
    Ok(info)
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
    pub resume_state: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<(bool, bool)>>>>,
    /// bugs.md Bug 0: same pending-diffs list the diff-review UI mutates.
    pub pending_diffs: Arc<std::sync::Mutex<Vec<crate::PendingDiff>>>,
    /// Bug 2: resolved-but-not-yet-collected diff outcomes, written by
    /// `apply_accepted_hunks`/`discard_pending_diff` — `wait_for_review`
    /// waits on `diff_notify` instead of polling this.
    pub resolved_diffs: Arc<std::sync::Mutex<std::collections::HashMap<String, tracelean_core::ResolvedDiffOutcome>>>,
    /// Wakeup signal paired with `resolved_diffs` — no polling.
    pub diff_notify: Arc<tokio::sync::Notify>,
    /// Lets Stop unblock a `wait_for_review` wait immediately.
    pub cancel: tracelean_core::agent::CancelToken,
}

#[async_trait::async_trait]
impl tracelean_core::agent::PauseHandler for TauriPauseHandler {
    async fn should_continue(&self, tool_calls_so_far: usize) -> bool {
        let _ = self.app_handle.emit("tool-loop-pause", serde_json::json!({
            "loops_completed": tool_calls_so_far,
            "message": format!("{} tool calls executed. Continue?", tool_calls_so_far)
        }));

        let (tx, rx) = tokio::sync::oneshot::channel::<(bool, bool)>();
        {
            let mut guard = self.resume_state.lock().await;
            *guard = Some(tx);
        }

        match rx.await {
            Ok((cont, _)) => cont,
            _ => false,
        }
    }

    /// bugs.md Feature 4: reuses the tool-loop pause UI and resume channel for
    /// per-command approval. The payload carries static-screening annotations
    /// (sandboxing_better.md T5) and whether a network checkbox is relevant
    /// (T4); the answer carries the per-command network grant.
    async fn approve_command(
        &self,
        command: &str,
    ) -> tracelean_core::agent::CommandApproval {
        use tauri::Manager;
        let annotations = tracelean_core::ai::shell_sandbox::screen_command(command);
        let settings_network = {
            // Best-effort read of the network policy for the prompt UI.
            self.app_handle
                .try_state::<AiSettingsWrapper>()
                .and_then(|s| s.0.lock().ok().map(|s| s.shell_network.clone()))
                .unwrap_or_else(|| "ask".to_string())
        };
        let _ = self.app_handle.emit("tool-loop-pause", serde_json::json!({
            "loops_completed": 0,
            "kind": "shell-approval",
            "command": command,
            "annotations": annotations,
            "network_policy": settings_network,
            "message": format!("Agent wants to run shell command:\n$ {}\nAllow?", command)
        }));

        let (tx, rx) = tokio::sync::oneshot::channel::<(bool, bool)>();
        {
            let mut guard = self.resume_state.lock().await;
            *guard = Some(tx);
        }

        match rx.await {
            Ok((approved, allow_network)) => {
                tracelean_core::agent::CommandApproval { approved, allow_network }
            }
            _ => tracelean_core::agent::CommandApproval { approved: false, allow_network: false },
        }
    }

    /// bugs.md Bug 0 / Bug 2: event-driven — waits on `diff_notify` (fired by
    /// `apply_accepted_hunks`/`discard_pending_diff` the moment the user
    /// resolves a staged diff) instead of polling. `notified()` is created
    /// *before* the resolved-map check below, so a resolution that lands
    /// between the check and the `.await` is never missed.
    async fn wait_for_review(&self, diff_ids: &[String]) -> Option<Vec<tracelean_core::ResolvedDiffOutcome>> {
        let _ = self.app_handle.emit("tool-loop-pause", serde_json::json!({
            "loops_completed": 0,
            "kind": "diff-review",
            "message": "Waiting for you to accept or reject the staged edit(s) before continuing…",
        }));

        loop {
            let notified = self.diff_notify.notified();
            {
                let mut resolved = self.resolved_diffs.lock().unwrap();
                if diff_ids.iter().all(|id| resolved.contains_key(id)) {
                    let outcomes = diff_ids.iter().filter_map(|id| resolved.remove(id)).collect();
                    let _ = self.app_handle.emit("tool-loop-resumed", serde_json::json!({ "kind": "diff-review" }));
                    return Some(outcomes);
                }
            }
            tokio::select! {
                _ = notified => continue,
                _ = self.cancel.cancelled() => return None,
            }
        }
    }
}

// --- Tool Loop Resume ---

/// Called by frontend when user clicks "Continue"/"Allow" or "Stop"/"Deny" on
/// a pause or shell-approval prompt. `allow_network` is the approval prompt's
/// network checkbox (ignored for plain pause prompts).
#[tauri::command]
pub async fn resume_tool_loop(
    resume_state: State<'_, ToolLoopResumeWrapper>,
    should_continue: bool,
    allow_network: Option<bool>,
) -> Result<(), String> {
    let mut guard = resume_state.0.lock().await;
    if let Some(tx) = guard.take() {
        let _ = tx.send((should_continue, allow_network.unwrap_or(false)));
    }
    Ok(())
}

/// T12: hard stop for the running agent turn (the ⏹ button). Unlike the
/// cooperative pause above, this cancels the shared token — checked every
/// loop iteration, raced against the in-flight LLM request, and polled by
/// run_shell's child-process loop — and also answers any pending
/// pause/approval prompt with "stop" so a blocked loop unblocks immediately.
#[tauri::command]
pub async fn stop_agent_run(
    cancel: State<'_, crate::AgentCancelWrapper>,
    resume_state: State<'_, ToolLoopResumeWrapper>,
) -> Result<(), String> {
    cancel.0.cancel();
    let mut guard = resume_state.0.lock().await;
    if let Some(tx) = guard.take() {
        let _ = tx.send((false, false));
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
    resolved_diffs: State<'_, crate::ResolvedDiffsWrapper>,
    diff_notify: State<'_, crate::DiffResolvedNotifyWrapper>,
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

    // Bug 2: hand the real outcome to whichever `wait_for_review` is
    // blocking on this diff id, before removing it — event-driven, no polling.
    let outcome = tracelean_core::ResolvedDiffOutcome {
        diff_id: diff.id.clone(),
        file: file.clone(),
        total_hunks: diff.hunks.len(),
        accepted_hunks: accepted_count,
        applied_message: diff.applied_message.clone(),
        first_line: diff.hunks.first().map(|h| h.original_start + 1),
    };
    d.remove(diff_idx);
    if let Ok(mut resolved) = resolved_diffs.0.lock() {
        resolved.insert(diff_id, outcome);
    }
    diff_notify.0.notify_waiters();

    invalidate_undo_cache(&cache);
    let _ = app.emit("undo-tree-changed", ());
    let _ = app.emit("files-changed", serde_json::json!({ "files": [file] }));

    Ok(format!("Applied {} accepted hunks.", accepted_count))
}

#[tauri::command]
pub fn discard_pending_diff(
    diffs: State<'_, PendingDiffsWrapper>,
    resolved_diffs: State<'_, crate::ResolvedDiffsWrapper>,
    diff_notify: State<'_, crate::DiffResolvedNotifyWrapper>,
    diff_id: String,
) -> Result<(), String> {
    let mut d = diffs.0.lock().map_err(|e| e.to_string())?;
    let mut removed: Vec<ai::diff_pipeline::PendingDiff> = Vec::new();
    d.retain(|x| {
        if x.id == diff_id {
            removed.push(x.clone());
            false
        } else {
            true
        }
    });
    if let Ok(mut resolved) = resolved_diffs.0.lock() {
        for diff in removed {
            resolved.insert(
                diff.id.clone(),
                tracelean_core::ResolvedDiffOutcome {
                    diff_id: diff.id.clone(),
                    file: diff.file.clone(),
                    total_hunks: diff.hunks.len(),
                    accepted_hunks: 0,
                    applied_message: diff.applied_message.clone(),
                    first_line: diff.hunks.first().map(|h| h.original_start + 1),
                },
            );
        }
    }
    diff_notify.0.notify_waiters();
    Ok(())
}
