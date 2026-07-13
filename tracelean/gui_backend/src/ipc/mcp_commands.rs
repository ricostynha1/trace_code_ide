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
    mock_provider: State<'_, MockProviderWrapper>,
    client: State<'_, McpClientWrapper>,
    state: State<'_, AppStateWrapper>,
    symbols_state: State<'_, SymbolTableWrapper>,
    graph_state: State<'_, TraceGraphWrapper>,
    cache: State<'_, UndoTreeCacheWrapper>,
    resume_state: State<'_, ToolLoopResumeWrapper>,
    messages: Vec<ai::provider::ChatMessage>,
) -> Result<ai::AiResponse, String> {
    use ai::provider::{AiProvider, MessageRole};

    let stream_id = uuid::Uuid::new_v4().to_string();
    let mut session = StreamSession::new(stream_id.clone());

    let _ = app.emit("ai-stream-start", serde_json::json!({ "stream_id": stream_id }));

    // Build tool schemas from builtin + MCP client tools
    let all_tool_schemas = {
        let mut schemas = ai::tools::builtin_tool_schemas();
        let mgr = client.0.lock().await;
        for (_server, tool) in mgr.all_tools() {
            schemas.push(tool.to_tool_schema());
        }
        schemas
    };

    // Build tool index for selection
    let tool_index = ai::ToolIndex::new(&all_tool_schemas);

    // Extract user query for tool selection (sliding window of last 3 turns)
    let user_query = {
        let mut window_parts: Vec<String> = Vec::new();
        let mut turn_count = 0;
        for m in messages.iter().rev() {
            if m.role == MessageRole::Tool { continue; }
            let label = match m.role {
                MessageRole::User => "User",
                MessageRole::Assistant => "Agent",
                _ => continue,
            };
            window_parts.push(format!("{}: {}", label, m.content));
            if m.role == MessageRole::User {
                turn_count += 1;
                if turn_count >= 3 { break; }
            }
        }
        window_parts.reverse();
        window_parts.join("\n")
    };

    let mut messages = {
        let system_prompt = "You are TraceLean, an AI coding assistant embedded in an IDE.\n\
            You help users understand, modify, and verify code using the provided tools.\n\n\
            CRITICAL — PARALLEL TOOL CALLS:\n\
            When you need multiple independent operations, call ALL tools in ONE response.\n\n\
            GOOD example — user asks to add a comment to 3 files:\n\
            Response: [tool_call: write_range(A.md), tool_call: write_range(B.md), tool_call: write_range(C.md)]\n\n\
            BAD example — same task but one call per turn:\n\
            Turn 1: [tool_call: write_range(A.md)]  ← wastes 2 extra LLM round-trips\n\
            Turn 2: [tool_call: write_range(B.md)]\n\
            Turn 3: [tool_call: write_range(C.md)]\n\n\
            Same applies to reads: if you need to read 3 files, read all 3 in one response.";
        let mut full = vec![ai::provider::ChatMessage {
            role: MessageRole::System,
            content: system_prompt.to_string(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }];
        full.extend(messages);
        full
    };

    let (model, provider_kind, or_key, br_token, br_region) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        let model = s.selected_model.clone().ok_or("No model selected")?;
        (
            model,
            s.active_provider.clone(),
            s.openrouter_api_key.clone(),
            s.bedrock_api_key.clone(),
            s.bedrock_region.clone(),
        )
    };

    let tool_loop_pause_threshold = 10;
    let mut consecutive_failures: u32 = 0;
    let mut previously_called: Vec<String> = Vec::new();
    let mut loop_i: usize = 0;
    let mut total_tool_calls: usize = 0;

    // T3.8: Read spend cap
    let spend_cap = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        s.spend_cap_usd
    };

    loop {
        // T3.8: Enforce spend cap
        {
            let s = stats.0.lock().map_err(|e| e.to_string())?;
            if s.total_cost_usd >= spend_cap {
                let _ = app.emit("ai-stream-token", session.finish());
                return Err(format!("Spend cap reached (${:.4} >= ${:.2}). Increase cap in settings to continue.", s.total_cost_usd, spend_cap));
            }
        }

        // Select relevant tools for this turn
        let selected_tools = tool_index.select_with_context(&user_query, &previously_called, None);

        let request = ai::AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            stop: None,
            tools: if selected_tools.is_empty() { None } else { Some(selected_tools) },
        };

        let start = std::time::Instant::now();

        let result: Result<ai::AiResponse, ai::provider::AiError> = match provider_kind {
            ai::ProviderKind::OpenRouter => {
                let key = or_key.clone()
                    .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                    .ok_or("OpenRouter API key not set").map_err(|e| {
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
                let token = br_token.clone()
                    .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok())
                    .ok_or("Bedrock bearer token not set").map_err(|e| e.to_string())?;
                let provider = ai::bedrock::BedrockProvider::new(token, br_region.clone());
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
                    let label = if loop_i == 0 { "chat_stream".to_string() } else { format!("chat_stream/tool_{}", loop_i) };
                    log.record_success(&label, &request, &response, duration_ms);
                }
                {
                    let mut s = stats.0.lock().map_err(|e| e.to_string())?;
                    s.record(&response.usage, &cost);
                }
                // Notify frontend about updated stats
                let _ = app.emit("ai-stats-updated", ());

                // Check for tool calls in response (native tool calling)
                if response.tool_calls.is_empty() {
                    let _ = app.emit("ai-stream-token", session.finish());
                    return Ok(response);
                }

                // Bug 2 fix: Emit assistant text content to chat UI between tool loops
                if !response.content.is_empty() {
                    let _ = app.emit("ai-chat-message", serde_json::json!({
                        "role": "assistant",
                        "content": response.content.clone()
                    }));
                }

                // Reset stream session so next iteration starts fresh
                session.accumulated.clear();
                let _ = app.emit("ai-stream-token", serde_json::json!({
                    "stream_id": session.id,
                    "token": "",
                    "done": false,
                    "accumulated": ""
                }));

                // Check if any tool call has malformed JSON arguments
                let has_malformed = response.tool_calls.iter().any(|tc| {
                    serde_json::from_str::<serde_json::Value>(&tc.function.arguments).is_err()
                });

                if has_malformed {
                    let error_details: Vec<String> = response.tool_calls.iter()
                        .filter(|tc| serde_json::from_str::<serde_json::Value>(&tc.function.arguments).is_err())
                        .map(|tc| format!("{}({}) — invalid JSON", tc.function.name, tc.function.arguments))
                        .collect();

                    let error_msg = format!(
                        "Error: your tool call(s) contained invalid JSON arguments and could not be executed:\n{}\nPlease retry with valid JSON.",
                        error_details.join("\n")
                    );

                    let _ = app.emit("ai-chat-message", serde_json::json!({
                        "role": "system",
                        "content": error_msg.clone()
                    }));

                    if !response.content.is_empty() {
                        messages.push(ai::provider::ChatMessage {
                            role: MessageRole::Assistant,
                            content: response.content.clone(),
                            tool_call_id: None,
                            tool_calls: Vec::new(),
                        });
                    }
                    messages.push(ai::provider::ChatMessage {
                        role: MessageRole::User,
                        content: error_msg,
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    });

                    let tool_msg = format!("\n\n[JSON parse error in tool calls — retrying]\n");
                    let token_event = session.push_token(&tool_msg);
                    let _ = app.emit("ai-stream-token", &token_event);

                    consecutive_failures += 1;
                    if consecutive_failures >= 6 {
                        let _ = app.emit("ai-stream-token", session.finish());
                        return Err("Too many consecutive tool call failures (invalid JSON). Stopping.".into());
                    }
                    loop_i += 1;
                    continue;
                }

                // All tool calls have valid JSON — push assistant message with tool_calls
                messages.push(ai::provider::ChatMessage {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                    tool_call_id: None,
                    tool_calls: response.tool_calls.clone(),
                });

                for tc in &response.tool_calls {
                    let tool_name = &tc.function.name;

                    // Pause every N tool calls for user confirmation
                    total_tool_calls += 1;
                    if total_tool_calls > 0 && total_tool_calls % tool_loop_pause_threshold == 0 {
                        let _ = app.emit("tool-loop-pause", serde_json::json!({
                            "loops_completed": total_tool_calls,
                            "message": format!("{} tool calls executed. Continue?", total_tool_calls)
                        }));

                        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
                        {
                            let mut guard = resume_state.0.lock().await;
                            *guard = Some(tx);
                        }

                        match rx.await {
                            Ok(true) => { /* user said continue */ }
                            _ => {
                                let _ = app.emit("ai-chat-message", serde_json::json!({
                                    "role": "system",
                                    "content": format!("[Stopped by user after {} tool calls.]", total_tool_calls)
                                }));
                                let _ = app.emit("ai-stream-token", session.finish());
                                return Err(format!("Tool loop stopped by user after {} tool calls.", total_tool_calls));
                            }
                        }
                    }

                    // Bug 1 fix: detect malformed JSON args and report to user
                    let arguments: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
                        Ok(v) => v,
                        Err(parse_err) => {
                            let error_msg = format!(
                                "tool_call_failed: model produced invalid JSON for {}({}): {}",
                                tool_name, tc.function.arguments, parse_err
                            );
                            let _ = app.emit("ai-chat-message", serde_json::json!({
                                "role": "system",
                                "content": error_msg.clone()
                            }));
                            // Push error as tool result so model can self-correct
                            messages.push(ai::provider::ChatMessage {
                                role: MessageRole::Tool,
                                content: format!("Error: invalid JSON arguments — {}", parse_err),
                                tool_call_id: Some(tc.id.clone()),
                                tool_calls: Vec::new(),
                            });
                            consecutive_failures += 1;
                            continue;
                        }
                    };

                    previously_called.push(tool_name.clone());

                    // Emit tool call message to frontend chat
                    let _ = app.emit("ai-chat-message", serde_json::json!({
                        "role": "system",
                        "content": format!("call {}", tool_name)
                    }));

                    let mcp_call = ai::ToolCall { name: tool_name.clone(), arguments };
                    let result = {
                        let mut s = state.0.lock().map_err(|e| e.to_string())?;
                        let sym = symbols_state.0.lock().map_err(|e| e.to_string())?;
                        let g = graph_state.0.lock().map_err(|e| e.to_string())?;
                        let root = s.project_root().cloned().unwrap_or_default();
                        let perms = ai::tool_executor::AgentPermissions::full_access("chat");
                        ai::tool_executor::execute_tool(&mcp_call, &root, &mut s, &sym, &g, &perms)
                    };

                    // Emit tool response message to frontend chat
                    let _ = app.emit("ai-chat-message", serde_json::json!({
                        "role": "system",
                        "content": format!("rsp {}", tool_name)
                    }));

                    // Push tool result as proper Tool role message
                    messages.push(ai::provider::ChatMessage {
                        role: MessageRole::Tool,
                        content: result.content.clone(),
                        tool_call_id: Some(tc.id.clone()),
                        tool_calls: Vec::new(),
                    });

                    // Track consecutive failures
                    if result.success {
                        consecutive_failures = 0;
                    } else {
                        consecutive_failures += 1;
                    }
                    if consecutive_failures >= 6 {
                        break;
                    }
                }

                // Stop if too many consecutive failures
                if consecutive_failures >= 6 {
                    let failure_msg = format!(
                        "{}\n\n[Tool execution stopped: {} consecutive failures. Please provide additional guidance.]",
                        response.content, consecutive_failures
                    );
                    let _ = app.emit("ai-stream-token", session.finish());
                    return Ok(ai::AiResponse {
                        content: failure_msg,
                        usage: response.usage.clone(),
                        raw_response: response.raw_response.clone(),
                        truncated: response.truncated,
                        tool_calls: Vec::new(),
                    });
                }

                // Invalidate undo cache and notify frontend after tool execution
                invalidate_undo_cache(&cache);
                let _ = app.emit("undo-tree-changed", ());
                let _ = app.emit("files-changed", ());

                // Continue loop — will call AI again with results
            }
            Err(e) => {
                let _ = app.emit("ai-stream-token", session.finish());
                let mut log = log_state.0.lock().map_err(|e2| e2.to_string())?;
                let label = if loop_i == 0 { "chat_stream".to_string() } else { format!("chat_stream/tool_{}", loop_i) };
                log.record_failure(&label, &request, &e, duration_ms);
                return Err(e.message);
            }
        }

        loop_i += 1;
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
