//! MCP host/client, agent permissions, streaming, and tool definition IPC commands.

use crate::ai;
use crate::ai::streaming::StreamSession;
use crate::{
    AppStateWrapper, AiSettingsWrapper, AiLogWrapper, AiSessionStatsWrapper,
    MockProviderWrapper, SymbolTableWrapper, TraceGraphWrapper,
    McpHostPermissionsWrapper, McpClientWrapper, AgentPermissionsStore,
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
    mock_provider: State<'_, MockProviderWrapper>,
    client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    messages: Vec<ai::provider::ChatMessage>,
) -> Result<ai::AiResponse, String> {
    use ai::provider::{AiProvider, MessageRole};

    let stream_id = uuid::Uuid::new_v4().to_string();
    let mut session = StreamSession::new(stream_id.clone());

    let _ = app.emit("ai-stream-start", serde_json::json!({ "stream_id": stream_id }));

    // Inject system prompt with tool definitions if none present
    let mut messages = {
        let has_system = messages.iter().any(|m| m.role == MessageRole::System);
        if has_system {
            messages
        } else {
            let mut tools = ai::tools::builtin_tool_definitions();
            let mgr = client.0.lock().await;
            let mcp_tools: Vec<ai::ToolDefinition> = mgr.all_tools().into_iter().map(|(_s, t)| t).collect();
            tools.extend(mcp_tools);
            let system_content = format!(
                "You are an AI assistant inside the TraceLean IDE. You help with requirements engineering, \
                 formal specification, implementation, and repair.\n\n{}",
                ai::tools::tools_as_system_prompt(&tools)
            );
            let mut msgs = vec![ai::provider::ChatMessage {
                role: MessageRole::System,
                content: system_content,
            }];
            msgs.extend(messages);
            msgs
        }
    };

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

    let max_tool_loops = 10;

    for _loop_i in 0..max_tool_loops {
        let request = ai::AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            stop: None,
        };

        let start = std::time::Instant::now();

        let result: Result<ai::AiResponse, ai::provider::AiError> = match provider_kind {
            ai::ProviderKind::OpenRouter => {
                let key = or_key.clone().ok_or("OpenRouter API key not set").map_err(|e| {
                    let _ = app.emit("ai-stream-token", session.finish());
                    e.to_string()
                })?;
                let provider = ai::openrouter::OpenRouterProvider::new(key);
                let resp = provider.complete(&request).await;
                if let Ok(ref response) = resp {
                    let content = &response.content;
                    let chunk_size = 50;
                    for chunk in content.as_bytes().chunks(chunk_size) {
                        let text = String::from_utf8_lossy(chunk).to_string();
                        let token_event = session.push_token(&text);
                        let _ = app.emit("ai-stream-token", &token_event);
                        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                    }
                }
                resp
            }
            ai::ProviderKind::Bedrock => {
                let access = br_access.clone().ok_or("Bedrock access key not set").map_err(|e| e.to_string())?;
                let secret = br_secret.clone().ok_or("Bedrock secret key not set").map_err(|e| e.to_string())?;
                let region = br_region.clone().unwrap_or_else(|| "us-east-1".into());
                let provider = ai::bedrock::BedrockProvider::new(access, secret, region);
                let resp = provider.complete(&request).await;
                if let Ok(ref response) = resp {
                    let content = &response.content;
                    let chunk_size = 50;
                    for chunk in content.as_bytes().chunks(chunk_size) {
                        let text = String::from_utf8_lossy(chunk).to_string();
                        let token_event = session.push_token(&text);
                        let _ = app.emit("ai-stream-token", &token_event);
                        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
                    }
                }
                resp
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
                let resp = provider.complete(&request).await;
                if let Ok(ref response) = resp {
                    let token_event = session.push_token(&response.content);
                    let _ = app.emit("ai-stream-token", &token_event);
                }
                resp
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
                    log.record_success("chat_stream", &request, &response, duration_ms);
                }
                {
                    let mut s = stats.0.lock().map_err(|e| e.to_string())?;
                    s.record(&response.usage, &cost);
                }

                // Check for tool_call blocks in the response
                let tool_calls = extract_tool_calls_from_response(&response.content);
                if tool_calls.is_empty() {
                    let _ = app.emit("ai-stream-token", session.finish());
                    return Ok(response);
                }

                // Execute tool calls and loop
                messages.push(ai::provider::ChatMessage {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                });

                let mut tool_results = String::new();
                for tc in &tool_calls {
                    let mcp_call = tc.to_mcp_call();
                    let result = {
                        let mut s = state.0.lock().map_err(|e| e.to_string())?;
                        let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
                        let g = graph_state.0.lock().map_err(|e| e.to_string())?;
                        let root = s.project_root().cloned().unwrap_or_default();
                        let perms = ai::tool_executor::AgentPermissions::full_access("chat");
                        ai::tool_executor::execute_tool(&mcp_call, &root, &mut s, &sym, &g, &perms)
                    };
                    tool_results.push_str(&format!(
                        "Tool `{}` result (success={}):\n{}\n\n",
                        mcp_call.name, result.success, result.content
                    ));
                    // Stream tool result to UI
                    let tool_msg = format!("\n\n[Tool: {} → {}]\n", mcp_call.name, if result.success { "ok" } else { "error" });
                    let token_event = session.push_token(&tool_msg);
                    let _ = app.emit("ai-stream-token", &token_event);
                }

                messages.push(ai::provider::ChatMessage {
                    role: MessageRole::User,
                    content: format!("Tool execution results:\n\n{}", tool_results),
                });
                // Continue loop — will call AI again with results
            }
            Err(e) => {
                let _ = app.emit("ai-stream-token", session.finish());
                let mut log = log_state.0.lock().map_err(|e2| e2.to_string())?;
                log.record_failure("chat_stream", &request, &e, duration_ms);
                return Err(e.message);
            }
        }
    }

    let _ = app.emit("ai-stream-token", session.finish());
    Err("Max tool call loops exceeded (10).".into())
}

/// Parse tool_call code blocks from an AI response.
/// Only recognizes properly fenced ```tool_call blocks.
fn extract_tool_calls_from_response(content: &str) -> Vec<ai::tools::AgentToolCall> {
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
