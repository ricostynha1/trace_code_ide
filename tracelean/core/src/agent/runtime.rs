//! Agent runtime — the LLM tool loop, extracted from gui_backend/ai_commands.rs.
//!
//! This is the core logic: prompt → [tool loop] → final response.
//! No Tauri, no UI framework. Only depends on core types + EventSink trait.

use serde::Serialize;

use crate::ai::{
    self,
    provider::{AiProvider, AiRequest, ChatMessage, MessageRole},
    tools::builtin_tool_schemas,
    ToolCall, ToolIndex,
};
use crate::{ToolCallEvent, ToolCallStatus};

use super::context::AgentContext;
use super::error::AgentError;

// ─── Public types ────────────────────────────────────────────────────────────

/// Record of a single tool call executed during a turn.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub success: bool,
    pub duration_ms: u64,
}

/// Result of a complete agent turn (possibly multi-iteration tool loop).
#[derive(Debug, Clone)]
pub struct AgentTurnResult {
    pub response: ai::AiResponse,
    pub tool_calls_executed: Vec<ToolCallRecord>,
    pub iterations: u32,
}

// ─── Constants ───────────────────────────────────────────────────────────────

const TOOL_LOOP_PAUSE_THRESHOLD: usize = 10;
const MAX_CONSECUTIVE_FAILURES: u32 = 6;

const SYSTEM_PROMPT: &str = "You are TraceLean, an AI coding assistant embedded in an IDE.\n\
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

// ─── Main entry point ────────────────────────────────────────────────────────

/// Run one agent turn: prompt → [tool loop] → final response.
pub async fn run_agent_turn(
    ctx: &AgentContext,
    messages: Vec<ChatMessage>,
) -> Result<AgentTurnResult, AgentError> {
    let (model, provider) = build_provider(ctx)?;

    // Build tool schemas from builtin + extra tools (e.g. MCP)
    let all_tool_schemas = {
        let mut schemas = builtin_tool_schemas();
        for tool in &ctx.extra_tools {
            schemas.push(tool.to_tool_schema());
        }
        schemas
    };

    let tool_index = ToolIndex::new(&all_tool_schemas);

    // Extract user query for tool selection (sliding window of last 3 turns)
    let user_query = extract_user_query(&messages);

    // Prepend system prompt
    let mut messages = {
        let mut full = vec![ChatMessage {
            role: MessageRole::System,
            content: SYSTEM_PROMPT.to_string(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }];
        full.extend(messages);
        full
    };

    let mut previously_called: Vec<String> = Vec::new();
    let mut consecutive_failures: u32 = 0;
    let mut loop_i: u32 = 0;
    let mut total_tool_calls: usize = 0;
    let mut all_tool_records: Vec<ToolCallRecord> = Vec::new();

    loop {
        // Enforce spend cap
        {
            let s = ctx.stats.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
            if s.total_cost_usd >= ctx.spend_cap_usd {
                return Err(AgentError::SpendCapReached {
                    spent: s.total_cost_usd,
                    cap: ctx.spend_cap_usd,
                });
            }
        }

        // Select relevant tools for this turn
        let selected_tools =
            tool_index.select_with_context(&user_query, &previously_called, None);

        let request = AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            stop: None,
            tools: if selected_tools.is_empty() {
                None
            } else {
                Some(selected_tools)
            },
        };

        // Verbose: show what we're sending
        if ctx.verbose {
            eprintln!("\n{}", "═".repeat(80));
            eprintln!("▶ ITERATION {} — sending {} messages to model", loop_i + 1, request.messages.len());
            eprintln!("{}", "─".repeat(80));
            for m in &request.messages {
                let role_tag = match m.role {
                    MessageRole::System => "\x1b[90m[system]\x1b[0m",
                    MessageRole::User => "\x1b[34m[user]\x1b[0m",
                    MessageRole::Assistant => "\x1b[32m[assistant]\x1b[0m",
                    MessageRole::Tool => "\x1b[33m[tool]\x1b[0m",
                };
                let content_preview = if m.content.len() > 500 {
                    format!("{}… ({} chars)", &m.content[..500], m.content.len())
                } else {
                    m.content.clone()
                };
                if let Some(ref tcid) = m.tool_call_id {
                    eprintln!("{} (tool_call_id={})", role_tag, tcid);
                } else {
                    eprintln!("{}", role_tag);
                }
                eprintln!("{}", content_preview);
                if !m.tool_calls.is_empty() {
                    for tc in &m.tool_calls {
                        eprintln!("  📞 {}({})", tc.function.name, &tc.function.arguments[..tc.function.arguments.len().min(200)]);
                    }
                }
                eprintln!();
            }
            if let Some(ref tools) = request.tools {
                eprintln!("\x1b[90m[tools offered: {}]\x1b[0m", tools.iter().map(|t| t.function.name.as_str()).collect::<Vec<_>>().join(", "));
            }
            eprintln!("{}", "─".repeat(80));
        }

        let start = std::time::Instant::now();
        let result = provider.complete(&request).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                // Verbose: show response
                if ctx.verbose {
                    eprintln!("\n\x1b[32m◀ RESPONSE\x1b[0m ({}ms, {} in/{} out tokens)",
                        duration_ms, response.usage.input_tokens, response.usage.output_tokens);
                    if !response.content.is_empty() {
                        let preview = if response.content.len() > 1000 {
                            format!("{}… ({} chars)", &response.content[..1000], response.content.len())
                        } else {
                            response.content.clone()
                        };
                        eprintln!("{}", preview);
                    }
                    if !response.tool_calls.is_empty() {
                        eprintln!("\n\x1b[35m  Tool calls ({}):\x1b[0m", response.tool_calls.len());
                        for tc in &response.tool_calls {
                            let args_preview = if tc.function.arguments.len() > 300 {
                                format!("{}…", &tc.function.arguments[..300])
                            } else {
                                tc.function.arguments.clone()
                            };
                            eprintln!("    🔧 {}({})", tc.function.name, args_preview);
                        }
                    }
                    eprintln!("{}", "═".repeat(80));
                }

                // Record stats
                let cost = response.usage.estimate_cost(
                    model.input_cost_per_m,
                    model.output_cost_per_m,
                    model.cached_input_cost_per_m,
                );
                {
                    let mut log = ctx.log.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
                    let label = if loop_i == 0 {
                        "chat".to_string()
                    } else {
                        format!("chat/tool_{}", loop_i)
                    };
                    log.record_success(&label, &request, &response, duration_ms);
                }
                {
                    let mut s = ctx.stats.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
                    s.record(&response.usage, &cost);
                }
                ctx.event_sink.emit("ai-stats-updated", "");

                // No tool calls → final response
                if response.tool_calls.is_empty() {
                    return Ok(AgentTurnResult {
                        response,
                        tool_calls_executed: all_tool_records,
                        iterations: loop_i + 1,
                    });
                }

                // Emit assistant text between tool loops
                if !response.content.is_empty() {
                    let payload = serde_json::json!({
                        "role": "assistant",
                        "content": response.content.clone()
                    });
                    ctx.event_sink
                        .emit("ai-chat-message", &payload.to_string());
                }

                // Check for malformed JSON arguments
                let has_malformed = response
                    .tool_calls
                    .iter()
                    .any(|tc| serde_json::from_str::<serde_json::Value>(&tc.function.arguments).is_err());

                if has_malformed {
                    let error_details: Vec<String> = response
                        .tool_calls
                        .iter()
                        .filter(|tc| {
                            serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                                .is_err()
                        })
                        .map(|tc| {
                            format!(
                                "{}({}) — invalid JSON",
                                tc.function.name, tc.function.arguments
                            )
                        })
                        .collect();

                    let error_msg = format!(
                        "Error: your tool call(s) contained invalid JSON arguments and could not be executed:\n{}\nPlease retry with valid JSON.",
                        error_details.join("\n")
                    );

                    ctx.event_sink.emit(
                        "ai-chat-message",
                        &serde_json::json!({"role": "system", "content": error_msg}).to_string(),
                    );

                    if !response.content.is_empty() {
                        messages.push(ChatMessage {
                            role: MessageRole::Assistant,
                            content: response.content.clone(),
                            tool_call_id: None,
                            tool_calls: Vec::new(),
                        });
                    }
                    messages.push(ChatMessage {
                        role: MessageRole::User,
                        content: error_msg,
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    });

                    consecutive_failures += 1;
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        return Err(AgentError::TooManyFailures(consecutive_failures));
                    }
                    loop_i += 1;
                    continue;
                }

                // Valid tool calls — push assistant message
                messages.push(ChatMessage {
                    role: MessageRole::Assistant,
                    content: response.content.clone(),
                    tool_call_id: None,
                    tool_calls: response.tool_calls.clone(),
                });

                // Execute each tool call
                for tc in &response.tool_calls {
                    let tool_name = &tc.function.name;

                    // Pause every N tool calls
                    total_tool_calls += 1;
                    if total_tool_calls > 0
                        && total_tool_calls % TOOL_LOOP_PAUSE_THRESHOLD == 0
                    {
                        if let Some(ref handler) = ctx.pause_handler {
                            if !handler.should_continue(total_tool_calls).await {
                                return Err(AgentError::StoppedByUser(total_tool_calls));
                            }
                        }
                    }

                    // Parse arguments
                    let arguments: serde_json::Value =
                        match serde_json::from_str(&tc.function.arguments) {
                            Ok(v) => v,
                            Err(parse_err) => {
                                let error_msg = format!(
                                    "tool_call_failed: invalid JSON for {}({}): {}",
                                    tool_name, tc.function.arguments, parse_err
                                );
                                ctx.event_sink.emit(
                                    "ai-chat-message",
                                    &serde_json::json!({"role": "system", "content": error_msg})
                                        .to_string(),
                                );
                                ctx.event_sink.emit(
                                    "tool-call",
                                    &serde_json::to_string(&ToolCallEvent {
                                        tool_name: tool_name.clone(),
                                        status: ToolCallStatus::Failed {
                                            error: error_msg.clone(),
                                        },
                                        duration_ms: Some(0),
                                        depth: 0,
                                        reason: None,
                                    })
                                    .unwrap_or_default(),
                                );
                                messages.push(ChatMessage {
                                    role: MessageRole::Tool,
                                    content: format!(
                                        "Error: invalid JSON arguments — {}",
                                        parse_err
                                    ),
                                    tool_call_id: Some(tc.id.clone()),
                                    tool_calls: Vec::new(),
                                });
                                consecutive_failures += 1;
                                continue;
                            }
                        };

                    previously_called.push(tool_name.clone());

                    // Emit "running" event
                    ctx.event_sink.emit(
                        "ai-chat-message",
                        &serde_json::json!({"role": "system", "content": format!("call {}", tool_name)})
                            .to_string(),
                    );
                    ctx.event_sink.emit(
                        "tool-call",
                        &serde_json::to_string(&ToolCallEvent {
                            tool_name: tool_name.clone(),
                            status: ToolCallStatus::Running,
                            duration_ms: None,
                            depth: 0,
                            reason: None,
                        })
                        .unwrap_or_default(),
                    );

                    // Execute tool
                    let start_tool = std::time::Instant::now();
                    let mcp_call = ToolCall {
                        name: tool_name.clone(),
                        arguments,
                    };
                    let tool_result = {
                        let mut s = ctx.state.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
                        let sym = ctx
                            .symbols
                            .lock()
                            .map_err(|e| AgentError::Lock(e.to_string()))?;
                        let g = ctx
                            .graph
                            .lock()
                            .map_err(|e| AgentError::Lock(e.to_string()))?;
                        ai::tool_executor::execute_tool(
                            &mcp_call,
                            &ctx.project_root,
                            &mut s,
                            &sym,
                            &g,
                            &ctx.permissions,
                        )
                    };
                    let tool_duration = start_tool.elapsed().as_millis() as u64;

                    // Verbose: show tool result
                    if ctx.verbose {
                        let result_preview = if tool_result.content.len() > 500 {
                            format!("{}… ({} chars)", &tool_result.content[..500], tool_result.content.len())
                        } else {
                            tool_result.content.clone()
                        };
                        let status_icon = if tool_result.success { "✅" } else { "❌" };
                        eprintln!("    {} {} → {} ({}ms)", status_icon, tool_name, result_preview, tool_duration);
                    }

                    // Emit response event
                    ctx.event_sink.emit(
                        "ai-chat-message",
                        &serde_json::json!({"role": "system", "content": format!("rsp {}", tool_name)})
                            .to_string(),
                    );
                    let status = if tool_result.success {
                        ToolCallStatus::Completed
                    } else {
                        ToolCallStatus::Failed {
                            error: tool_result.content.clone(),
                        }
                    };
                    ctx.event_sink.emit(
                        "tool-call",
                        &serde_json::to_string(&ToolCallEvent {
                            tool_name: tool_name.clone(),
                            status,
                            duration_ms: Some(tool_duration),
                            depth: 0,
                            reason: None,
                        })
                        .unwrap_or_default(),
                    );

                    // Record
                    all_tool_records.push(ToolCallRecord {
                        tool_name: tool_name.clone(),
                        success: tool_result.success,
                        duration_ms: tool_duration,
                    });

                    // Push tool result message
                    messages.push(ChatMessage {
                        role: MessageRole::Tool,
                        content: tool_result.content.clone(),
                        tool_call_id: Some(tc.id.clone()),
                        tool_calls: Vec::new(),
                    });

                    // Track consecutive failures
                    if tool_result.success {
                        consecutive_failures = 0;
                    } else {
                        consecutive_failures += 1;
                    }
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        break;
                    }
                }

                // Stop if too many consecutive failures
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    let failure_msg = format!(
                        "{}\n\n[Tool execution stopped: {} consecutive failures. Please provide additional guidance.]",
                        response.content, consecutive_failures
                    );
                    return Ok(AgentTurnResult {
                        response: ai::AiResponse {
                            content: failure_msg,
                            usage: response.usage.clone(),
                            raw_response: response.raw_response.clone(),
                            truncated: response.truncated,
                            tool_calls: Vec::new(),
                        },
                        tool_calls_executed: all_tool_records,
                        iterations: loop_i + 1,
                    });
                }

                // Notify about state changes
                ctx.event_sink.emit("undo-tree-changed", "");
                ctx.event_sink.emit("files-changed", "");
            }
            Err(e) => {
                let mut log = ctx.log.lock().map_err(|e2| AgentError::Lock(e2.to_string()))?;
                let label = if loop_i == 0 {
                    "chat".to_string()
                } else {
                    format!("chat/tool_{}", loop_i)
                };
                log.record_failure(&label, &request, &e, duration_ms);
                return Err(AgentError::Provider(e.message));
            }
        }

        loop_i += 1;
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a provider from the current AiSettings in the context.
fn build_provider(
    ctx: &AgentContext,
) -> Result<(ai::ModelConfig, Box<dyn AiProvider + Send + Sync>), AgentError> {
    let settings = ctx
        .settings
        .lock()
        .map_err(|e| AgentError::Lock(e.to_string()))?;

    let model = settings
        .selected_model
        .clone()
        .ok_or(AgentError::NoModel)?;

    let provider: Box<dyn AiProvider + Send + Sync> = match settings.active_provider {
        ai::ProviderKind::OpenRouter => {
            let key = settings
                .openrouter_api_key
                .clone()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .ok_or_else(|| {
                    AgentError::ProviderNotConfigured("OpenRouter API key not set".into())
                })?;
            Box::new(ai::openrouter::OpenRouterProvider::new(key))
        }
        ai::ProviderKind::Bedrock => {
            let token = settings
                .bedrock_api_key
                .clone()
                .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok())
                .ok_or_else(|| {
                    AgentError::ProviderNotConfigured("Bedrock bearer token not set".into())
                })?;
            Box::new(ai::bedrock::BedrockProvider::new(
                token,
                settings.bedrock_region.clone(),
            ))
        }
        ai::ProviderKind::Mock => {
            let (p, _rx) = ai::mock::MockProvider::new();
            Box::new(p)
        }
    };

    Ok((model, provider))
}

/// Extract user query from messages for tool selection (sliding window of last 3 user turns).
fn extract_user_query(messages: &[ChatMessage]) -> String {
    let mut window_parts: Vec<String> = Vec::new();
    let mut turn_count = 0;
    for m in messages.iter().rev() {
        if m.role == MessageRole::Tool {
            continue;
        }
        let label = match m.role {
            MessageRole::User => "User",
            MessageRole::Assistant => "Agent",
            _ => continue,
        };
        window_parts.push(format!("{}: {}", label, m.content));
        if m.role == MessageRole::User {
            turn_count += 1;
            if turn_count >= 3 {
                break;
            }
        }
    }
    window_parts.reverse();
    window_parts.join("\n")
}
