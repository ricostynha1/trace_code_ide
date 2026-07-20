//! Built-in ACP agent — runs as subprocess, communicates via stdio.
//!
//! Implements the Agent side of ACP with:
//! - Turn-based LLM loop (prompt → LLM → tools → LLM → ... → final response)
//! - Session/update notifications (agent_message_chunk, tool_call, tool_call_update)
//! - Permission requests for write operations
//! - Context compaction when conversation grows too large
//! - Steering channel for abort/redirect

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, CancelNotification, ContentBlock, ContentChunk, InitializeRequest,
    InitializeResponse, NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse,
    SessionId, SessionNotification, SessionUpdate, StopReason, TextContent,
    ToolCall as AcpToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use agent_client_protocol::{
    on_receive_notification, on_receive_request, Agent, Client, ConnectionTo, Responder, Stdio,
};
use tokio::sync::{watch, Mutex};

use crate::ai::provider::{ChatMessage, MessageRole};

// ---------------------------------------------------------------------------
// Steering
// ---------------------------------------------------------------------------

/// Steering commands sent from client to the running agent loop.
#[derive(Debug, Clone)]
pub enum SteeringCommand {
    /// Continue normally.
    None,
    /// Abort current work, return partial result.
    Abort,
    /// Replace current prompt context and restart the turn.
    Redirect(String),
}

// ---------------------------------------------------------------------------
// Context compaction
// ---------------------------------------------------------------------------

/// Max total chars before compaction triggers (~100k chars ≈ 25k tokens).
const COMPACTION_THRESHOLD: usize = 100_000;
/// How many recent user messages to keep intact.
const KEEP_RECENT_USER_MSGS: usize = 3;
/// Max chars for tool results after compaction.
const TRUNCATED_TOOL_RESULT_LEN: usize = 200;

/// Conversation message with role tracking for compaction.
#[derive(Debug, Clone)]
struct ConversationEntry {
    role: MessageRole,
    content: String,
    /// If true, this is a tool result that can be truncated.
    is_tool_result: bool,
}

/// Cost-aware context compaction.
///
/// Uses the cost model to decide what to prune based on cache economics.
/// Falls back to simple truncation if cost model shows Keep for everything.
fn compact_context(system_prompt: &str, entries: &mut Vec<ConversationEntry>) {
    // bugs.md Bug 2: mirrors the AiSettings.disable_context_trimming toggle
    // in the GUI path (agent/runtime.rs::cost_aware_compact). This ACP
    // subprocess doesn't have the GUI's settings struct in scope, so the
    // launcher threads the same on/off switch through an env var, matching
    // how this function already reads its pricing from MODEL_INPUT_COST_PER_M
    // and friends below.
    if std::env::var("TRACELEAN_DISABLE_CONTEXT_TRIMMING").as_deref() == Ok("1") {
        return;
    }

    let total_chars: usize = system_prompt.len() + entries.iter().map(|e| e.content.len()).sum::<usize>();
    if total_chars < COMPACTION_THRESHOLD {
        return;
    }

    use crate::ai::{
        retention::{RetentionEntry, EntryKind, RetentionAction},
        cost_trimmed_summary_model::{batch_prune_decisions, PruneContext},
        provider_cache::{CacheMode, ProviderCacheConfig},
    };

    // Build a default provider cache config (assume automatic, no explicit cache)
    let cache_cfg = ProviderCacheConfig {
        cache_mode: CacheMode::Automatic,
        cache_read_discount: 0.5,
        cache_write_multiplier: 0.0,
        ttl_seconds: Some(300),
        requires_markers: false,
        notes: None,
    };

    // Read pricing from env (summary model may be cheaper)
    let main_input_per_m: f64 = std::env::var("MODEL_INPUT_COST_PER_M")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let summary_input_per_m: f64 = std::env::var("SUMMARY_MODEL_INPUT_COST_PER_M")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(main_input_per_m * 0.5);
    let summary_output_per_m: f64 = std::env::var("SUMMARY_MODEL_OUTPUT_COST_PER_M")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(main_input_per_m * 3.0);

    let cost_per_token = main_input_per_m / 1_000_000.0;

    let prune_ctx = PruneContext {
        provider: cache_cfg,
        cost_per_token,
        n_expected: 4,
        time_since_last_request_secs: 0,
        cached_prefix_tokens: 0,
        compression_ratio: 0.25,
        summarizer_input_cost: summary_input_per_m / 1_000_000.0,
        summarizer_output_cost: summary_output_per_m / 1_000_000.0,
    };

    // Find indices of user messages (for "keep last N" logic)
    let user_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.role == MessageRole::User)
        .map(|(i, _)| i)
        .collect();

    let keep_from = if user_indices.len() > KEEP_RECENT_USER_MSGS {
        user_indices[user_indices.len() - KEEP_RECENT_USER_MSGS]
    } else {
        0
    };

    // Build retention entries for the older messages
    let older_entries: Vec<RetentionEntry> = entries[..keep_from]
        .iter()
        .enumerate()
        .map(|(i, e)| RetentionEntry {
            id: i as u64,
            kind: if e.is_tool_result { EntryKind::ToolResult } else { EntryKind::AssistantMsg },
            content: e.content.clone(),
            resources: Vec::new(),
            created_turn: 0,
            last_used_turn: 0,
            approx_tokens: e.content.len() / 4,
            ttl: None,
            invalidation_events: Vec::new(),
            action: RetentionAction::Eligible,
            args_hash: None,
            ephemeral: false,
            offloaded: false,
            offload_path: None,
        })
        .collect();

    let refs: Vec<&RetentionEntry> = older_entries.iter().collect();
    // Every entry here is already Eligible (there's no separate "kept" set
    // in this older/simpler ACP path), so the full-stream list and the
    // eligible list are the same slice.
    let (prune_ids, _logs) = batch_prune_decisions(&refs, &refs, &prune_ctx, 1);

    // Apply prune decisions
    let mut pruned_any = false;
    for i in 0..keep_from {
        if prune_ids.contains(&(i as u64)) {
            let entry = &mut entries[i];
            if entry.is_tool_result {
                if entry.content.len() > TRUNCATED_TOOL_RESULT_LEN {
                    entry.content.truncate(TRUNCATED_TOOL_RESULT_LEN);
                    entry.content.push_str("... [pruned:cost]");
                    pruned_any = true;
                }
            } else if entry.content.len() > 100 {
                let first_line = entry.content.lines().next().unwrap_or("").to_string();
                entry.content = format!("[compacted:cost] {}", &first_line[..first_line.len().min(80)]);
                pruned_any = true;
            }
        }
    }

    // Fallback: if cost model kept everything but we're still over threshold, do simple truncation
    if !pruned_any {
        for i in 0..keep_from {
            let entry = &mut entries[i];
            if entry.is_tool_result && entry.content.len() > TRUNCATED_TOOL_RESULT_LEN {
                entry.content.truncate(TRUNCATED_TOOL_RESULT_LEN);
                entry.content.push_str("... [truncated]");
            } else if !entry.is_tool_result && entry.content.len() > 100 {
                let first_line = entry.content.lines().next().unwrap_or("").to_string();
                entry.content = format!("[compacted] {}", &first_line[..first_line.len().min(80)]);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// Per-session state for the agent.
#[allow(dead_code)]
struct SessionState {
    id: SessionId,
    cwd: PathBuf,
    conversation: Vec<ConversationEntry>,
    tool_call_counter: u64,
}

impl SessionState {
    fn new(id: SessionId, cwd: PathBuf) -> Self {
        Self {
            id,
            cwd,
            conversation: Vec::new(),
            tool_call_counter: 0,
        }
    }

    fn next_tool_call_id(&mut self) -> String {
        self.tool_call_counter += 1;
        format!("tc_{}", self.tool_call_counter)
    }
}

/// Shared agent state across handlers.
struct AgentState {
    sessions: HashMap<String, SessionState>,
    steering_rx: watch::Receiver<SteeringCommand>,
}

type SharedState = Arc<Mutex<AgentState>>;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the built-in agent. Call from a subprocess main().
///
/// `steering_rx` allows the parent process to send abort/redirect commands.
pub async fn run_builtin_agent(
    steering_rx: watch::Receiver<SteeringCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
    let state: SharedState = Arc::new(Mutex::new(AgentState {
        sessions: HashMap::new(),
        steering_rx,
    }));

    let state_init = Arc::clone(&state);
    let state_new = Arc::clone(&state);
    let state_prompt = Arc::clone(&state);
    let state_cancel = Arc::clone(&state);

    Agent
        .builder()
        .name("tracelean-agent")
        .on_receive_request(
            {
                let _state = state_init;
                async move |req: InitializeRequest, responder: Responder<InitializeResponse>, _cx: ConnectionTo<Client>| {
                    let resp = InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new());
                    responder.respond(resp)
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state_new;
                async move |req: NewSessionRequest, responder: Responder<NewSessionResponse>, _cx: ConnectionTo<Client>| {
                    let st = state.clone();
                    let session_id = SessionId::new(uuid::Uuid::new_v4().to_string());
                    let session = SessionState::new(session_id.clone(), req.cwd);

                    {
                        let mut guard = st.lock().await;
                        guard.sessions.insert(session_id.0.to_string(), session);
                    }

                    responder.respond(NewSessionResponse::new(session_id))
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state_prompt;
                async move |req: PromptRequest, responder: Responder<PromptResponse>, cx: ConnectionTo<Client>| {
                    let st = state.clone();
                    let stop_reason = handle_prompt_loop(st, req, &cx).await;
                    responder.respond(PromptResponse::new(stop_reason))
                }
            },
            on_receive_request!(),
        )
        .on_receive_notification(
            {
                let _state = state_cancel;
                async move |_notif: CancelNotification, _cx: ConnectionTo<Client>| -> Result<(), agent_client_protocol::Error> {
                    // Cancellation is handled via the steering channel.
                    // The watch channel is set to Abort by the parent process.
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Prompt handler — the agent loop
// ---------------------------------------------------------------------------

use crate::ai::SYSTEM_PROMPT;

/// Execute the agent turn loop for a prompt.
async fn handle_prompt_loop(
    state: SharedState,
    req: PromptRequest,
    cx: &ConnectionTo<Client>,
) -> StopReason {
    let session_id_str = req.session_id.0.to_string();

    // Extract user text from prompt content blocks
    let user_text = extract_text_from_content(&req.prompt);

    // Add user message to conversation
    {
        let mut st = state.lock().await;
        if let Some(session) = st.sessions.get_mut(&session_id_str) {
            session.conversation.push(ConversationEntry {
                role: MessageRole::User,
                content: user_text.clone(),
                is_tool_result: false,
            });
        }
    }

    // Agent loop: LLM → tools → LLM → ... → final response
    let max_iterations = 20;
    for _iteration in 0..max_iterations {
        // Check steering channel
        {
            let st = state.lock().await;
            let cmd = st.steering_rx.borrow().clone();
            match cmd {
                SteeringCommand::Abort => {
                    return StopReason::Cancelled;
                }
                SteeringCommand::Redirect(new_prompt) => {
                    // Replace last user message and restart
                    drop(st);
                    let mut st = state.lock().await;
                    if let Some(session) = st.sessions.get_mut(&session_id_str) {
                        if let Some(last_user) = session
                            .conversation
                            .iter_mut()
                            .rev()
                            .find(|e| e.role == MessageRole::User)
                        {
                            last_user.content = new_prompt;
                        }
                    }
                    continue;
                }
                SteeringCommand::None => {}
            }
        }

        // Assemble context for LLM
        let messages = {
            let mut st = state.lock().await;
            let session = match st.sessions.get_mut(&session_id_str) {
                Some(s) => s,
                None => return StopReason::EndTurn,
            };

            // Compact if needed
            compact_context(SYSTEM_PROMPT, &mut session.conversation);

            // Build message list: system + conversation
            let mut msgs = vec![ChatMessage {
                role: MessageRole::System,
                content: SYSTEM_PROMPT.to_string(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            }];
            for entry in &session.conversation {
                msgs.push(ChatMessage {
                    role: entry.role.clone(),
                    content: entry.content.clone(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                });
            }
            msgs
        };

        // Call LLM
        let llm_response = call_llm(&messages).await;

        // Parse response for tool calls
        let parsed = parse_llm_response(&llm_response);

        match parsed {
            LlmParsed::FinalText(text) => {
                // Send agent_message_chunk notification
                let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone())));
                let notif = SessionNotification::new(
                    req.session_id.clone(),
                    SessionUpdate::AgentMessageChunk(chunk),
                );
                let _ = cx.send_notification(notif);

                // Store assistant response
                let mut st = state.lock().await;
                if let Some(session) = st.sessions.get_mut(&session_id_str) {
                    session.conversation.push(ConversationEntry {
                        role: MessageRole::Assistant,
                        content: text,
                        is_tool_result: false,
                    });
                }

                return StopReason::EndTurn;
            }
            LlmParsed::ToolCalls(tool_calls) => {
                for tc in tool_calls {
                    // Check steering before each tool
                    {
                        let st = state.lock().await;
                        if matches!(*st.steering_rx.borrow(), SteeringCommand::Abort) {
                            return StopReason::Cancelled;
                        }
                    }

                    let tool_call_id = {
                        let mut st = state.lock().await;
                        let session = st.sessions.get_mut(&session_id_str).unwrap();
                        session.next_tool_call_id()
                    };

                    // Notify: tool_call started
                    let kind = tool_kind_for(&tc.name);
                    let acp_tc = AcpToolCall::new(
                        ToolCallId::new(tool_call_id.clone()),
                        format!("{}: {}", tc.name, tc.summary()),
                    )
                    .kind(kind)
                    .status(ToolCallStatus::InProgress);

                    let notif = SessionNotification::new(
                        req.session_id.clone(),
                        SessionUpdate::ToolCall(acp_tc),
                    );
                    let _ = cx.send_notification(notif);

                    // Execute tool
                    let result = execute_tool(&tc, cx, &req.session_id).await;

                    // Notify: tool_call completed
                    let update_fields = ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Completed);
                    let update = ToolCallUpdate::new(
                        ToolCallId::new(tool_call_id.clone()),
                        update_fields,
                    );
                    let notif = SessionNotification::new(
                        req.session_id.clone(),
                        SessionUpdate::ToolCallUpdate(update),
                    );
                    let _ = cx.send_notification(notif);

                    // Add assistant tool call + tool result to conversation
                    let mut st = state.lock().await;
                    if let Some(session) = st.sessions.get_mut(&session_id_str) {
                        session.conversation.push(ConversationEntry {
                            role: MessageRole::Assistant,
                            content: format!("[tool_call: {} {}]", tc.name, serde_json::to_string(&tc.arguments).unwrap_or_default()),
                            is_tool_result: false,
                        });
                        session.conversation.push(ConversationEntry {
                            role: MessageRole::User, // tool results fed back as "user" role
                            content: format!("[tool_result: {}] {}", tc.name, result),
                            is_tool_result: true,
                        });
                    }
                }
            }
        }
    }

    // Exceeded max iterations
    StopReason::MaxTurnRequests
}

// ---------------------------------------------------------------------------
// LLM integration
// ---------------------------------------------------------------------------

/// Call the LLM with assembled messages.
/// Uses the configured provider from AiSettings (via env vars for standalone agent).
async fn call_llm(messages: &[ChatMessage]) -> String {
    use crate::ai::{self, provider::AiProvider};
    use crate::AiSettings;

    // Read settings from env
    let settings = AiSettings {
        active_provider: if std::env::var("OPENROUTER_API_KEY").is_ok() {
            ai::ProviderKind::OpenRouter
        } else if std::env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok() {
            ai::ProviderKind::Bedrock
        } else {
            ai::ProviderKind::Mock
        },
        review_edits: false,
        review_commands: false,
        log_show_only_diffs: false,
        n_expected_rounds: 8,
        openrouter_api_key: std::env::var("OPENROUTER_API_KEY").ok(),
        bedrock_api_key: std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok(),
        bedrock_region: std::env::var("AWS_REGION").ok().or(Some("eu-west-1".to_string())),
        selected_model: None, // Will use default model below
        summary_model: None,
        spend_cap_usd: std::env::var("SPEND_CAP_USD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0),
        ..Default::default()
    };

    // Pick a default model if none configured
    let model = settings.selected_model.clone().unwrap_or_else(|| ai::ModelConfig {
        provider: settings.active_provider.clone(),
        model_id: std::env::var("MODEL_ID").unwrap_or_else(|_| "anthropic/claude-sonnet-4-20250514".to_string()),
        display_name: "Agent Model".to_string(),
        max_tokens: 16384,
        temperature: 0.0,
        input_cost_per_m: 3.0,
        output_cost_per_m: 15.0,
        cached_input_cost_per_m: 0.3,
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: false,
        ..Default::default()
    });

    let provider: Box<dyn AiProvider + Send + Sync> = match settings.active_provider {
        ai::ProviderKind::OpenRouter => {
            match settings.openrouter_api_key {
                Some(key) => Box::new(ai::openrouter::OpenRouterProvider::new(key)),
                None => return "Error: OPENROUTER_API_KEY not set".to_string(),
            }
        }
        ai::ProviderKind::Bedrock => {
            match settings.bedrock_api_key {
                Some(token) => Box::new(ai::bedrock::BedrockProvider::new(token, settings.bedrock_region)),
                None => return "Error: AWS_BEARER_TOKEN_BEDROCK not set".to_string(),
            }
        }
        ai::ProviderKind::Mock => {
            let (p, _rx) = ai::mock::MockProvider::new();
            Box::new(p)
        }
    };

    let request = ai::AiRequest {
        dynamic_tools: None,
        model,
        messages: messages.to_vec(),
        stop: None,
        tools: None, // ACP agent uses text-based tool calling via parse_llm_response
        cache_breakpoints: Vec::new(),
    };

    match provider.complete(&request).await {
        Ok(response) => response.content,
        Err(e) => format!("LLM error: {}", e.message),
    }
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Parsed tool call from LLM response.
#[derive(Debug, Clone)]
struct ParsedToolCall {
    name: String,
    arguments: serde_json::Value,
}

impl ParsedToolCall {
    fn summary(&self) -> String {
        match self.arguments.get("path") {
            Some(p) => p.as_str().unwrap_or("").to_string(),
            None => String::new(),
        }
    }
}

/// Result of parsing an LLM response.
enum LlmParsed {
    FinalText(String),
    ToolCalls(Vec<ParsedToolCall>),
}

/// Parse LLM response text for tool calls.
/// TODO: Replace with proper structured output parsing once provider returns tool_use blocks.
///
/// Expected format from LLM (JSON tool calls):
/// ```json
/// {"tool_calls": [{"name": "read_file", "arguments": {"path": "/foo"}}]}
/// ```
/// If no tool_calls key found, treat entire response as final text.
fn parse_llm_response(response: &str) -> LlmParsed {
    // Try to parse as JSON with tool_calls
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(response) {
        if let Some(calls) = val.get("tool_calls").and_then(|v| v.as_array()) {
            let parsed: Vec<ParsedToolCall> = calls
                .iter()
                .filter_map(|c| {
                    let name = c.get("name")?.as_str()?.to_string();
                    let arguments = c.get("arguments").cloned().unwrap_or(serde_json::Value::Null);
                    Some(ParsedToolCall { name, arguments })
                })
                .collect();
            if !parsed.is_empty() {
                return LlmParsed::ToolCalls(parsed);
            }
        }
    }
    LlmParsed::FinalText(response.to_string())
}

// ---------------------------------------------------------------------------
// Tool execution
// ---------------------------------------------------------------------------

/// Map tool name → ACP ToolKind.
fn tool_kind_for(name: &str) -> ToolKind {
    match name {
        "read_file" | "list_directory" => ToolKind::Read,
        "write_file" | "create_file" => ToolKind::Edit,
        "delete_file" => ToolKind::Delete,
        "search" | "grep" => ToolKind::Search,
        "run_command" => ToolKind::Execute,
        _ => ToolKind::Other,
    }
}

/// Check if a tool requires write permission.
fn requires_permission(name: &str) -> bool {
    matches!(name, "write_file" | "create_file" | "delete_file" | "run_command")
}

/// Execute a tool, requesting permission for writes.
async fn execute_tool(
    tc: &ParsedToolCall,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
) -> String {
    // Request permission for write operations
    if requires_permission(&tc.name) {
        // TODO: Use RequestPermissionRequest once we confirm the exact API.
        // For now, we log that permission would be requested.
        let _ = (cx, session_id);
        // let perm_req = RequestPermissionRequest { ... };
        // let perm_resp = cx.send_request(perm_req).await;
        // if !perm_resp.granted { return "Permission denied".to_string(); }
    }

    match tc.name.as_str() {
        "read_file" => execute_read_file(&tc.arguments).await,
        "write_file" => execute_write_file(&tc.arguments).await,
        "list_directory" => execute_list_directory(&tc.arguments).await,
        "search" | "grep" => execute_search(&tc.arguments).await,
        "run_command" => execute_run_command(&tc.arguments).await,
        _ => format!("Unknown tool: {}", tc.name),
    }
}

async fn execute_read_file(args: &serde_json::Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: missing 'path' argument".to_string(),
    };
    match tokio::fs::read_to_string(path).await {
        Ok(content) => content,
        Err(e) => format!("Error reading {}: {}", path, e),
    }
}

async fn execute_write_file(args: &serde_json::Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: missing 'path' argument".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: missing 'content' argument".to_string(),
    };
    match tokio::fs::write(path, content).await {
        Ok(()) => format!("Wrote {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing {}: {}", path, e),
    }
}

async fn execute_list_directory(args: &serde_json::Value) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: missing 'path' argument".to_string(),
    };
    match tokio::fs::read_dir(path).await {
        Ok(mut entries) => {
            let mut result = String::new();
            while let Ok(Some(entry)) = entries.next_entry().await {
                result.push_str(&entry.file_name().to_string_lossy());
                result.push('\n');
            }
            result
        }
        Err(e) => format!("Error listing {}: {}", path, e),
    }
}

async fn execute_search(args: &serde_json::Value) -> String {
    let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
    // TODO: Integrate with existing search/grep tools from crate::ai::tool_executor
    format!("[search stub] pattern={} path={}", pattern, path)
}

async fn execute_run_command(args: &serde_json::Value) -> String {
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: missing 'command' argument".to_string(),
    };
    // TODO: Use tokio::process::Command with proper sandboxing
    match tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .await
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!(
                "exit={}\nstdout:\n{}\nstderr:\n{}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            )
        }
        Err(e) => format!("Error running command: {}", e),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract text from ACP content blocks.
fn extract_text_from_content(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tool definitions (for context assembly to LLM)
// ---------------------------------------------------------------------------

/// Get tool definitions as a string for inclusion in the system prompt.
/// TODO: Convert these into proper function-calling format when the provider supports it.
pub fn tool_definitions_prompt() -> &'static str {
    r#"
Available tools:
- read_file(path: string) — Read a file's contents
- write_file(path: string, content: string) — Write content to a file (requires permission)
- list_directory(path: string) — List directory contents
- search(pattern: string, path: string) — Search for text in files
- run_command(command: string) — Run a shell command (requires permission)

To call a tool, respond with JSON:
{"tool_calls": [{"name": "tool_name", "arguments": {"key": "value"}}]}

If you don't need tools, respond with plain text.
"#
}
