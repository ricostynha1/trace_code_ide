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
    provider_cache::ProviderCacheConfig,
};
use crate::{ToolCallEvent, ToolCallStatus};

use super::context::{AgentContext, DEFAULT_CHARS_PER_TOKEN};
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
    /// Dynamic tools loaded at the end of the turn (bugs.md Feature 3) —
    /// persisted on the session so discovered tools survive across turns.
    pub dynamic_tools: Vec<String>,
    /// bugs.md: exactly what was sent in the last request of this turn
    /// (messages + tool schemas) — the session persists this so the *next*
    /// turn's automatic-cache prediction has something to diff against (see
    /// `ChatSession::last_sent`).
    pub last_sent: SentRequestSnapshot,
}

/// bugs.md: snapshot of the last request actually sent to the provider —
/// messages AND tool schemas. Automatic (non-explicit-marker) caching keys
/// off the whole wire prefix, not just the conversation messages: tool
/// schemas are sent as separate `AiRequest` fields but still occupy prompt
/// tokens and are just as cache-eligible when unchanged turn to turn. Kept
/// out of `AgentTurnResult`'s `Debug`-only default so it round-trips through
/// `ChatSession`'s on-disk JSON too.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct SentRequestSnapshot {
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<crate::ai::provider::ToolSchema>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_tools: Option<Vec<crate::ai::provider::ToolSchema>>,
}

/// Bug 3: session_id -> (prompt_tokens, completion_tokens) of the most
/// recent LLM response seen so far in an in-flight turn. Written once per
/// tool-loop iteration (not just once at turn end) so the context-usage bar
/// can update live during a multi-iteration turn.
pub type LiveContextMap = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (u32, u32)>>>;

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
    run_agent_turn_inner(ctx, messages, Vec::new(), None, None).await
}

/// Run one agent turn with persistent session state.
/// Compaction persists in session.model_view across calls — preserves caching.
/// The caller appends the new user message to session before calling this.
/// `live_context` (Bug 3): when set, the tool loop writes this turn's latest
/// (prompt, completion) token counts into it every iteration — not just once
/// at the end — so a concurrently-polled context-usage bar reflects an
/// in-flight, multi-iteration turn instead of only the previous one.
pub async fn run_agent_turn_session(
    ctx: &AgentContext,
    session: &mut super::session::ChatSession,
    live_context: Option<(LiveContextMap, String)>,
) -> Result<AgentTurnResult, AgentError> {
    session.advance_turn();

    // Use model_view as the messages for LLM (already compacted from prior turns)
    let messages = session.model_view.clone();
    let prev_sent = if session.last_sent.messages.is_empty() {
        None
    } else {
        Some(session.last_sent.clone())
    };
    let result = run_agent_turn_inner(ctx, messages, session.dynamic_tools.clone(), prev_sent, live_context).await?;

    // Persist evolved state: final_messages includes all compaction + tool calls + final response
    // This is the model_view for next call — preserves caching prefix stability.
    session.model_view = result.final_messages.clone();
    // Feature 3: discovered tools persist with the session.
    session.dynamic_tools = result.dynamic_tools.clone();
    // bugs.md: carry the last-sent request across turns so the next turn's
    // automatic-cache prediction (Bedrock/MiniMax etc.) has a prefix to diff
    // against — this was previously a local variable reset every turn, which
    // meant single-request turns (the common case) never got a prediction.
    session.last_sent = result.last_sent.clone();

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
    initial_dynamic_tools: Vec<String>,
    prev_sent_init: Option<SentRequestSnapshot>,
    live_context: Option<(LiveContextMap, String)>,
) -> Result<AgentTurnResult, AgentError> {
    let (model, provider) = build_provider(ctx)?;
    // Load tool registry (static tools always sent)
    const TOOLS_JSON: &str = include_str!("../../../data/tools.json");
    let mut tool_registry = ToolRegistry::load_from_str(TOOLS_JSON)
        .expect("embedded data/tools.json must parse");
    // Feature 3: restore this session's previously discovered tools.
    tool_registry.load_dynamic_tools(&initial_dynamic_tools);
    // Static tool set = registry static + extra (e.g. MCP). This is the FROZEN
    // prefix: it must not change during the session (Feature 3), so discovered
    // tools travel separately in AiRequest::dynamic_tools.
    let static_tools = {
        let mut schemas = tool_registry.static_schemas().to_vec();
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

    // T12: the shell executor polls this flag so Stop can kill an in-flight
    // child process, not just wait for it to finish.
    let exec_permissions = {
        let mut p = ctx.permissions.clone();
        p.cancel_flag = Some(ctx.cancel.flag_handle());
        p
    };

    let mut consecutive_failures: u32 = 0;
    let mut loop_i: u32 = 0;
    let mut total_tool_calls: usize = 0;
    let mut total_compactions: u32 = 0;
    let mut all_tool_records: Vec<ToolCallRecord> = Vec::new();

    // P9b: explicit-cache marker planning + verification state.
    let explicit_cache_cfg = ai::provider_cache::provider_cache_key(&model)
        .and_then(|key| ai::ProviderCacheRegistry::embedded().map(|r| r.get_or_default(key)))
        .filter(|c| c.requires_markers);
    let mut cache_verifier = CacheVerifier::new(explicit_cache_cfg.is_some());
    let mut cache_predictions = ai::cost_trimmed_summary_model::CachePredictionTracker::new();
    // trimmed_summary_auto_decision.md item 2: how many prefix tokens were
    // actually served from cache last response — the real signal the
    // cut-point cost model needs, fed in place of the constant 0.
    let mut last_cached_tokens: usize = 0;
    // bugs.md Bug 3: what was actually sent (messages + tool schemas) in the
    // last successful request, so automatic-caching providers (no explicit
    // markers at all — Bedrock/MiniMax) still get a cache-health prediction,
    // not just the explicit-marker path.
    let mut prev_sent_messages: Option<Vec<ChatMessage>> = prev_sent_init.as_ref().map(|s| s.messages.clone());
    let mut prev_sent_tools: Option<Vec<crate::ai::provider::ToolSchema>> =
        prev_sent_init.as_ref().and_then(|s| s.tools.clone());
    let mut prev_sent_dynamic_tools: Option<Vec<crate::ai::provider::ToolSchema>> =
        prev_sent_init.as_ref().and_then(|s| s.dynamic_tools.clone());

    loop {
        // T12: hard stop — checked at the top of every iteration so a Stop
        // clicked between requests takes effect immediately.
        if ctx.cancel.is_cancelled() {
            return Err(AgentError::StoppedByUser(total_tool_calls));
        }

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

        // Cost-aware context compaction (prune/summarize if profitable).
        // Runs on every request including the first of a turn — in persistent
        // sessions that first request carries the whole accumulated history
        // (bugs.md Bug 5: compaction almost never actuated).
        let cached_prefix_tokens_for_prune = relative_cached_prefix_tokens(
            last_cached_tokens,
            prev_sent_messages.as_deref(),
            &prev_sent_tools,
            &prev_sent_dynamic_tools,
        );
        let compaction_info = {
            let info = cost_aware_compact(ctx, &mut messages, &model, provider.as_ref(), cached_prefix_tokens_for_prune).await;
            if ctx.verbose {
                if let Some(i) = &info {
                    eprintln!(
                        "[compaction] {} {} messages, ~{} → ~{} tokens",
                        i.kind, i.messages_removed, i.tokens_before, i.tokens_after
                    );
                }
            }
            info
        };
        let was_compacted = compaction_info.is_some();
        if was_compacted {
            total_compactions += 1;
        }

        // Computed ahead of `plan_cache_breakpoints` below so its marker
        // economics can be costed against the tool schemas actually being
        // sent this turn, instead of assuming they're free (Bug B).
        let dynamic_schemas: Vec<crate::ai::provider::ToolSchema> =
            tool_registry.dynamic_schemas().into_iter().cloned().collect();
        let dynamic_tools_this_turn = if dynamic_schemas.is_empty() {
            None
        } else {
            Some(dynamic_schemas.clone())
        };

        // Session-calibrated chars-per-token ratio (bugs.md: the flat
        // chars/4 default under-predicted real cached tokens by 50-200% on
        // JSON/code-heavy tool traffic, which fed both the shown prediction
        // AND the marker floor/economics checks). Read once per request so
        // every estimate below is internally consistent.
        let chars_per_token = ctx
            .calibrated_chars_per_token
            .lock()
            .map(|r| *r)
            .unwrap_or(DEFAULT_CHARS_PER_TOKEN);

        // P9b (D9b.1): plan write-if-worth-it markers for this request.
        let cache_breakpoints: Vec<usize> = if cache_verifier.enabled {
            explicit_cache_cfg
                .as_ref()
                .map(|cfg| {
                    let cold_ratio = ctx
                        .timing_tracker
                        .lock()
                        .map(|t| t.cold_turn_ratio())
                        .unwrap_or(1.0);
                    let static_tools_tokens = estimate_tools_tokens_ratio(&Some(static_tools.clone()), chars_per_token);
                    let dynamic_tools_tokens = estimate_tools_tokens_ratio(&dynamic_tools_this_turn, chars_per_token);
                    plan_cache_breakpoints(
                        cfg,
                        &messages,
                        loop_i,
                        cold_ratio,
                        static_tools_tokens,
                        dynamic_tools_tokens,
                        model.cache_min_tokens as usize,
                        chars_per_token,
                    )
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let sent_prefix_tokens: usize = cache_breakpoints
            .iter()
            .max()
            .map(|&i| messages.iter().take(i + 1).map(|m| estimate_msg_tokens_ratio(m, chars_per_token)).sum())
            .unwrap_or(0);

        // D9b.3: markers sent last request → this one should read from cache.
        // bugs.md Bug 3: remember the prediction so the log entry for this
        // exact request can show predicted vs. actual cache health.
        let predicted_cache_this_turn = if cache_verifier.predicted_prefix_tokens > 0 {
            let total_est: usize = messages.iter().map(|m| estimate_msg_tokens_ratio(m, chars_per_token)).sum();
            cache_predictions.predict(
                loop_i as usize,
                cache_verifier.predicted_prefix_tokens,
                total_est,
            );
            Some(cache_verifier.predicted_prefix_tokens)
        } else if !cache_verifier.enabled {
            // No explicit markers for this provider at all (e.g. Bedrock's
            // MiniMax) — predict from the shared prefix with the last request
            // instead of leaving cache health unmeasured for every automatic
            // (passive) caching provider. `Some(0)` (prefix fully diverged,
            // e.g. right after compaction) is a real prediction and must
            // still show up as "0 predicted" — only `None` (no previous
            // request to compare against yet, i.e. the very first turn) means
            // no prediction was possible at all.
            //
            // bugs.md: the message list alone undercounts the real cache
            // prefix — the system prompt is a message so that part was
            // already covered, but the *tool schemas* (static + dynamic) are
            // sent as separate `AiRequest` fields, not messages, and were
            // never added to the prediction even though providers place them
            // at the front of the wire prompt (so they're cached identically
            // to a matching message prefix whenever the tool set itself is
            // unchanged from the previous request).
            prev_sent_messages.as_ref().map(|prev| {
                let msg_tokens = common_prefix_tokens_ratio(prev, &messages, chars_per_token);
                let static_tokens = if prev_sent_tools.as_ref() == Some(&static_tools) {
                    estimate_tools_tokens_ratio(&Some(static_tools.clone()), chars_per_token)
                } else {
                    0
                };
                let dynamic_tokens = if prev_sent_dynamic_tools == dynamic_tools_this_turn {
                    estimate_tools_tokens_ratio(&dynamic_tools_this_turn, chars_per_token)
                } else {
                    0
                };
                msg_tokens + static_tokens + dynamic_tokens
            })
        } else {
            None
        };

        let request = AiRequest {
            model: model.clone(),
            messages: messages.clone(),
            stop: None,
            tools: Some(static_tools.clone()),
            dynamic_tools: dynamic_tools_this_turn.clone(),
            cache_breakpoints,
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
        // T12: race the LLM request against the hard-stop token so an
        // in-flight streaming call aborts promptly (dropping the future
        // cancels the underlying HTTP request) instead of waiting for the
        // next tool-call-boundary checkpoint.
        let result = tokio::select! {
            r = provider.complete(&request) => r,
            _ = ctx.cancel.cancelled() => {
                return Err(AgentError::StoppedByUser(total_tool_calls));
            }
        };
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

                // Self-calibrate the chars-per-token ratio from this real
                // response before anything else reads it this turn.
                update_chars_per_token_calibration(ctx, &request, &response.usage);

                // Record stats
                let cost = response.usage.estimate_cost(
                    model.input_cost_per_m,
                    model.output_cost_per_m,
                    model.cached_input_cost_per_m,
                    crate::ai::provider_cache::cache_write_multiplier(&model),
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
                    if let Some(info) = compaction_info.clone() {
                        log.attach_compaction_to_last(info);
                    }
                    if let Some(predicted) = predicted_cache_this_turn {
                        log.set_last_entry_predicted_cache(predicted);
                    }
                    // `persist` was implemented but never actually called
                    // anywhere — every interaction (including real cache
                    // usage stats) lived in memory only and vanished on
                    // restart, with no way to inspect a past session's
                    // request/response history from disk.
                    log.persist(&ctx.project_root);
                }
                {
                    let mut s = ctx.stats.lock().map_err(|e| AgentError::Lock(e.to_string()))?;
                    s.record(&response.usage, &cost);
                }
                // Bug 3: live per-iteration update — same values
                // run_agent_turn_session writes to session.last_prompt_tokens/
                // last_completion_tokens at turn end, just written every
                // iteration instead of once, so a concurrently-polled
                // session_info() reflects an in-flight turn.
                if let Some((live_map, sid)) = &live_context {
                    if let Ok(mut m) = live_map.lock() {
                        m.insert(sid.clone(), (response.usage.input_tokens, response.usage.output_tokens));
                    }
                }
                ctx.event_sink.emit("ai-stats-updated", "");
                last_cached_tokens = response.usage.cached_tokens as usize;
                prev_sent_messages = Some(request.messages.clone());
                prev_sent_tools = request.tools.clone();
                prev_sent_dynamic_tools = request.dynamic_tools.clone();

                // P9b (D9b.3): verify, don't trust — paid writes must produce
                // cached reads; two consecutive misses disable markers.
                if cache_verifier.predicted_prefix_tokens > 0 {
                    cache_predictions
                        .record_actual(loop_i as usize, response.usage.cached_tokens as usize);
                }
                if let Some(warning) = cache_verifier.observe(response.usage.cached_tokens) {
                    if ctx.verbose {
                        eprintln!("[cache-anomaly] {}", warning);
                    }
                    ctx.event_sink.emit(
                        "cache-anomaly",
                        &serde_json::json!({"turn": loop_i, "message": warning}).to_string(),
                    );
                }
                cache_verifier.predicted_prefix_tokens = sent_prefix_tokens;

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

                // bugs.md Bug 1.8: surface reasoning/thinking text in the chat
                // (rendered as a collapsed block by the frontend).
                if let Some(t) = response.thinking.as_ref().filter(|t| !t.trim().is_empty()) {
                    ctx.event_sink.emit(
                        "ai-chat-message",
                        &serde_json::json!({"role": "thinking", "content": t}).to_string(),
                    );
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
                    let dynamic_tools =
                        tool_registry.dynamic_tools.iter().map(|d| d.name.clone()).collect();
                    return Ok(AgentTurnResult {
                        response,
                        tool_calls_executed: all_tool_records,
                        iterations: loop_i + 1,
                        final_messages: messages,
                        compactions: total_compactions,
                        dynamic_tools,
                        last_sent: SentRequestSnapshot {
                            messages: prev_sent_messages.clone().unwrap_or_default(),
                            tools: prev_sent_tools.clone(),
                            dynamic_tools: prev_sent_dynamic_tools.clone(),
                        },
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

                    // T12: hard stop between tool calls of the same batch.
                    if ctx.cancel.is_cancelled() {
                        return Err(AgentError::StoppedByUser(total_tool_calls));
                    }

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

                    // bugs.md Feature 3: discover_tools is handled here (not in
                    // the executor) because it mutates the session's dynamic
                    // tool set — matches are LOADED and become callable on the
                    // next iteration via AiRequest::dynamic_tools.
                    if tool_name == "discover_tools" {
                        let query = arguments
                            .get("query")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let max = arguments
                            .get("max_results")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(10)
                            .clamp(1, 20) as usize;
                        let matches = tool_registry.discover_tools(&query, max);
                        let added = tool_registry.load_dynamic_tools(&matches);
                        let content = if matches.is_empty() {
                            format!(
                                "No additional tools match '{}'. Available additional tools: {}.",
                                query,
                                tool_registry.available_dynamic_names().join(", ")
                            )
                        } else {
                            let lines: Vec<String> = matches
                                .iter()
                                .map(|n| {
                                    format!(
                                        "- {}",
                                        tool_registry.short_help_for(n).unwrap_or(n)
                                    )
                                })
                                .collect();
                            format!(
                                "{} tool(s) now loaded and callable like any other tool:\n{}",
                                added.len().max(matches.len()),
                                lines.join("\n")
                            )
                        };
                        ctx.event_sink.emit(
                            "tool-call",
                            &serde_json::to_string(&ToolCallEvent {
                                tool_name: tool_name.clone(),
                                status: ToolCallStatus::Completed,
                                duration_ms: Some(0),
                                depth: 0,
                                reason: None,
                                call_id: Some(tc.id.clone()),
                                args_preview: Some(crate::preview_str(&tc.function.arguments, 200)),
                                result_preview: Some(crate::preview_str(&content, 200)),
                            })
                            .unwrap_or_default(),
                        );
                        all_tool_records.push(ToolCallRecord {
                            tool_name: tool_name.clone(),
                            success: true,
                            duration_ms: 0,
                        });
                        messages.push(ChatMessage {
                            role: MessageRole::Tool,
                            content,
                            tool_call_id: Some(tc.id.clone()),
                            tool_calls: Vec::new(),
                        });
                        consecutive_failures = 0;
                        continue;
                    }

                    // Keep discovered tools warm while the model uses them.
                    tool_registry.mark_tool_used(tool_name);

                    // bugs.md Feature 4: shell commands can require explicit
                    // per-command user approval (mirrors edit review). The
                    // approval also carries the sandbox network grant (T4).
                    let mut shell_network_granted = false;
                    if tool_name == "run_shell" && ctx.permissions.review_commands {
                        let cmd_str = arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let approval = match &ctx.pause_handler {
                            Some(handler) => handler.approve_command(&cmd_str).await,
                            // headless: nobody to ask
                            None => super::context::CommandApproval {
                                approved: true,
                                allow_network: false,
                            },
                        };
                        shell_network_granted = approval.allow_network;
                        if !approval.approved {
                            let note = format!(
                                "[run_shell rejected by user — command was NOT executed: {}]",
                                cmd_str
                            );
                            ctx.event_sink.emit(
                                "tool-call",
                                &serde_json::to_string(&ToolCallEvent {
                                    tool_name: tool_name.clone(),
                                    status: ToolCallStatus::Failed {
                                        error: "rejected by user".into(),
                                    },
                                    duration_ms: Some(0),
                                    depth: 0,
                                    reason: None,
                                    call_id: Some(tc.id.clone()),
                                    args_preview: Some(crate::preview_str(&tc.function.arguments, 200)),
                                    result_preview: Some(crate::preview_str(&note, 200)),
                                })
                                .unwrap_or_default(),
                            );
                            all_tool_records.push(ToolCallRecord {
                                tool_name: tool_name.clone(),
                                success: false,
                                duration_ms: 0,
                            });
                            messages.push(ChatMessage {
                                role: MessageRole::Tool,
                                content: note,
                                tool_call_id: Some(tc.id.clone()),
                                tool_calls: Vec::new(),
                            });
                            continue;
                        }
                    }

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
                    // Per-command permissions: carries the one-shot network
                    // grant from the approval prompt into the sandbox.
                    let call_permissions = {
                        let mut p = exec_permissions.clone();
                        p.shell_network_once = shell_network_granted;
                        p
                    };
                    // bugs.md Bug 0: if this tool call stages new pending
                    // diffs (review mode covers both edit_file and
                    // run_shell's sandbox-materialized mutations), record the
                    // pre-call count so we can block below until the user
                    // resolves them.
                    let mut review_wait_target: Option<usize> = None;
                    let mut new_diff_ids: Vec<String> = Vec::new();
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
                        // P10: review mode stages edits into pending_diffs
                        // instead of applying them.
                        if ctx.permissions.review_edits {
                            let mut diffs = ctx
                                .pending_diffs
                                .lock()
                                .map_err(|e| AgentError::Lock(e.to_string()))?;
                            let before = diffs.len();
                            let mut sink = ai::tool_executor::ReviewSink {
                                pending: &mut diffs,
                                agent: ctx.permissions.agent_id.clone(),
                            };
                            let result = ai::tool_executor::execute_tool_reviewed(
                                &mcp_call,
                                &ctx.project_root,
                                &mut s,
                                &sym,
                                &g,
                                &call_permissions,
                                &Some(ctx.embed_index.clone()),
                                Some(&mut sink),
                            );
                            if diffs.len() > before {
                                // bugs.md: the chat surfaces staged edits like
                                // approval prompts and opens the first file —
                                // it needs to know *which* files were staged.
                                let new_files: Vec<serde_json::Value> = diffs[before..]
                                    .iter()
                                    .map(|d| {
                                        serde_json::json!({
                                            "file": d.file,
                                            "hunks": d.hunks.len(),
                                            "first_line": d
                                                .hunks
                                                .first()
                                                .map(|h| h.original_start + 1),
                                        })
                                    })
                                    .collect();
                                ctx.event_sink.emit(
                                    "pending-diffs-changed",
                                    &serde_json::json!({
                                        "count": diffs.len(),
                                        "new_files": new_files,
                                    })
                                    .to_string(),
                                );
                                new_diff_ids = diffs[before..].iter().map(|d| d.id.clone()).collect();
                                review_wait_target = Some(before);
                            }
                            result
                        } else {
                            ai::tool_executor::execute_tool_with_index(
                                &mcp_call,
                                &ctx.project_root,
                                &mut s,
                                &sym,
                                &g,
                                &call_permissions,
                                &Some(ctx.embed_index.clone()),
                            )
                        }
                    };
                    let tool_duration = start_tool.elapsed().as_millis() as u64;

                    // sandboxing_better.md T2: sandbox-materialized file
                    // changes just became real undo-tree events — refresh UI.
                    if tool_name == "run_shell" {
                        let n_mutations = tool_result
                            .data
                            .as_ref()
                            .and_then(|d| d.get("fs_mutations"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        if n_mutations > 0 {
                            ctx.event_sink.emit("undo-tree-changed", "");
                            ctx.event_sink.emit("files-changed", "");
                        }
                    }

                    // bugs.md Feature 4: agent shell commands show up in the
                    // bottom terminal panel like user-run commands.
                    if tool_name == "run_shell" {
                        let cmd_str = mcp_call
                            .arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        ctx.event_sink.emit(
                            "agent-shell",
                            &serde_json::json!({
                                "command": cmd_str,
                                "output": crate::preview_str(&tool_result.content, 4000),
                                "success": tool_result.success,
                                "duration_ms": tool_duration,
                            })
                            .to_string(),
                        );
                    }

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

                    // Push tool result message — deferred until after
                    // wait_for_review below when this call staged diffs, so
                    // the model sees the real accept/reject/partial outcome
                    // instead of the "staged for review" placeholder text
                    // (bugs.md Bug 2).
                    if review_wait_target.is_none() {
                        messages.push(ChatMessage {
                            role: MessageRole::Tool,
                            content: tool_result.content.clone(),
                            tool_call_id: Some(tc.id.clone()),
                            tool_calls: Vec::new(),
                        });
                    } else if let Some(ref handler) = ctx.pause_handler {
                        // bugs.md Bug 0 / Bug 2: don't let the agent keep
                        // calling tools (or start a new turn) against a
                        // project state the staged hunks haven't actually
                        // reached yet — block until the user resolves them
                        // (or aborts the run), then report what really
                        // happened.
                        match handler.wait_for_review(&new_diff_ids).await {
                            None => return Err(AgentError::StoppedByUser(total_tool_calls)),
                            Some(outcomes) => {
                                let content = if outcomes.is_empty() {
                                    tool_result.content.clone()
                                } else {
                                    outcomes
                                        .iter()
                                        .map(|o| {
                                            if o.accepted_hunks == o.total_hunks {
                                                o.applied_message.clone()
                                            } else if o.accepted_hunks == 0 {
                                                format!(
                                                    "Edit to '{}' was rejected by the user (at line {}); file unchanged.",
                                                    o.file,
                                                    o.first_line.unwrap_or(1)
                                                )
                                            } else {
                                                format!(
                                                    "Edit to '{}' was partially accepted by the user ({}/{} hunks applied at line {}); file updated with only the accepted changes.",
                                                    o.file,
                                                    o.accepted_hunks,
                                                    o.total_hunks,
                                                    o.first_line.unwrap_or(1)
                                                )
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                };
                                messages.push(ChatMessage {
                                    role: MessageRole::Tool,
                                    content,
                                    tool_call_id: Some(tc.id.clone()),
                                    tool_calls: Vec::new(),
                                });
                            }
                        }
                    } else {
                        // No pause handler configured (headless runs) — no
                        // one to block on, fall back to the placeholder text.
                        messages.push(ChatMessage {
                            role: MessageRole::Tool,
                            content: tool_result.content.clone(),
                            tool_call_id: Some(tc.id.clone()),
                            tool_calls: Vec::new(),
                        });
                    }

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
                    let dynamic_tools =
                        tool_registry.dynamic_tools.iter().map(|d| d.name.clone()).collect();
                    return Ok(AgentTurnResult {
                        response: ai::AiResponse {
                            thinking: None,
                            content: failure_msg,
                            usage: response.usage.clone(),
                            raw_response: response.raw_response.clone(),
                            raw_request: response.raw_request.clone(),
                            truncated: response.truncated,
                            tool_calls: Vec::new(),
                        },
                        tool_calls_executed: all_tool_records,
                        iterations: loop_i + 1,
                        final_messages: messages,
                        compactions: total_compactions,
                        dynamic_tools,
                        last_sent: SentRequestSnapshot {
                            messages: prev_sent_messages.clone().unwrap_or_default(),
                            tools: prev_sent_tools.clone(),
                            dynamic_tools: prev_sent_dynamic_tools.clone(),
                        },
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
                log.persist(&ctx.project_root);
                return Err(AgentError::Provider(e.message));
            }
        }

        // Dynamic-tool retention bookkeeping (Feature 3): unused discovered
        // tools become eligible for removal after the retention window.
        tool_registry.advance_turn();

        loop_i += 1;
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Rough token estimate for one message (content + structured tool calls),
/// using the flat 4-chars/token default. Compaction timing and tests use
/// this fixed form; live marker-planning/prediction call sites use
/// [`estimate_msg_tokens_ratio`] with the session's calibrated ratio instead
/// (bugs.md: the flat default under-predicted real cached tokens by
/// 50-200% on JSON/code-heavy tool traffic).
pub(crate) fn estimate_msg_tokens(m: &ChatMessage) -> usize {
    estimate_msg_tokens_ratio(m, DEFAULT_CHARS_PER_TOKEN)
}

/// Same estimate as [`estimate_msg_tokens`], parameterized on a
/// chars-per-token ratio calibrated from real `usage.input_tokens`.
pub(crate) fn estimate_msg_tokens_ratio(m: &ChatMessage, chars_per_token: f64) -> usize {
    (m.content.len() as f64 / chars_per_token) as usize
        + m.tool_calls
            .iter()
            .map(|tc| (tc.function.arguments.len() as f64 / chars_per_token) as usize + 5)
            .sum::<usize>()
}

/// Bug A fix (new_features_work_plan.md #2): `cached_prefix_tokens` off the
/// wire is an ABSOLUTE count — the provider's whole cached prefix, including
/// the system prompt and (for `SystemPromptEmbed` providers) the tool
/// definitions rendered into it. `batch_prune_decisions`' entry offsets are
/// RELATIVE: `classify_messages` starts counting at 0 right after the system
/// message (`trimmed_rules_table.rs`), and tool schemas are never part of
/// that count at all. Feeding the absolute count straight into
/// `PruneContext::cached_prefix_tokens` compared every entry's relative
/// offset against a baseline it was never measured from, overstating
/// invalidation risk for essentially every prune candidate. Strip the same
/// system+tools baseline that was actually part of the cached request
/// (`prev_sent_*`, what produced `last_cached_tokens`) before handing it to
/// the cost model.
pub(crate) fn relative_cached_prefix_tokens(
    last_cached_tokens: usize,
    prev_sent_messages: Option<&[ChatMessage]>,
    prev_sent_tools: &Option<Vec<crate::ai::provider::ToolSchema>>,
    prev_sent_dynamic_tools: &Option<Vec<crate::ai::provider::ToolSchema>>,
) -> usize {
    let system_tokens = prev_sent_messages
        .and_then(|m| m.first())
        .map(estimate_msg_tokens)
        .unwrap_or(0);
    let tools_tokens = estimate_tools_tokens(prev_sent_tools) + estimate_tools_tokens(prev_sent_dynamic_tools);
    last_cached_tokens.saturating_sub(system_tokens + tools_tokens)
}

/// bugs.md: tool schemas (static + dynamic) are sent as separate `AiRequest`
/// fields, not `ChatMessage`s, but still occupy real prompt tokens and — when
/// unchanged from the previous request — are just as cacheable as a matching
/// message prefix. Left out of `common_prefix_tokens`, the automatic-cache
/// prediction undercounted every turn by the full size of the tool schemas.
pub(crate) fn estimate_tools_tokens(tools: &Option<Vec<crate::ai::provider::ToolSchema>>) -> usize {
    estimate_tools_tokens_ratio(tools, DEFAULT_CHARS_PER_TOKEN)
}

/// Same estimate as [`estimate_tools_tokens`], parameterized on a calibrated
/// chars-per-token ratio — see [`estimate_msg_tokens_ratio`].
pub(crate) fn estimate_tools_tokens_ratio(
    tools: &Option<Vec<crate::ai::provider::ToolSchema>>,
    chars_per_token: f64,
) -> usize {
    tools
        .as_ref()
        .map(|ts| {
            ts.iter()
                .map(|t| {
                    ((t.function.name.len()
                        + t.function.description.len()
                        + t.function.parameters.to_string().len()) as f64
                        / chars_per_token) as usize
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Total characters actually sent this request (system+messages+tool-call
/// args+tool schemas) — the exact same content the estimate helpers above
/// count, so `real_chars / usage.input_tokens` after the response comes
/// back is a faithful, self-correcting chars-per-token calibration for this
/// session's actual content mix.
fn total_request_chars(request: &AiRequest) -> usize {
    let tools_chars = |tools: &Option<Vec<crate::ai::provider::ToolSchema>>| -> usize {
        tools
            .as_ref()
            .map(|ts| {
                ts.iter()
                    .map(|t| {
                        t.function.name.len()
                            + t.function.description.len()
                            + t.function.parameters.to_string().len()
                    })
                    .sum()
            })
            .unwrap_or(0)
    };
    let messages_chars: usize = request
        .messages
        .iter()
        .map(|m| {
            m.content.len()
                + m.tool_calls
                    .iter()
                    .map(|tc| tc.function.arguments.len())
                    .sum::<usize>()
        })
        .sum();
    messages_chars + tools_chars(&request.tools) + tools_chars(&request.dynamic_tools)
}

/// Update the session's calibrated chars-per-token ratio from one real
/// request/response pair. EMA-smoothed (not last-observed-only) so a single
/// unusual turn — e.g. right after compaction rewrites the content mix —
/// can't swing every subsequent estimate; clamped to a sane band so a
/// division-edge-case turn (tiny `usage.input_tokens`) can't send future
/// estimates to an unusable extreme.
pub(crate) fn update_chars_per_token_calibration(
    ctx: &AgentContext,
    request: &AiRequest,
    usage: &crate::ai::tracking::TokenUsage,
) {
    if usage.input_tokens == 0 {
        return;
    }
    let chars = total_request_chars(request) as f64;
    if chars <= 0.0 {
        return;
    }
    let observed = chars / usage.input_tokens as f64;
    if let Ok(mut ratio) = ctx.calibrated_chars_per_token.lock() {
        let smoothed = *ratio * 0.7 + observed * 0.3;
        *ratio = smoothed.clamp(1.5, 6.0);
    }
}

/// bugs.md Bug 3: predicted cached tokens for providers with AUTOMATIC
/// (non-explicit-marker) caching — e.g. Bedrock/MiniMax, which have no
/// `cache_breakpoints` mechanism at all. Automatic caching keys off exactly
/// this: the provider hits cache for however much of the new request's
/// message prefix is byte-identical to what it cached from the previous
/// request; anything from the first divergence onward is a fresh write.
/// Parameterized on a calibrated chars-per-token ratio — see
/// [`estimate_msg_tokens_ratio`]. Tests pass `DEFAULT_CHARS_PER_TOKEN`
/// explicitly; the live caller passes the session's calibrated ratio.
pub(crate) fn common_prefix_tokens_ratio(
    prev: &[ChatMessage],
    current: &[ChatMessage],
    chars_per_token: f64,
) -> usize {
    prev.iter()
        .zip(current.iter())
        .take_while(|(a, b)| a == b)
        .map(|(m, _)| estimate_msg_tokens_ratio(m, chars_per_token))
        .sum()
}

/// P9b (D9b.3) session cache-marker health: markers stay on only while paid
/// writes keep producing cached reads.
struct CacheVerifier {
    /// Whether markers may be emitted on the next request.
    enabled: bool,
    /// Prefix tokens marked on the PREVIOUS request (0 = none sent).
    predicted_prefix_tokens: usize,
    consecutive_misses: u32,
}

impl CacheVerifier {
    fn new(enabled: bool) -> Self {
        Self { enabled, predicted_prefix_tokens: 0, consecutive_misses: 0 }
    }

    /// Feed the cached-token count of the response that followed a marked
    /// request. Returns a warning message the moment markers get disabled
    /// (two consecutive paid-write/no-read turns).
    fn observe(&mut self, actual_cached_tokens: u32) -> Option<String> {
        if self.predicted_prefix_tokens == 0 {
            return None;
        }
        if actual_cached_tokens > 0 {
            self.consecutive_misses = 0;
            return None;
        }
        self.consecutive_misses += 1;
        if self.consecutive_misses >= 2 && self.enabled {
            self.enabled = false;
            return Some(format!(
                "cache markers disabled: paid cache writes on {} turns but got no cached reads",
                self.consecutive_misses
            ));
        }
        None
    }
}

/// P9b (D9b.1): message indexes after which a cache marker pays for itself.
///
/// P(reuse within TTL) is the complement of the tracker's cold-turn ratio,
/// floored at 0.5 mid-conversation (an active loop virtually guarantees a
/// next request). The planner's marker economics then compare expected read
/// savings against the one-time write surcharge.
fn plan_cache_breakpoints(
    cfg: &ProviderCacheConfig,
    messages: &[ChatMessage],
    loop_i: u32,
    cold_turn_ratio: f64,
    static_tools_tokens: usize,
    dynamic_tools_tokens: usize,
    min_cacheable_tokens: usize,
    chars_per_token: f64,
) -> Vec<usize> {
    use crate::ai::ttl_tracking::{CacheBlock, CacheMarkerPlanner};

    if messages.is_empty() {
        return Vec::new();
    }

    // Floored at 0.5: an active agent loop virtually guarantees a next
    // request, so the write is worth it from the first iteration.
    let _ = loop_i;
    let p_reuse = (1.0 - cold_turn_ratio).max(0.5);
    let n_expected = if p_reuse >= 0.5 { 3 } else { 1 };

    let system_tokens = estimate_msg_tokens_ratio(&messages[0], chars_per_token);
    let stable_prefix_tokens: usize = messages[1..messages.len().saturating_sub(1)]
        .iter()
        .map(|m| estimate_msg_tokens_ratio(m, chars_per_token))
        .sum();

    // Bug B fix: tool schemas are real wire tokens (bugs.md), not free. They
    // used to be passed as hardcoded 0s here, which silently collapsed the
    // "after static tools"/"after dynamic tools" candidates onto the "after
    // system prompt" position and undercounted every position downstream of
    // them (including the conversation-prefix candidate), skewing every
    // marker's cost/benefit math on this path.
    let planner = CacheMarkerPlanner::new(cfg.clone(), min_cacheable_tokens);
    let markers = planner.plan_markers(
        system_tokens,
        static_tools_tokens,
        dynamic_tools_tokens,
        0,
        stable_prefix_tokens,
        n_expected,
    );

    // The stable prefix ends before the newest *unstable* addition — not just
    // the newest single message. When an assistant turn fires several
    // parallel tool calls, each tool result lands as its own `ChatMessage`
    // (bedrock.rs merges the trailing run of them into one wire message at
    // send time), and that whole run keeps growing in place across requests
    // until every result is back. Anchoring the breakpoint one message
    // behind the tail (as if only a single message could ever be "new")
    // lands it *inside* that still-growing run on a bursty turn. The next
    // request then extends the run further, mutating the exact wire content
    // the old checkpoint was written against — so the provider can't find a
    // matching prefix for ANY of it and silently pays full price to
    // re-embed and rewrite the whole thing (observed live: a 24.8k-token
    // turn with 0 cached tokens right after a turn that had built up to
    // 9.7k cached, ~43% of that session's entire cost). Skipping back past
    // the *entire* trailing run of Tool messages keeps the breakpoint a
    // strict forward extension of whatever was cached last time, at the
    // cost of leaving the newest tool-call batch itself uncached until the
    // turn after it lands.
    let newest_batch_start = {
        let mut i = messages.len();
        while i > 0 && messages[i - 1].role == MessageRole::Tool {
            i -= 1;
        }
        if i == messages.len() {
            messages.len().saturating_sub(1)
        } else {
            i
        }
    };

    let mut idxs: Vec<usize> = markers
        .iter()
        .filter_map(|m| match m.after_block {
            CacheBlock::SystemPrompt => Some(0usize),
            CacheBlock::ConversationPrefix { .. } => Some(newest_batch_start.saturating_sub(1)),
            CacheBlock::AfterMessage { index } => Some(index),
            // Tool schemas are a separate `AiRequest` field, not a
            // `ChatMessage` — there is no message index that sits literally
            // "between the tools and the system prompt". But neither
            // implemented wire format (Bedrock Converse, OpenRouter) offers a
            // way to mark the tools array independently either: both render
            // tools *before* system/messages and only support inserting a
            // cache boundary inside the system block or a message, at which
            // point it covers everything rendered before it — tools
            // included. So "after static/dynamic tools" and "after system
            // prompt" are the *same* wire position for every provider this
            // planner currently drives; mapping both onto message index 0
            // is not an approximation, it's the actual insertion point.
            // Previously this arm dropped the marker entirely, which
            // silently produced zero cache markers on the very first
            // request of a session whenever tool schemas made the
            // system+tools total clear a model's floor while the system
            // prompt text alone did not (bugs.md: live trace showed 1511
            // combined tokens over Sonnet 4.6's 1024 floor, system alone
            // ~347 under it — turn 1 got no marker at all).
            CacheBlock::StaticTools | CacheBlock::DynamicTools => Some(0usize),
        })
        .filter(|&i| i < messages.len())
        .collect();
    idxs.sort_unstable();
    idxs.dedup();
    idxs
}

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

/// Derive the ProviderCacheConfig the cost model uses for this model —
/// model pricing first, then provider_cache.json, then a conservative default
/// (never "prune is free"; see trimmed_summarization_problem.md, problem 1).
fn cache_config_from_model(model: &ai::ModelConfig) -> ProviderCacheConfig {
    ai::provider_cache::resolve_cache_config(model).0
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
    cached_prefix_tokens_hint: usize,
) -> Option<ai::log::CompactionInfo> {
    // bugs.md Bug 2: an explicit, user-facing settings toggle to fully
    // disable trimming/summarization (for isolating caching problems with a
    // stable context) — unlike a guessed code-level threshold, this is an
    // inspectable on/off switch the user controls directly.
    if ctx.settings.lock().map(|s| s.disable_context_trimming).unwrap_or(false) {
        return None;
    }

    let messages_before = messages.len();
    // Estimate tokens for the prune decision (trivial math, always run).
    let total_tokens_est: usize = messages.iter().map(|m| {
        let content_tokens = m.content.len() / 4;
        let tc_tokens: usize = m.tool_calls.iter().map(|tc| tc.function.arguments.len() / 4 + 5).sum();
        content_tokens + tc_tokens
    }).sum();
    let message_overhead = messages.len() * 4;

    // The decision runs every turn (no "context is still small, skip this
    // pass" gate here — that kind of shortcut has to be a rule in the rules
    // table, inspectable and applied per-message, not a code-level trick
    // that skips the whole cost model based on a guessed threshold). What
    // actually protects small/recent conversations is the rules table's own
    // recent-turns-protected window (`trimmed_rules_table` /
    // `RetentionEngine::eligible_for_pruning`): nothing is even a candidate
    // until it ages out of it, so a two-turn conversation naturally has an
    // empty eligible set and this function is a no-op below.

    let cache_cfg = cache_config_from_model(model);

    // Get summary model (if configured) for pricing
    let summary_model_cfg = ctx.settings
        .lock()
        .ok()
        .and_then(|s| s.summary_model.clone());

    let mut prune_ctx = PruneContext::from_models(
        cache_cfg,
        model,
        summary_model_cfg.as_ref(),
    );
    // bugs.md Bug 1: N (expected remaining rounds) is the tuning knob for how
    // aggressively context is trimmed/summarized. It comes from settings
    // (default 8) — the old 1/4-context force-summarize escape hatch is gone.
    prune_ctx.n_expected = ctx
        .settings
        .lock()
        .ok()
        .map(|s| s.n_expected_rounds)
        .unwrap_or(8)
        .max(1);
    // trimmed_summary_auto_decision.md item 2: feed the cut-point model the
    // actual warm-prefix size (from the previous response's usage) instead of
    // the constant 0 that made the invalidation penalty always vanish.
    prune_ctx.cached_prefix_tokens = cached_prefix_tokens_hint;
    prune_ctx.time_since_last_request_secs = ctx
        .timing_tracker
        .lock()
        .map(|t| t.last_idle_secs())
        .unwrap_or(0);

    // Classify every message against the rules table (trimmed_rules_table:
    // type-based Keep/Eligible + turn assignment), then let the cost model
    // decide whether/where to cut among what the table allowed.
    let (to_prune, to_summarize, tokens_freed, mut details, entry_to_msg_idx) = {
        let mut engine = match ctx.retention_engine.lock() {
            Ok(e) => e,
            Err(_) => return None, // can't lock → skip compaction
        };

        // Rebuild the view from the live message list on every call. The
        // engine is recreated each chat turn and compaction drains `messages`,
        // so any incremental count-based mapping between engine entries and
        // messages desyncs after the first drain (bugs.md Bug 5: compaction
        // fired once and never again).
        let classified = ai::trimmed_rules_table::classify_messages(messages);
        let entry_to_msg_idx = classified.entry_to_msg_idx;
        engine.user_view.entries = classified.entries;
        engine.current_turn = classified.current_turn;

        // Get prune decisions (D0 fix: a single cut-point scan, not a sum of
        // independent per-message votes — see cost_trimmed_summary_model's
        // batch_prune_decisions).
        let all_entries: Vec<&crate::ai::retention::RetentionEntry> = engine.user_view.entries.iter().collect();
        let eligible = engine.eligible_for_pruning();
        let (prune_ids, _logs) = batch_prune_decisions(&all_entries, &eligible, &prune_ctx, engine.current_turn);

        // Feature 1.1: record which messages were CANDIDATES for trimming but
        // survived the cost math — the log shows them with a distinct marker.
        let mut details: Vec<ai::log::CompactionMessageDetail> = eligible
            .iter()
            .filter(|e| !prune_ids.contains(&e.id))
            .map(|e| ai::log::CompactionMessageDetail {
                role: format!("{:?}", e.kind),
                preview: crate::preview_str(&e.content, 160),
                tokens: e.approx_tokens,
                action: "trim_candidate".to_string(),
            })
            .collect();

        // Get summarization decision (pure cost math — the old 1/4-context
        // force-summarize hatch was removed per bugs.md Bug 1; tune
        // n_expected_rounds in settings instead).
        let summarizable = engine.summarizable_entries();
        let sum_decision = if summarizable.is_empty() {
            CostDecision::Keep { reason: "nothing to summarize".into() }
        } else {
            summarization_decision(&summarizable, &prune_ctx)
        };

        // Collect content for summarization before pruning
        let summarize_content: Vec<String> = if sum_decision.is_summarize() {
            details.extend(summarizable.iter().map(|e| ai::log::CompactionMessageDetail {
                role: format!("{:?}", e.kind),
                preview: crate::preview_str(&e.content, 160),
                tokens: e.approx_tokens,
                action: "summarized".to_string(),
            }));
            summarizable.iter().map(|e| e.content.clone()).collect()
        } else {
            Vec::new()
        };

        let freed: usize = eligible.iter()
            .filter(|e| prune_ids.contains(&e.id))
            .map(|e| e.approx_tokens)
            .sum();

        // Apply prune
        engine.prune_entries(&prune_ids);

        (prune_ids, summarize_content, freed, details, entry_to_msg_idx)
    };

    // If there are entries to summarize, call the LLM
    let mut summarized = false;
    if !to_summarize.is_empty() {
        let summary = call_summary_llm(ctx, model, provider, &to_summarize).await;
        if let Some(summary_text) = summary {
            summarized = true;
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
    }
    // Summary skipped or failed → fall back to pruning so an oversized
    // context still shrinks instead of being carried to the next request.
    //
    // trimmed_summary_auto_decision.md item 3 (fixes D3): actuate exactly
    // what the cut-point decision selected — map `to_prune` (retention-engine
    // entry ids) through `entry_to_msg_idx` to the real `messages` indices
    // and remove precisely those, instead of a positional keep_head/keep_tail
    // heuristic that was decoupled from the cost model's own output. The
    // selected set is not necessarily contiguous (kept user messages and
    // recent-turn-protected entries can sit between pruned ones), which is
    // exactly the shape the cut-point model can produce.
    if !summarized && !to_prune.is_empty() {
        let mut prune_indices: Vec<usize> = to_prune
            .iter()
            .filter_map(|id| entry_to_msg_idx.get(*id as usize).copied())
            .collect();
        prune_indices.sort_unstable();
        prune_indices.dedup();

        if !prune_indices.is_empty() {
            for &i in &prune_indices {
                let m = &messages[i];
                details.push(ai::log::CompactionMessageDetail {
                    role: format!("{:?}", m.role).to_lowercase(),
                    preview: crate::preview_str(&m.content, 160),
                    tokens: estimate_msg_tokens(m),
                    action: "trimmed".to_string(),
                });
            }
            // No note is inserted: a pruned message simply disappears from
            // context, same as if it had never been sent. A "[Context
            // compacted...]" marker would itself be a message occupying
            // context/cache space for something that carries no information
            // the model needs.
            let prune_set: std::collections::HashSet<usize> =
                prune_indices.iter().copied().collect();
            let mut i = 0;
            messages.retain(|_| {
                let keep = !prune_set.contains(&i);
                i += 1;
                keep
            });
        }
    }

    sanitize_tool_pairing(messages);

    // Report what happened for the interaction log (bugs.md: log icons).
    let tokens_after: usize = messages.iter().map(estimate_msg_tokens).sum();
    let trimmed = messages.len() < messages_before;
    // bugs.md Bug 5: a turn with nothing pruned/summarized can still have
    // trim_candidate details (eligible entries the cost math chose to keep)
    // — those are worth surfacing in the log too, not just discarded here.
    if !summarized && !trimmed && tokens_freed == 0 && details.is_empty() {
        return None;
    }
    Some(ai::log::CompactionInfo {
        kind: if summarized {
            "summarized"
        } else if trimmed {
            "trimmed"
        } else {
            "candidates"
        }
        .to_string(),
        // Per-message count of what was actually dropped from context. The net
        // length delta undercounts: pruning re-inserts kept user messages, so
        // "trimmed 0 messages" showed up while tokens changed.
        messages_removed: details
            .iter()
            .filter(|d| d.action == "trimmed" || d.action == "summarized")
            .count(),
        tokens_before: total_tokens_est + message_overhead,
        tokens_after,
        details,
    })
}

/// bugs.md Feature 2: user-triggered summarization. Summarizes everything but
/// the system prompt and the last few messages, regardless of cost math —
/// the user pressed the button, they want a lean context.
pub async fn force_summarize(
    ctx: &AgentContext,
    messages: &mut Vec<ChatMessage>,
) -> Result<ai::log::CompactionInfo, String> {
    let (model, provider) = build_provider(ctx).map_err(|e| e.to_string())?;
    let tokens_before: usize = messages.iter().map(estimate_msg_tokens).sum();

    let start = if messages.first().map(|m| m.role == MessageRole::System).unwrap_or(false) {
        1
    } else {
        0
    };
    let keep_tail = 4.min(messages.len().saturating_sub(start));
    let cut_end = messages.len() - keep_tail;
    if cut_end <= start + 1 {
        return Err("nothing to summarize — context is already small".into());
    }

    let contents: Vec<String> = messages[start..cut_end]
        .iter()
        .map(|m| {
            let mut s = format!("{:?}: {}", m.role, m.content);
            for tc in &m.tool_calls {
                s.push_str(&format!("\n[called {}({})]", tc.function.name, tc.function.arguments));
            }
            s
        })
        .collect();

    // Feature 1.1: record what went into the summary for the Log tab.
    let details: Vec<ai::log::CompactionMessageDetail> = messages[start..cut_end]
        .iter()
        .map(|m| ai::log::CompactionMessageDetail {
            role: format!("{:?}", m.role).to_lowercase(),
            preview: crate::preview_str(&m.content, 160),
            tokens: estimate_msg_tokens(m),
            action: "summarized".to_string(),
        })
        .collect();

    let summary = call_summary_llm(ctx, &model, provider.as_ref(), &contents)
        .await
        .ok_or("summarization call failed")?;

    messages.drain(start..cut_end);
    messages.insert(start, ChatMessage {
        role: MessageRole::User,
        content: format!("[Context Summary]\n{}", summary),
        tool_call_id: None,
        tool_calls: Vec::new(),
    });
    sanitize_tool_pairing(messages);

    let tokens_after: usize = messages.iter().map(estimate_msg_tokens).sum();
    Ok(ai::log::CompactionInfo {
        kind: "summarized".to_string(),
        // Count the messages that actually went into the summary, not the net
        // length delta (the inserted summary message masks one removal).
        messages_removed: details.iter().filter(|d| d.action == "summarized").count(),
        tokens_before,
        tokens_after,
        details,
    })
}

/// Repair the native tool-call pairing invariant after compaction mutates the
/// message list. Providers hard-reject histories where a `role: tool` message
/// has no preceding assistant message carrying the matching `tool_calls`
/// entry, or where an assistant `tool_calls` entry has no result. Index-based
/// pruning can produce both, so: unanswered assistant calls are flattened to
/// text, and orphaned tool results become user messages.
fn sanitize_tool_pairing(messages: &mut Vec<ChatMessage>) {
    use std::collections::HashSet;

    // Pass 1: assistant tool_calls whose results were pruned → flatten the
    // calls into text content. (Runs first: flattening orphans the surviving
    // results of that message, which pass 2 then converts.)
    let n = messages.len();
    for i in 0..n {
        if messages[i].role != MessageRole::Assistant || messages[i].tool_calls.is_empty() {
            continue;
        }
        let mut answered: HashSet<String> = HashSet::new();
        let mut j = i + 1;
        while j < n && messages[j].role == MessageRole::Tool {
            if let Some(id) = &messages[j].tool_call_id {
                answered.insert(id.clone());
            }
            j += 1;
        }
        if messages[i].tool_calls.iter().any(|tc| !answered.contains(&tc.id)) {
            let rendered: Vec<String> = messages[i]
                .tool_calls
                .iter()
                .map(|tc| format!("[called {}({})]", tc.function.name, tc.function.arguments))
                .collect();
            let msg = &mut messages[i];
            if !msg.content.is_empty() {
                msg.content.push('\n');
            }
            msg.content.push_str(&rendered.join("\n"));
            msg.tool_calls.clear();
        }
    }

    // Pass 2: tool results with no pending call from the closest preceding
    // assistant message → user messages (content is kept, role becomes valid).
    let mut pending: HashSet<String> = HashSet::new();
    for msg in messages.iter_mut() {
        match msg.role {
            MessageRole::Assistant => {
                pending = msg.tool_calls.iter().map(|tc| tc.id.clone()).collect();
            }
            MessageRole::Tool => {
                let ok = msg
                    .tool_call_id
                    .as_ref()
                    .map(|id| pending.remove(id))
                    .unwrap_or(false);
                if !ok {
                    msg.role = MessageRole::User;
                    msg.content = format!("[earlier tool result]\n{}", msg.content);
                    msg.tool_call_id = None;
                }
            }
            _ => pending.clear(),
        }
    }
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
        dynamic_tools: None,
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
        cache_breakpoints: Vec::new(),
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
                crate::ai::provider_cache::cache_write_multiplier(&request.model),
            );
            if let Ok(mut s) = ctx.stats.lock() {
                s.record(&response.usage, &cost);
            }
            if let Ok(mut log) = ctx.log.lock() {
                log.record_success("summarization", &request, &response, 0);
                log.persist(&ctx.project_root);
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

#[cfg(test)]
mod pairing_tests {
    use super::*;
    use crate::ai::provider::{ToolCallFunction, ToolCallResponse};

    fn call(id: &str, name: &str) -> ToolCallResponse {
        ToolCallResponse {
            id: id.into(),
            call_type: "function".into(),
            function: ToolCallFunction { name: name.into(), arguments: "{}".into() },
        }
    }

    fn assistant(calls: Vec<ToolCallResponse>) -> ChatMessage {
        ChatMessage { role: MessageRole::Assistant, content: String::new(), tool_call_id: None, tool_calls: calls }
    }

    fn tool(id: &str, content: &str) -> ChatMessage {
        ChatMessage { role: MessageRole::Tool, content: content.into(), tool_call_id: Some(id.into()), tool_calls: Vec::new() }
    }

    fn user(content: &str) -> ChatMessage {
        ChatMessage { role: MessageRole::User, content: content.into(), tool_call_id: None, tool_calls: Vec::new() }
    }

    #[test]
    fn orphan_tool_results_become_user_messages() {
        // The compaction shape that 400s on Bedrock: prune note followed by
        // tool results whose assistant message was drained.
        let mut msgs = vec![
            user("prompt"),
            user("[Context compacted]"),
            tool("call_1", "result one"),
            tool("call_2", "result two"),
            assistant(vec![call("call_3", "read_file")]),
            tool("call_3", "file body"),
        ];
        sanitize_tool_pairing(&mut msgs);
        assert_eq!(msgs[2].role, MessageRole::User);
        assert!(msgs[2].content.contains("result one"));
        assert_eq!(msgs[3].role, MessageRole::User);
        // Intact pair untouched
        assert_eq!(msgs[4].tool_calls.len(), 1);
        assert_eq!(msgs[5].role, MessageRole::Tool);
    }

    #[test]
    fn unanswered_assistant_calls_are_flattened() {
        // Assistant made two calls but one result was pruned: calls flatten to
        // text, the surviving result becomes a user message.
        let mut msgs = vec![
            user("prompt"),
            assistant(vec![call("call_1", "read_file"), call("call_2", "run_shell")]),
            tool("call_2", "shell output"),
        ];
        sanitize_tool_pairing(&mut msgs);
        assert!(msgs[1].tool_calls.is_empty());
        assert!(msgs[1].content.contains("read_file"));
        assert!(msgs[1].content.contains("run_shell"));
        assert_eq!(msgs[2].role, MessageRole::User);
        assert!(msgs[2].content.contains("shell output"));
    }

    #[test]
    fn intact_history_is_unchanged() {
        let mut msgs = vec![
            user("prompt"),
            assistant(vec![call("call_1", "read_file")]),
            tool("call_1", "body"),
            assistant(Vec::new()),
        ];
        let before = format!("{:?}", msgs);
        sanitize_tool_pairing(&mut msgs);
        assert_eq!(before, format!("{:?}", msgs));
    }

    #[test]
    fn user_message_between_pair_orphans_result() {
        // A message inserted between an assistant call and its result breaks
        // adjacency; the result must be demoted, and the now-unanswered call
        // flattened.
        let mut msgs = vec![
            assistant(vec![call("call_1", "find_semantic")]),
            user("[Context compacted]"),
            tool("call_1", "match list"),
        ];
        sanitize_tool_pairing(&mut msgs);
        assert!(msgs[0].tool_calls.is_empty());
        assert_eq!(msgs[2].role, MessageRole::User);
    }
}

#[cfg(test)]
mod common_prefix_tests {
    use super::*;

    fn msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage { role, content: content.to_string(), tool_call_id: None, tool_calls: Vec::new() }
    }

    #[test]
    fn full_overlap_when_only_a_message_was_appended() {
        let prev = vec![msg(MessageRole::System, "sys prompt"), msg(MessageRole::User, "hello there")];
        let mut current = prev.clone();
        current.push(msg(MessageRole::Assistant, "hi, how can I help"));
        let expected: usize = prev.iter().map(estimate_msg_tokens).sum();
        assert_eq!(common_prefix_tokens_ratio(&prev, &current, 4.0), expected);
    }

    #[test]
    fn zero_when_first_message_changed() {
        // bugs.md: a real prediction of 0 (prefix fully invalidated, e.g.
        // right after compaction) must be distinguishable from "no
        // prediction was made" — this returns Some(0) upstream, not None.
        let prev = vec![msg(MessageRole::System, "sys prompt v1")];
        let current = vec![msg(MessageRole::System, "sys prompt v2")];
        assert_eq!(common_prefix_tokens_ratio(&prev, &current, 4.0), 0);
    }

    #[test]
    fn partial_overlap_stops_at_first_divergence() {
        let prev = vec![
            msg(MessageRole::System, "sys"),
            msg(MessageRole::User, "question one"),
            msg(MessageRole::Assistant, "answer one"),
        ];
        let mut current = prev.clone();
        current[2] = msg(MessageRole::Assistant, "a DIFFERENT answer one");
        current.push(msg(MessageRole::User, "question two"));
        let expected: usize = prev[..2].iter().map(estimate_msg_tokens).sum();
        assert_eq!(common_prefix_tokens_ratio(&prev, &current, 4.0), expected);
    }
}

#[cfg(test)]
mod cache_marker_tests {
    use super::*;
    use crate::ai::provider_cache::CacheMode;

    fn anthropic_cfg() -> ProviderCacheConfig {
        ProviderCacheConfig {
            cache_mode: CacheMode::Explicit,
            cache_read_discount: 0.1,
            cache_write_multiplier: 1.25,
            ttl_seconds: Some(300),
            requires_markers: true,
            notes: None,
        }
    }

    fn msg(role: MessageRole, len: usize) -> ChatMessage {
        ChatMessage {
            role,
            content: "x".repeat(len),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    #[test]
    fn markers_planned_from_first_request() {
        // Big system prompt + first user message: system-prompt marker pays
        // for itself even with no timing history (cold_ratio 1.0 → floor 0.5).
        let messages = vec![msg(MessageRole::System, 8000), msg(MessageRole::User, 200)];
        let idxs = plan_cache_breakpoints(&anthropic_cfg(), &messages, 0, 1.0, 0, 0, 0, 4.0);
        assert_eq!(idxs, vec![0], "system prompt should carry a marker");
    }

    /// Live bug: a system prompt alone (~347 est. tokens) sat under Sonnet
    /// 4.6's 1024-token floor, but system+tools combined (~1511) cleared it
    /// comfortably — yet the very first request of the session got zero
    /// cache markers. The planner's own economics correctly identified
    /// "after tools" (pos2, clearing the floor) as the winning candidate,
    /// but the message-index mapping used to drop `StaticTools`/
    /// `DynamicTools` candidates entirely since there's no `ChatMessage`
    /// index "between tools and system". Both now map onto message index 0,
    /// the system block's only real insertion point.
    #[test]
    fn small_system_prompt_still_gets_a_marker_when_tools_push_it_over_the_floor() {
        let messages = vec![msg(MessageRole::System, 1390), msg(MessageRole::User, 75)];
        // system alone ~= 1390/4 = 347 tokens: under a 1024 floor.
        let static_tools_tokens = 1164; // matches the live trace's tool-schema estimate
        // combined ~= 347 + 1164 = 1511: clears a 1024 floor.
        let idxs = plan_cache_breakpoints(&anthropic_cfg(), &messages, 0, 1.0, static_tools_tokens, 0, 1024, 4.0);
        assert_eq!(
            idxs,
            vec![0],
            "system+tools clearing the floor should still place a marker at the only available insertion point"
        );
    }

    #[test]
    fn deep_conversation_marks_stable_prefix() {
        let mut messages = vec![msg(MessageRole::System, 8000)];
        for _ in 0..6 {
            messages.push(msg(MessageRole::User, 2000));
            messages.push(msg(MessageRole::Assistant, 2000));
        }
        let idxs = plan_cache_breakpoints(&anthropic_cfg(), &messages, 3, 0.0, 0, 0, 0, 4.0);
        assert!(idxs.contains(&0), "system marker expected");
        assert!(
            idxs.contains(&(messages.len() - 2)),
            "stable conversation prefix marker expected, got {:?}",
            idxs
        );
        assert!(idxs.len() <= 4, "Anthropic allows max 4 breakpoints");
    }

    /// End-to-end: a model floor high enough to exclude the small system
    /// prompt (e.g. Haiku 4.5's 4096) suppresses the marker entirely, while
    /// the same request under a lower floor (e.g. Sonnet 4.6's 1024) still
    /// gets one — matching what was observed live between the two models on
    /// the exact same agent conversation.
    #[test]
    fn model_floor_suppresses_markers_the_generic_economics_would_otherwise_place() {
        let messages = vec![msg(MessageRole::System, 8000), msg(MessageRole::User, 200)];
        // system prompt ≈ 8000/4 = 2000 tokens: below a 4096 floor, above a
        // 1024 one.
        let idxs_high_floor = plan_cache_breakpoints(&anthropic_cfg(), &messages, 0, 1.0, 0, 0, 4096, 4.0);
        assert!(idxs_high_floor.is_empty(), "2000 tokens must not clear a 4096 floor: {:?}", idxs_high_floor);

        let idxs_low_floor = plan_cache_breakpoints(&anthropic_cfg(), &messages, 0, 1.0, 0, 0, 1024, 4.0);
        assert_eq!(idxs_low_floor, vec![0], "2000 tokens should clear a 1024 floor");
    }

    #[test]
    fn relative_cached_prefix_strips_system_and_tools_baseline() {
        // Bug A: 1000 absolute cached tokens where 300 of them are the system
        // message and 200 are static+dynamic tool schemas should leave 500
        // tokens of actual conversation-body prefix cached — the frame
        // `batch_prune_decisions`' relative entry offsets are measured in.
        let prev_messages = vec![msg(MessageRole::System, 1200)]; // ~300 tokens (len/4)
        let tools = vec![crate::ai::provider::ToolSchema {
            tool_type: "function".into(),
            function: crate::ai::provider::ToolFunction {
                name: "x".repeat(100),
                description: "y".repeat(300),
                parameters: serde_json::json!({}),
            },
        }];
        let static_tokens = estimate_tools_tokens(&Some(tools.clone()));
        let got = relative_cached_prefix_tokens(1000, Some(&prev_messages), &Some(tools), &None);
        assert_eq!(got, 1000usize.saturating_sub(300 + static_tokens));
    }

    #[test]
    fn relative_cached_prefix_saturates_at_zero_when_baseline_exceeds_cache() {
        // If the whole absolute cache count is smaller than the system+tools
        // baseline (e.g. right after the tool set changed), the conversation
        // body has nothing cached — never underflow to a huge usize.
        let prev_messages = vec![msg(MessageRole::System, 8000)];
        let got = relative_cached_prefix_tokens(10, Some(&prev_messages), &None, &None);
        assert_eq!(got, 0);
    }

    #[test]
    fn relative_cached_prefix_with_no_prior_request_is_zero_baseline() {
        let got = relative_cached_prefix_tokens(500, None, &None, &None);
        assert_eq!(got, 500);
    }

    /// Live bug: a burst of parallel tool calls appends several `Tool`-role
    /// messages in one turn. The old `messages.len() - 2` anchor landed the
    /// breakpoint *inside* that still-growing batch, so the very next request
    /// (which finishes appending the batch) no longer matched the cached
    /// prefix at all — a full-price rewrite that discarded everything cached
    /// so far. The breakpoint must land before the whole trailing Tool run,
    /// not just before its last message.
    #[test]
    fn conversation_prefix_marker_skips_the_entire_trailing_tool_batch() {
        let messages = vec![
            msg(MessageRole::System, 8000),
            msg(MessageRole::User, 2000),
            msg(MessageRole::Assistant, 2000), // requests 3 parallel tool calls
            msg(MessageRole::Tool, 2000),
            msg(MessageRole::Tool, 2000),
            msg(MessageRole::Tool, 2000),
        ];
        let idxs = plan_cache_breakpoints(&anthropic_cfg(), &messages, 3, 0.0, 0, 0, 0, 4.0);
        // Before the fix this would be messages.len() - 2 == 4, landing
        // between the 1st and 2nd tool result — inside the batch.
        let tool_batch_start = 3;
        assert!(
            idxs.iter().all(|&i| i < tool_batch_start),
            "conversation-prefix marker must land before the whole trailing \
             tool-result batch, got {:?}",
            idxs
        );
    }

    #[test]
    fn automatic_provider_gets_no_markers() {
        let cfg = ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.5,
            cache_write_multiplier: 0.0,
            ttl_seconds: None,
            requires_markers: false,
            notes: None,
        };
        let messages = vec![msg(MessageRole::System, 8000), msg(MessageRole::User, 200)];
        assert!(plan_cache_breakpoints(&cfg, &messages, 2, 0.0, 0, 0, 0, 4.0).is_empty());
    }

    #[test]
    fn verifier_disables_after_two_consecutive_misses() {
        let mut v = CacheVerifier::new(true);
        // Nothing was marked yet → a 0-cached response is not an anomaly.
        assert!(v.observe(0).is_none());
        v.predicted_prefix_tokens = 5000;
        assert!(v.observe(0).is_none(), "first miss tolerated");
        assert!(v.enabled);
        let warning = v.observe(0);
        assert!(warning.is_some(), "second consecutive miss disables markers");
        assert!(!v.enabled);
        // Once disabled it stays disabled and stays quiet.
        assert!(v.observe(0).is_none());
        assert!(!v.enabled);
    }

    #[test]
    fn verifier_reset_on_cache_hit() {
        let mut v = CacheVerifier::new(true);
        v.predicted_prefix_tokens = 5000;
        assert!(v.observe(0).is_none());
        assert!(v.observe(4800).is_none(), "hit resets the miss streak");
        assert!(v.observe(0).is_none(), "streak restarts at one");
        assert!(v.enabled);
    }

    #[test]
    fn provider_cache_key_maps_known_providers() {
        use crate::ai::provider_cache::provider_cache_key;
        let mut m = crate::ai::ModelConfig::default();
        m.model_id = "anthropic/claude-sonnet-4".into();
        assert_eq!(provider_cache_key(&m), Some("anthropic_5min"));
        // MiniMax has a passive automatic cache (80% read discount) — mapping
        // it means the cost model stops pricing prefix trims as free.
        m.model_id = "MiniMax-M2".into();
        assert_eq!(provider_cache_key(&m), Some("minimax_passive"));
        m.model_id = "minimax.minimax-m2.5".into();
        assert_eq!(provider_cache_key(&m), Some("minimax_passive"));
        m.model_id = "some-unknown-model".into();
        assert_eq!(provider_cache_key(&m), None);
    }
}

#[cfg(test)]
mod compaction_tests {
    use super::*;
    use crate::ai::mock::MockProvider;

    fn history(pairs: usize) -> Vec<ChatMessage> {
        let mut msgs = vec![
            ChatMessage {
                role: MessageRole::System,
                content: "system prompt".repeat(20),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
            ChatMessage {
                role: MessageRole::User,
                content: "please fix the bug".into(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            },
        ];
        for i in 0..pairs {
            msgs.push(ChatMessage {
                role: MessageRole::Assistant,
                content: format!("looking at file {i} ").repeat(20),
                tool_call_id: None,
                tool_calls: Vec::new(),
            });
            msgs.push(ChatMessage {
                role: MessageRole::Tool,
                content: format!("tool output {i} ").repeat(120),
                tool_call_id: Some(format!("call_{i}")),
                tool_calls: Vec::new(),
            });
        }
        msgs
    }

    /// bugs.md Bug 5: a long accumulated history must compact on the very
    /// first request of a turn (there is no loop_i gate inside the function,
    /// and eligibility is rebuilt from the live message list).
    #[tokio::test]
    async fn long_history_compacts_on_first_call() {
        // Zero-cost ModelConfig makes should_summarize pick "Summarize"; without
        // auto-answers the mock summary call blocks on its response channel for
        // up to 10 minutes. Same pattern as tui/tests/smoke.rs.
        std::env::set_var("TRACELEAN_MOCK_AUTO", "mock summary");
        let ctx = AgentContext::from_env(std::env::temp_dir());
        let model = ai::ModelConfig::default(); // no caching → prune always free
        let (provider, _rx) = MockProvider::new();

        let mut messages = history(12);
        let before = messages.len();
        let info = cost_aware_compact(&ctx, &mut messages, &model, &provider, 0).await;

        assert!(info.is_some(), "12 old tool-loop pairs must trigger compaction");
        assert!(messages.len() < before, "messages must shrink ({before} → {})", messages.len());
        assert_eq!(messages[0].role, MessageRole::System, "system prompt survives");
        // Which branch fires depends on the summary call: with the mock
        // auto-answering, summarization succeeds and leaves "[Context Summary]";
        // if it ever fails, the prune fallback leaves "[Context compacted".
        // Either way a marker must record that compaction happened.
        assert!(
            messages.iter().any(|m| m.content.contains("[Context Summary]")
                || m.content.contains("[Context compacted")),
            "compaction marker inserted"
        );
    }

    /// bugs.md Bug 5 regression: compaction must keep firing on repeated
    /// calls as the conversation grows again (the old count-based entry
    /// registration desynced after the first drain and never fired again).
    #[tokio::test]
    async fn compaction_fires_again_after_regrowth() {
        std::env::set_var("TRACELEAN_MOCK_AUTO", "mock summary");
        let ctx = AgentContext::from_env(std::env::temp_dir());
        let model = ai::ModelConfig::default();
        let (provider, _rx) = MockProvider::new();

        let mut messages = history(12);
        let first = cost_aware_compact(&ctx, &mut messages, &model, &provider, 0).await;
        assert!(first.is_some());

        // Conversation grows again: another 12 tool-loop pairs on top.
        let regrown = history(12);
        messages.extend(regrown.into_iter().skip(2)); // skip its system+user
        let before = messages.len();
        let second = cost_aware_compact(&ctx, &mut messages, &model, &provider, 0).await;

        assert!(second.is_some(), "compaction must fire again after regrowth");
        assert!(messages.len() < before);
    }

    /// A short recent conversation must NOT be compacted.
    #[tokio::test]
    async fn short_history_left_alone() {
        let ctx = AgentContext::from_env(std::env::temp_dir());
        let model = ai::ModelConfig::default();
        let (provider, _rx) = MockProvider::new();

        let mut messages = history(2);
        let before = messages.len();
        let info = cost_aware_compact(&ctx, &mut messages, &model, &provider, 0).await;

        assert!(info.is_none(), "recent turns are protected");
        assert_eq!(messages.len(), before);
    }

    /// bugs.md Bug 5: a turn where entries are eligible (aged out of the
    /// protected window) but the cost math keeps all of them — because a
    /// huge assumed cached prefix makes every cut unprofitable, and a low
    /// n_expected makes summarization unprofitable too — must still surface
    /// those as "candidates" in the log instead of being discarded as `None`.
    #[tokio::test]
    async fn eligible_but_kept_reports_candidates_not_none() {
        let ctx = AgentContext::from_env(std::env::temp_dir());
        {
            let mut settings = ctx.settings.lock().unwrap();
            settings.n_expected_rounds = 1;
        }
        let model = ai::ModelConfig {
            input_cost_per_m: 3.0,
            output_cost_per_m: 15.0,
            ..ai::ModelConfig::default()
        };
        let (provider, _rx) = MockProvider::new();

        let mut messages = history(12);
        let before = messages.len();
        // A huge assumed cached prefix makes invalidating any of it (pruning
        // or summarizing) far more expensive than the tiny per-turn savings.
        let info = cost_aware_compact(&ctx, &mut messages, &model, &provider, 10_000_000).await;

        let info = info.expect("eligible candidates must still produce a CompactionInfo");
        assert_eq!(info.kind, "candidates");
        assert!(
            info.details.iter().any(|d| d.action == "trim_candidate"),
            "kept-eligible entries must be recorded as trim_candidate"
        );
        assert_eq!(messages.len(), before, "nothing actually removed");
    }

    /// bugs.md Bug 2: the settings toggle must make compaction a strict no-op,
    /// even on a history that would otherwise trigger it (long_history_compacts_on_first_call).
    #[tokio::test]
    async fn disable_context_trimming_setting_is_a_full_no_op() {
        std::env::set_var("TRACELEAN_MOCK_AUTO", "mock summary");
        let ctx = AgentContext::from_env(std::env::temp_dir());
        {
            let mut settings = ctx.settings.lock().unwrap();
            settings.disable_context_trimming = true;
        }
        let model = ai::ModelConfig::default();
        let (provider, _rx) = MockProvider::new();

        let mut messages = history(12);
        let before = messages.len();
        let info = cost_aware_compact(&ctx, &mut messages, &model, &provider, 0).await;

        assert!(info.is_none(), "trimming must be fully disabled by the setting");
        assert_eq!(messages.len(), before, "messages must be untouched");
    }
}

#[cfg(test)]
mod calibration_tests {
    use super::*;
    use crate::ai::tracking::TokenUsage;

    fn text_request(content: &str) -> AiRequest {
        AiRequest {
            model: ai::ModelConfig::default(),
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: content.to_string(),
                tool_call_id: None,
                tool_calls: Vec::new(),
            }],
            stop: None,
            tools: None,
            dynamic_tools: None,
            cache_breakpoints: Vec::new(),
        }
    }

    fn usage_with_input_tokens(input_tokens: u32) -> TokenUsage {
        TokenUsage {
            input_tokens,
            output_tokens: 0,
            thinking_tokens: 0,
            cached_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    /// Live bug: the flat chars/4 default consistently under-predicted real
    /// cached tokens by 50-200% on JSON/code-heavy tool traffic (real ratio
    /// runs lower than 4 chars/token for that content). One real
    /// request/response pair should nudge the calibrated ratio toward the
    /// observed value, not leave it pinned at the default.
    #[test]
    fn one_real_response_moves_the_ratio_toward_the_observed_value() {
        let ctx = AgentContext::from_env(std::env::temp_dir());
        assert_eq!(*ctx.calibrated_chars_per_token.lock().unwrap(), DEFAULT_CHARS_PER_TOKEN);

        // 400 chars of content, but the provider reports 200 real input
        // tokens: observed ratio 2.0, well under the 4.0 default.
        let request = text_request(&"x".repeat(400));
        update_chars_per_token_calibration(&ctx, &request, &usage_with_input_tokens(200));

        let ratio = *ctx.calibrated_chars_per_token.lock().unwrap();
        assert!(
            ratio < DEFAULT_CHARS_PER_TOKEN && ratio > 2.0,
            "expected the ratio to move from 4.0 toward 2.0 but not jump straight there, got {ratio}"
        );
    }

    /// A single anomalous turn (e.g. a near-empty request right after
    /// compaction) must not be able to swing the calibration to an unusable
    /// extreme — it's EMA-smoothed and clamped.
    #[test]
    fn calibration_is_smoothed_and_clamped_against_outlier_turns() {
        let ctx = AgentContext::from_env(std::env::temp_dir());

        // Wildly implausible observed ratio (10 chars sent, 1 real token —
        // ratio 10.0) must not push the calibration anywhere near 10.0 in
        // one step, and must stay inside the clamp band.
        let request = text_request(&"x".repeat(10));
        update_chars_per_token_calibration(&ctx, &request, &usage_with_input_tokens(1));

        let ratio = *ctx.calibrated_chars_per_token.lock().unwrap();
        assert!(ratio <= 6.0, "clamp band must cap the ratio, got {ratio}");
        assert!(
            ratio < 6.0 || (DEFAULT_CHARS_PER_TOKEN * 0.7 + 10.0 * 0.3) >= 6.0,
            "single outlier turn must be smoothed, not applied in full, got {ratio}"
        );
    }

    /// A zero-token response (e.g. an error path with no real usage data)
    /// must leave the calibration untouched rather than dividing by zero or
    /// corrupting it with a bogus observation.
    #[test]
    fn zero_input_tokens_leaves_calibration_untouched() {
        let ctx = AgentContext::from_env(std::env::temp_dir());
        let request = text_request("hello");
        update_chars_per_token_calibration(&ctx, &request, &usage_with_input_tokens(0));
        assert_eq!(*ctx.calibrated_chars_per_token.lock().unwrap(), DEFAULT_CHARS_PER_TOKEN);
    }
}
