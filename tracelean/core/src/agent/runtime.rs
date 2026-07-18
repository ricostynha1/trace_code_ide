//! Agent runtime — the LLM tool loop, extracted from gui_backend/ai_commands.rs.
//!
//! This is the core logic: prompt → [tool loop] → final response.
//! No Tauri, no UI framework. Only depends on core types + EventSink trait.

use serde::Serialize;

use crate::ai::{
    self,
    provider::{AiProvider, AiRequest, ChatMessage, MessageRole},
    ToolCall, ToolRegistry,
    batch_prune_decisions, summarization_decision, CostDecision, PruneContext,
    provider_cache::{CacheMode, ProviderCacheConfig},
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
    /// The final state of messages after the tool loop (including compaction, tool calls, results).
    /// Used by session-based API to persist model_view.
    pub final_messages: Vec<ChatMessage>,
    /// How many loop iterations compacted/pruned context this turn (P7).
    pub compactions: u32,
}

// ─── Constants ───────────────────────────────────────────────────────────────

const TOOL_LOOP_PAUSE_THRESHOLD: usize = 10;
const MAX_CONSECUTIVE_FAILURES: u32 = 6;

use crate::ai::SYSTEM_PROMPT;

// ─── Main entry point ────────────────────────────────────────────────────────

/// Run one agent turn: prompt → [tool loop] → final response.
/// Legacy interface: takes raw messages, compaction is ephemeral (for benchmarks/headless).
pub async fn run_agent_turn(
    ctx: &AgentContext,
    messages: Vec<ChatMessage>,
) -> Result<AgentTurnResult, AgentError> {
    run_agent_turn_inner(ctx, messages).await
}

/// Run one agent turn with persistent session state.
/// Compaction persists in session.model_view across calls — preserves caching.
/// The caller appends the new user message to session before calling this.
pub async fn run_agent_turn_session(
    ctx: &AgentContext,
    session: &mut super::session::ChatSession,
) -> Result<AgentTurnResult, AgentError> {
    session.advance_turn();

    // Use model_view as the messages for LLM (already compacted from prior turns)
    let messages = session.model_view.clone();
    let result = run_agent_turn_inner(ctx, messages).await?;

    // Persist evolved state: final_messages includes all compaction + tool calls + final response
    // This is the model_view for next call — preserves caching prefix stability.
    session.model_view = result.final_messages.clone();

    // Track real context size for the utilization bar (P7)
    session.last_prompt_tokens = result.response.usage.input_tokens;
    session.last_completion_tokens = result.response.usage.output_tokens;
    session.compaction_count += result.compactions;

    // Append only the final assistant response to user_view (user already sees tool calls via events)
    let assistant_msg = ChatMessage {
        role: MessageRole::Assistant,
        content: result.response.content.clone(),
        tool_call_id: None,
        tool_calls: result.response.tool_calls.clone(),
    };
    session.user_view.push(assistant_msg);

    // Mark compacted if model_view diverged from user_view
    if session.model_view.len() != session.user_view.len() + 1 {
        // +1 for system prompt
        session.compacted = true;
    }

    Ok(result)
}

/// Inner implementation shared by both entry points.
async fn run_agent_turn_inner(
    ctx: &AgentContext,
    input_messages: Vec<ChatMessage>,
) -> Result<AgentTurnResult, AgentError> {
    let (model, provider) = build_provider(ctx)?;

    // Load tool registry (static tools always sent)
    const TOOLS_JSON: &str = include_str!("../../../data/tools.json");
    let tool_registry = ToolRegistry::load_from_str(TOOLS_JSON)
        .expect("embedded data/tools.json must parse");

    // Build all tools = static + dynamic from registry + extra (e.g. MCP)
    let all_tools = {
        let mut schemas = tool_registry.request_schemas();
        for tool in &ctx.extra_tools {
            schemas.push(tool.to_tool_schema());
        }
        schemas
    };

    // Prepend system prompt (if not already present)
    let mut messages = if input_messages.first().map(|m| m.role == MessageRole::System).unwrap_or(false) {
        input_messages
    } else {
        let mut full = vec![ChatMessage {
            role: MessageRole::System,
            content: SYSTEM_PROMPT.to_string(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }];
        full.extend(input_messages);
        full
    };

    let mut consecutive_failures: u32 = 0;
    let mut loop_i: u32 = 0;
    let mut total_tool_calls: usize = 0;
    let mut total_compactions: u32 = 0;
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

        // Cost-aware context compaction (prune/summarize if profitable)
        let was_compacted = if loop_i > 0 {
            let saved = cost_aware_compact(ctx, &mut messages, &model, provider.as_ref()).await;
            if ctx.verbose && saved > 0 {
                eprintln!("[compaction] freed ~{} tokens", saved);
            }
            saved > 0
        } else {
            false
        };
        if was_compacted {
            total_compactions += 1;
        }

        let request = AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            stop: None,
            tools: Some(all_tools.clone()),
        };

        // Verbose: show what we're sending (markdown format for easy inspection)
        if ctx.verbose {
            eprintln!("\n---");
            eprintln!("## ITERATION {} — sending {} messages to model", loop_i + 1, request.messages.len());
            eprintln!("");
            for m in &request.messages {
                let role_tag = match m.role {
                    MessageRole::System => "**[system]**",
                    MessageRole::User => "**[user]**",
                    MessageRole::Assistant => "**[assistant]**",
                    MessageRole::Tool => "**[tool]**",
                };
                let content_preview = if m.content.len() > 500 {
                    format!("{}… ({} chars)", &m.content[..500], m.content.len())
                } else {
                    m.content.clone()
                };
                if let Some(ref tcid) = m.tool_call_id {
                    eprintln!("{} (tool_call_id=`{}`)", role_tag, tcid);
                } else {
                    eprintln!("{}", role_tag);
                }
                eprintln!("{}", content_preview);
                if !m.tool_calls.is_empty() {
                    for tc in &m.tool_calls {
                        eprintln!("  - 📞 `{}({})`", tc.function.name, &tc.function.arguments[..tc.function.arguments.len().min(200)]);
                    }
                }
                eprintln!();
            }
            eprintln!("---");
        }

        // Record request timing (idle time → cache cold detection)
        if let Ok(mut tracker) = ctx.timing_tracker.lock() {
            tracker.record_request_sent(loop_i as usize);
        }

        let start = std::time::Instant::now();
        let result = provider.complete(&request).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Record response timing
        if let Ok(mut tracker) = ctx.timing_tracker.lock() {
            tracker.record_response_received(loop_i as usize);
        }

        match result {
            Ok(response) => {
                // Verbose: show response
                if ctx.verbose {
                    eprintln!("\n### ◀ RESPONSE ({}ms, {} in/{} out tokens)",
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
                        eprintln!("\n**Tool calls ({}):**", response.tool_calls.len());
                        for tc in &response.tool_calls {
                            let args_preview = if tc.function.arguments.len() > 300 {
                                format!("{}…", &tc.function.arguments[..300])
                            } else {
                                tc.function.arguments.clone()
                            };
                            eprintln!("  - 🔧 `{}({})`", tc.function.name, args_preview);
                        }
                    }
                    eprintln!("---");
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
                    if was_compacted {
                        log.mark_last_entry_compacted();
                    }
                }
                {
                    let mut s = ctx.stats.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
                    s.record(&response.usage, &cost);
                }
                ctx.event_sink.emit("ai-stats-updated", "");

                // Verbose: per-iteration cost breakdown
                if ctx.verbose {
                    let s = ctx.stats.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
                    let cache_pct = if response.usage.input_tokens > 0 {
                        (response.usage.cached_tokens as f64 / response.usage.input_tokens as f64) * 100.0
                    } else {
                        0.0
                    };
                    eprintln!("> **Iteration {} cost:** ${:.6} | cached: {}/{} tokens ({:.1}%) | cumulative: ${:.6}",
                        loop_i + 1,
                        cost.total_usd,
                        response.usage.cached_tokens,
                        response.usage.input_tokens,
                        cache_pct,
                        s.total_cost_usd,
                    );
                    eprintln!("");
                }

                // No tool calls → final response
                if response.tool_calls.is_empty() {
                    // Append final assistant response to messages for session persistence
                    messages.push(ChatMessage {
                        role: MessageRole::Assistant,
                        content: response.content.clone(),
                        tool_call_id: None,
                        tool_calls: Vec::new(),
                    });
                    return Ok(AgentTurnResult {
                        response,
                        tool_calls_executed: all_tool_records,
                        iterations: loop_i + 1,
                        final_messages: messages,
                        compactions: total_compactions,
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
                        .filter_map(|tc| {
                            match serde_json::from_str::<serde_json::Value>(&tc.function.arguments) {
                                Ok(_) => None,
                                Err(e) => Some(ai::tool_errors::format_tool_call_error(
                                    Some(&tc.function.name),
                                    &tc.function.arguments,
                                    &format!("invalid JSON arguments: {}", e),
                                    Some(&tool_registry),
                                    model.tool_call_format,
                                )),
                            }
                        })
                        .collect();

                    let error_msg = error_details.join("\n\n");

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

                    // Parse arguments, then validate against the tool's schema
                    // before execution (D3.3) — both failures produce the same
                    // canonical actionable error.
                    let parsed: Result<serde_json::Value, String> =
                        serde_json::from_str(&tc.function.arguments)
                            .map_err(|e| format!("invalid JSON arguments: {}", e))
                            .and_then(|v: serde_json::Value| {
                                match tool_registry.schema_for(tool_name) {
                                    Some(schema) => ai::tool_errors::validate_args(&v, schema)
                                        .map(|_| v)
                                        .map_err(|e| format!("invalid arguments: {}", e)),
                                    None => Ok(v),
                                }
                            });
                    let arguments: serde_json::Value = match parsed {
                        Ok(v) => v,
                        Err(problem) => {
                            let error_msg = ai::tool_errors::format_tool_call_error(
                                Some(tool_name),
                                &tc.function.arguments,
                                &problem,
                                Some(&tool_registry),
                                model.tool_call_format,
                            );
                            ctx.event_sink.emit(
                                "tool-call",
                                &serde_json::to_string(&ToolCallEvent {
                                    tool_name: tool_name.clone(),
                                    status: ToolCallStatus::Failed {
                                        error: problem.clone(),
                                    },
                                    duration_ms: Some(0),
                                    depth: 0,
                                    reason: None,
                                    call_id: Some(tc.id.clone()),
                                    args_preview: Some(crate::preview_str(
                                        &tc.function.arguments,
                                        200,
                                    )),
                                    result_preview: Some(crate::preview_str(&problem, 200)),
                                })
                                .unwrap_or_default(),
                            );
                            messages.push(ChatMessage {
                                role: MessageRole::Tool,
                                content: error_msg,
                                tool_call_id: Some(tc.id.clone()),
                                tool_calls: Vec::new(),
                            });
                            consecutive_failures += 1;
                            continue;
                        }
                    };

                    // Emit "running" event (chip UI renders these; no prose message)
                    ctx.event_sink.emit(
                        "tool-call",
                        &serde_json::to_string(&ToolCallEvent {
                            tool_name: tool_name.clone(),
                            status: ToolCallStatus::Running,
                            duration_ms: None,
                            depth: 0,
                            reason: None,
                            call_id: Some(tc.id.clone()),
                            args_preview: Some(crate::preview_str(&tc.function.arguments, 200)),
                            result_preview: None,
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
                        eprintln!("  - {} `{}` → {} ({}ms)", status_icon, tool_name, result_preview, tool_duration);
                    }

                    // Emit completion event
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
                            call_id: Some(tc.id.clone()),
                            args_preview: Some(crate::preview_str(&tc.function.arguments, 200)),
                            result_preview: Some(crate::preview_str(&tool_result.content, 200)),
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
                        final_messages: messages,
                        compactions: total_compactions,
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
            let mut bp = ai::bedrock::BedrockProvider::new(
                token,
                settings.bedrock_region.clone(),
            );
            bp.verbose = ctx.verbose;
            Box::new(bp)
        }
        ai::ProviderKind::Mock => {
            let (p, _rx) = ai::mock::MockProvider::new();
            Box::new(p)
        }
    };

    Ok((model, provider))
}

/// Extract user query from messages for tool selection (sliding window of last 3 user turns).
#[allow(dead_code)]
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

// ─── Cost-Aware Context Compaction ────────────────────────────────────────────

/// Derive a ProviderCacheConfig from ModelConfig (best-effort from pricing data).
fn cache_config_from_model(model: &ai::ModelConfig) -> ProviderCacheConfig {
    let has_cache = model.supports_caching && model.cached_input_cost_per_m > 0.0;
    if has_cache {
        // Derive read discount from pricing ratio
        let discount = model.cached_input_cost_per_m / model.input_cost_per_m.max(0.001);
        ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: discount.clamp(0.01, 0.99),
            cache_write_multiplier: 0.0,
            ttl_seconds: Some(300), // conservative default
            requires_markers: false,
            notes: None,
        }
    } else {
        // No caching — prune is always free
        ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.0,
            cache_write_multiplier: 0.0,
            ttl_seconds: Some(0), // always cold → prune always profitable
            requires_markers: false,
            notes: None,
        }
    }
}

/// Run cost-aware context compaction before sending messages to the LLM.
///
/// This replaces dumb truncation with:
/// 1. Use RetentionEngine to identify eligible/summarizable entries
/// 2. Use batch_prune_decisions + summarization_decision with real pricing
/// 3. For Summarize decisions, call the summary_model (or main model) to compress
/// 4. Prune entries that are net-positive to drop
///
/// Modifies `messages` in-place. Returns number of tokens saved.
async fn cost_aware_compact(
    ctx: &AgentContext,
    messages: &mut Vec<ChatMessage>,
    model: &ai::ModelConfig,
    provider: &(dyn AiProvider + Send + Sync),
) -> usize {
    // Estimate tokens for the prune decision (trivial math, always run).
    let total_tokens_est: usize = messages.iter().map(|m| {
        let content_tokens = m.content.len() / 4;
        let tc_tokens: usize = m.tool_calls.iter().map(|tc| tc.function.arguments.len() / 4 + 5).sum();
        content_tokens + tc_tokens
    }).sum();
    let message_overhead = messages.len() * 4;
    let _effective_estimate = total_tokens_est + message_overhead;

    let cache_cfg = cache_config_from_model(model);

    // Get summary model (if configured) for pricing
    let summary_model_cfg = ctx.settings
        .lock()
        .ok()
        .and_then(|s| s.summary_model.clone());

    let prune_ctx = PruneContext::from_models(
        cache_cfg,
        model,
        summary_model_cfg.as_ref(),
    );

    // Feed messages into retention engine and get decisions
    let (to_prune, to_summarize, tokens_freed) = {
        let mut engine = match ctx.retention_engine.lock() {
            Ok(e) => e,
            Err(_) => return 0, // can't lock → skip compaction
        };

        // Update timing
        if let Ok(tracker) = ctx.timing_tracker.lock() {
            let idle_secs = tracker.last_idle_secs();
            let _ = idle_secs;
        }

        engine.advance_turn();

        // Only register NEW messages not already tracked by the engine.
        // The engine.user_view.entries.len() tells us how many we've already added.
        // Messages layout: [system, ...conversation]. We skip system at [0].
        let already_registered = engine.user_view.entries.len();
        let new_msgs = messages.iter().enumerate().skip(1) // skip system
            .skip(already_registered); // skip already-registered

        for (_i, msg) in new_msgs {
            let is_tool = msg.role == MessageRole::Tool;
            let tokens = msg.content.len() / 4;
            if tokens > 0 {
                // Assign creation turn based on position: older messages get earlier turns.
                // Each pair of (assistant + tool) messages is roughly 1 turn.
                let msg_turn = engine.current_turn;
                engine.add_entry(crate::ai::retention::RetentionEntry {
                    id: 0, // auto-assigned
                    kind: if is_tool {
                        crate::ai::retention::EntryKind::ToolResult
                    } else if msg.role == MessageRole::User {
                        crate::ai::retention::EntryKind::UserMsg
                    } else {
                        crate::ai::retention::EntryKind::AssistantMsg
                    },
                    content: msg.content.clone(),
                    resources: Vec::new(),
                    created_turn: msg_turn,
                    last_used_turn: msg_turn,
                    approx_tokens: tokens,
                    ttl: None,
                    invalidation_events: Vec::new(),
                    action: crate::ai::retention::RetentionAction::Eligible,
                    args_hash: None,
                    ephemeral: false,
                    offloaded: false,
                    offload_path: None,
                });
            }
        }

        // Get prune decisions
        let eligible = engine.eligible_for_pruning();
        let (prune_ids, _logs) = batch_prune_decisions(&eligible, &prune_ctx, engine.current_turn);

        // Get summarization decision
        let summarizable = engine.summarizable_entries();
        let sum_decision = if !summarizable.is_empty() {
            summarization_decision(&summarizable, &prune_ctx)
        } else {
            CostDecision::Keep { reason: "nothing to summarize".into() }
        };

        // Collect IDs to summarize
        let summarize_ids: Vec<u64> = match &sum_decision {
            CostDecision::Summarize { .. } => {
                summarizable.iter().map(|e| e.id).collect()
            }
            _ => Vec::new(),
        };

        // Collect content for summarization before pruning
        let summarize_content: Vec<String> = summarizable.iter()
            .filter(|e| summarize_ids.contains(&e.id))
            .map(|e| e.content.clone())
            .collect();

        let freed: usize = eligible.iter()
            .filter(|e| prune_ids.contains(&e.id))
            .map(|e| e.approx_tokens)
            .sum();

        // Apply prune
        engine.prune_entries(&prune_ids);

        (prune_ids, summarize_content, freed)
    };

    // If there are entries to summarize, call the LLM
    if !to_summarize.is_empty() {
        let summary = call_summary_llm(ctx, model, provider, &to_summarize).await;
        if let Some(summary_text) = summary {
            // Insert summary into retention engine
            if let Ok(mut engine) = ctx.retention_engine.lock() {
                let sum_tokens = summary_text.len() / 4;
                engine.insert_summary(&[], summary_text.clone(), sum_tokens);
            }

            // Insert summary message into the conversation (after system prompt)
            let summary_msg = ChatMessage {
                role: MessageRole::User,
                content: format!("[Context Summary]\n{}", summary_text),
                tool_call_id: None,
                tool_calls: Vec::new(),
            };

            // Remove old messages that were summarized + pruned, insert summary
            // Strategy: keep system[0], keep last 6 messages, replace middle with summary
            let keep_tail = 6.min(messages.len().saturating_sub(1));
            let cut_end = messages.len() - keep_tail;
            if cut_end > 1 {
                messages.drain(1..cut_end);
                messages.insert(1, summary_msg);
            }
        }
    } else if !to_prune.is_empty() {
        // Aggressive prune: if message count is high, remove old tool-loop pairs.
        // Keep: system[0], user[1], last N messages (recent context).
        // Middle tool-loop messages (assistant with tool_calls + tool results) get dropped.
        let keep_tail = 8.min(messages.len().saturating_sub(2)); // keep last 8 msgs
        let keep_head = 2; // system + original user prompt
        if messages.len() > keep_head + keep_tail {
            let cut_start = keep_head;
            let cut_end = messages.len() - keep_tail;
            // Build a brief summary of what was pruned
            let pruned_count = cut_end - cut_start;
            let pruned_tools: Vec<String> = messages[cut_start..cut_end].iter()
                .filter(|m| !m.tool_calls.is_empty())
                .flat_map(|m| m.tool_calls.iter().map(|tc| tc.function.name.clone()))
                .collect();
            let prune_note = format!(
                "[Context compacted: {} messages pruned. Tools called: {}]",
                pruned_count,
                if pruned_tools.is_empty() { "none".to_string() } else { pruned_tools.join(", ") }
            );
            messages.drain(cut_start..cut_end);
            messages.insert(cut_start, ChatMessage {
                role: MessageRole::User,
                content: prune_note,
                tool_call_id: None,
                tool_calls: Vec::new(),
            });
        } else {
            // Not enough to drain — just truncate tool results
            let cut_point = messages.len().saturating_sub(keep_tail);
            for msg in messages[1..cut_point].iter_mut() {
                if msg.role == MessageRole::Tool && msg.content.len() > 200 {
                    msg.content.truncate(200);
                    msg.content.push_str("... [pruned]");
                }
            }
        }
    }

    tokens_freed
}

/// Call the summary/compression model to produce a condensed version of conversation entries.
/// Uses summary_model if configured, otherwise falls back to main model.
async fn call_summary_llm(
    ctx: &AgentContext,
    main_model: &ai::ModelConfig,
    main_provider: &(dyn AiProvider + Send + Sync),
    entries: &[String],
) -> Option<String> {
    let combined = entries.join("\n---\n");
    if combined.is_empty() {
        return None;
    }

    let prompt = format!(
        "Summarize the following conversation context into a concise but complete summary. \
         Preserve all important facts, file paths, code decisions, and tool results. \
         Remove redundancy and verbose tool output. Be terse.\n\n---\n{}",
        combined
    );

    let summary_model_cfg = ctx.settings
        .lock()
        .ok()
        .and_then(|s| s.summary_model.clone());

    let (model_to_use, provider_to_use): (ai::ModelConfig, Option<Box<dyn AiProvider + Send + Sync>>) =
        if let Some(ref sm) = summary_model_cfg {
            // Build a provider for the summary model
            match build_summary_provider(ctx, sm) {
                Ok(p) => (sm.clone(), Some(p)),
                Err(_) => (main_model.clone(), None), // fallback to main
            }
        } else {
            (main_model.clone(), None)
        };

    let request = AiRequest {
        model: model_to_use,
        messages: vec![
            ChatMessage {
                role: MessageRole::System,
                content: "You are a context compression assistant. Summarize concisely.".into(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
            ChatMessage {
                role: MessageRole::User,
                content: prompt,
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
        ],
        stop: None,
        tools: None,
    };

    let provider_ref: &(dyn AiProvider + Send + Sync) = match &provider_to_use {
        Some(p) => p.as_ref(),
        None => main_provider,
    };

    match provider_ref.complete(&request).await {
        Ok(response) => {
            // Record cost
            let cost = response.usage.estimate_cost(
                request.model.input_cost_per_m,
                request.model.output_cost_per_m,
                request.model.cached_input_cost_per_m,
            );
            if let Ok(mut s) = ctx.stats.lock() {
                s.record(&response.usage, &cost);
            }
            if let Ok(mut log) = ctx.log.lock() {
                log.record_success("summarization", &request, &response, 0);
            }
            Some(response.content)
        }
        Err(e) => {
            if ctx.verbose {
                eprintln!("[summarization failed: {}]", e.message);
            }
            None
        }
    }
}

/// Build a provider for the summary model (same logic as build_provider but for a specific model).
fn build_summary_provider(
    ctx: &AgentContext,
    model: &ai::ModelConfig,
) -> Result<Box<dyn AiProvider + Send + Sync>, AgentError> {
    let settings = ctx.settings.lock().map_err(|e| AgentError::Lock(e.to_string()))?;

    let provider: Box<dyn AiProvider + Send + Sync> = match model.provider {
        ai::ProviderKind::OpenRouter => {
            let key = settings
                .openrouter_api_key
                .clone()
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .ok_or_else(|| AgentError::ProviderNotConfigured("OpenRouter API key not set".into()))?;
            Box::new(ai::openrouter::OpenRouterProvider::new(key))
        }
        ai::ProviderKind::Bedrock => {
            let token = settings
                .bedrock_api_key
                .clone()
                .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok())
                .ok_or_else(|| AgentError::ProviderNotConfigured("Bedrock bearer token not set".into()))?;
            Box::new(ai::bedrock::BedrockProvider::new(token, settings.bedrock_region.clone()))
        }
        ai::ProviderKind::Mock => {
            let (p, _rx) = ai::mock::MockProvider::new();
            Box::new(p)
        }
    };

    Ok(provider)
}
