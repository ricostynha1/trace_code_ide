//! MCP host/client, agent permissions, streaming, and tool definition IPC commands.

use crate::ai;
use crate::ai::streaming::StreamSession;
use crate::{
    AppStateWrapper, AiSettingsWrapper, AiLogWrapper, AiSessionStatsWrapper,
    MockProviderWrapper, SymbolTableWrapper, TraceGraphWrapper,
    McpHostPermissionsWrapper, McpClientWrapper, AgentPermissionsStore,
    UndoTreeCacheWrapper, ToolLoopResumeWrapper, invalidate_undo_cache,
};
use tauri::{AppHandle, Emitter, State};

// --- MCP Host (4.15) ---

#[tauri::command]
pub fn mcp_list_tools() -> Vec<ai::ToolDefinition> {
    ai::tools::builtin_tool_definitions()
}

#[tauri::command]
pub fn mcp_call_tool(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    mcp_perms: State<'_, McpHostPermissionsWrapper>,
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<ai::ToolResult, String> {
    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    let perms = mcp_perms.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().unwrap_or_default();

    let call = ai::ToolCall { name: tool_name, arguments };
    Ok(ai::tool_executor::execute_tool(&call, &root, &mut s, &sym, &g, &perms))
}

#[tauri::command]
pub fn mcp_handle_jsonrpc(
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    mcp_perms: State<'_, McpHostPermissionsWrapper>,
    request_json: String,
) -> Result<String, String> {
    let request: ai::mcp_host::JsonRpcRequest = serde_json::from_str(&request_json)
        .map_err(|e| format!("Invalid JSON-RPC: {}", e))?;

    let mut s = state.0.lock().map_err(|e| e.to_string())?;
    let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
    let g = graph_state.0.lock().map_err(|e| e.to_string())?;
    let perms = mcp_perms.0.lock().map_err(|e| e.to_string())?;
    let root = s.project_root().cloned().unwrap_or_default();

    let response = ai::mcp_host::handle_request(&request, &root, &mut s, &sym, &g, &perms);
    serde_json::to_string(&response).map_err(|e| e.to_string())
}

// --- MCP Client (4.16) ---

#[tauri::command]
pub async fn mcp_client_connect(
    state: State<'_, AppStateWrapper>,
    client: State<'_, McpClientWrapper>,
) -> Result<Vec<String>, String> {
    let root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().ok_or("No project open")?
    };
    let config = ai::mcp_client::McpConfig::load(&root);
    let mut mgr = client.0.lock().await;
    let errors = mgr.connect_all(&config).await;
    if errors.is_empty() {
        Ok(mgr.connected_servers())
    } else {
        Ok(errors)
    }
}

#[tauri::command]
pub async fn mcp_client_list_tools(
    client: State<'_, McpClientWrapper>,
) -> Result<Vec<(String, ai::ToolDefinition)>, String> {
    let mgr = client.0.lock().await;
    Ok(mgr.all_tools())
}

#[tauri::command]
pub async fn mcp_client_call_tool(
    client: State<'_, McpClientWrapper>,
    server_name: String,
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<ai::ToolResult, String> {
    let mut mgr = client.0.lock().await;
    mgr.call_tool(&server_name, &tool_name, arguments).await
}

#[tauri::command]
pub async fn mcp_client_disconnect(
    client: State<'_, McpClientWrapper>,
) -> Result<(), String> {
    let mut mgr = client.0.lock().await;
    mgr.disconnect_all().await;
    Ok(())
}

#[tauri::command]
pub async fn mcp_client_status(
    client: State<'_, McpClientWrapper>,
) -> Result<Vec<String>, String> {
    let mgr = client.0.lock().await;
    Ok(mgr.connected_servers())
}

// --- Agent Permission Model (4.17) ---

#[tauri::command]
pub fn get_agent_permissions(
    store: State<'_, AgentPermissionsStore>,
    agent_id: String,
) -> Result<ai::AgentPermissions, String> {
    let s = store.0.lock().map_err(|e| e.to_string())?;
    Ok(s.get(&agent_id).cloned().unwrap_or_else(|| ai::AgentPermissions::full_access(&agent_id)))
}

#[tauri::command]
pub fn set_agent_permissions(
    store: State<'_, AgentPermissionsStore>,
    permissions: ai::AgentPermissions,
) -> Result<(), String> {
    let mut s = store.0.lock().map_err(|e| e.to_string())?;
    s.insert(permissions.agent_id.clone(), permissions);
    Ok(())
}

#[tauri::command]
pub fn list_agent_permissions(
    store: State<'_, AgentPermissionsStore>,
) -> Result<Vec<ai::AgentPermissions>, String> {
    let s = store.0.lock().map_err(|e| e.to_string())?;
    Ok(s.values().cloned().collect())
}

// --- Streaming (4.18) ---

#[tauri::command]
pub async fn ai_chat_stream(
    app: AppHandle,
    settings: State<'_, AiSettingsWrapper>,
    log_state: State<'_, AiLogWrapper>,
    stats: State<'_, AiSessionStatsWrapper>,
    _mock_provider: State<'_, MockProviderWrapper>,
    client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    resume_state: State<'_, ToolLoopResumeWrapper>,
    diffs: State<'_, crate::PendingDiffsWrapper>,
    session_store: State<'_, crate::ChatSessionStoreWrapper>,
    session_id: String,
    user_message: String,
) -> Result<ai::AiResponse, String> {
    use tracelean_core::agent::{AgentContext, run_agent_turn_session, ChatSession};
    use tracelean_core::ai::provider::{ChatMessage, MessageRole};

    let stream_id = uuid::Uuid::new_v4().to_string();
    let mut session = StreamSession::new(stream_id.clone());
    let _ = app.emit("ai-chat-stream", serde_json::json!({ "stream_id": stream_id }));

    // Snapshot project root
    let project_root = {
        let s = state.0.lock().map_err(|e| e.to_string())?;
        s.project_root().cloned().unwrap_or_default()
    };

    // Collect MCP tool definitions
    let extra_tools: Vec<ai::tools::ToolDefinition> = {
        let mgr = client.0.lock().await;
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
        permissions: {
            let mut p = tracelean_core::AgentPermissions::full_access("chat");
            p.review_edits = settings.0.lock().map(|s| s.review_edits).unwrap_or(false);
            p
        },
        project_root: project_root.clone(),
        event_sink: std::sync::Arc::new(crate::TauriEventSink { app_handle: app.clone() }),
        spend_cap_usd: spend_cap,
        pause_handler: Some(std::sync::Arc::new(crate::ipc::ai_commands::TauriPauseHandler {
            app_handle: app.clone(),
            resume_state: resume_state.0.clone(),
        })),
        verbose: false,
        retention_engine: std::sync::Arc::new(std::sync::Mutex::new(tracelean_core::ai::RetentionEngine::with_defaults())),
        timing_tracker: std::sync::Arc::new(std::sync::Mutex::new(tracelean_core::ai::TurnTimingTracker::new())),
        pending_diffs: diffs.0.clone(),
    };

    // Session-based (bugs.md Bug 1): the store owns the conversation, so the
    // context bar and compaction state survive across streamed turns too.
    let mut store = session_store.0.lock().await;
    let chat_session = store
        .entry(session_id.clone())
        .or_insert_with(|| ChatSession::new(session_id.clone()));
    chat_session.append(ChatMessage {
        role: MessageRole::User,
        content: user_message,
        tool_call_id: None,
        tool_calls: Vec::new(),
    });

    // Delegate to the single core implementation (tool loop, compaction, etc.)
    let cost_before = stats.0.lock().map(|s| s.total_cost_usd).unwrap_or(0.0);
    let result = run_agent_turn_session(&ctx, chat_session).await;

    // P11: persist the session every turn (best-effort).
    if result.is_ok() {
        let cost_after = stats.0.lock().map(|s| s.total_cost_usd).unwrap_or(cost_before);
        chat_session.total_cost_usd += (cost_after - cost_before).max(0.0);
        chat_session.updated_at = chrono::Utc::now().to_rfc3339();
        if !project_root.as_os_str().is_empty() {
            tracelean_core::ai::service::save_session(&project_root, chat_session);
        }
    }
    match result {
        Ok(turn_result) => {
            // Simulate streaming: emit response content in chunks
            let content = &turn_result.response.content;
            if !content.is_empty() {
                let chunk_size = 50;
                for chunk in content.as_bytes().chunks(chunk_size) {
                    let text = String::from_utf8_lossy(chunk).to_string();
                    let token_event = session.push_token(&text);
                    let _ = app.emit("ai-stream-token", &token_event);
                    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                }
            }
            let _ = app.emit("ai-stream-token", session.finish());

            // Invalidate undo cache after tool execution
            invalidate_undo_cache(&cache);

            let _ = app.emit("ai-chat-stream", session.finish());
            Ok(turn_result.response)
        }
        Err(e) => {
            let _ = app.emit("ai-stream-token", session.finish());
            let _ = app.emit("ai-chat-stream", session.finish());      
            Err(e.to_string())
        }
    }
}

// --- Tool Definitions ---

#[tauri::command]
pub fn get_agent_tools() -> Vec<ai::ToolDefinition> {
    ai::tools::builtin_tool_definitions()
}

#[tauri::command]
pub async fn get_agent_tools_prompt(
    client: State<'_, McpClientWrapper>,
) -> Result<String, String> {
    let mut tools = ai::tools::builtin_tool_definitions();
    let mgr = client.0.lock().await;
    let mcp_tools: Vec<ai::ToolDefinition> = mgr.all_tools().into_iter().map(|(_s, t)| t).collect();
    tools.extend(mcp_tools);
    Ok(ai::tools::tools_as_system_prompt(&tools))
}
