//! Amazon Bedrock provider — two wire protocols, chosen by model.
//!
//! - Claude models (`model_id` contains "claude") go to the **Bedrock Runtime
//!   Converse API**: `bedrock-runtime.{region}.amazonaws.com/model/{id}/converse`,
//!   authenticated with the bearer token as a plain `Authorization: Bearer`
//!   header. Bedrock's OpenAI-compatible Chat Completions endpoint (below)
//!   actively rejects Claude models outright
//!   (`"does not support the '/v1/chat/completions' API"`), so this isn't a
//!   preference, it's the only way Claude works at all on Bedrock. Converse
//!   gets real prompt caching via explicit `cachePoint` blocks (Claude only
//!   caches on an explicit marker — there is no automatic/passive caching for
//!   it) and native `toolUse`/`toolResult` content blocks — verified live
//!   against the real endpoint (cache write → cache read across two calls,
//!   full tool-call → tool-result round trip).
//!
//!   Cross-region-only Claude models (Sonnet 4.5 and later, at time of
//!   writing) require an inference-profile-prefixed model ID
//!   (`global.anthropic.claude-...`, `eu.anthropic.claude-...`, etc.) — Bedrock
//!   rejects the bare ID with a 400 naming the requirement. This is not
//!   handled with a retry: the error is surfaced as-is (see
//!   `complete_anthropic`) so the fix (pick a profile-prefixed model ID in
//!   settings) is visible instead of hidden behind silent retry logic.
//! - Every other Bedrock model (Nova, MiniMax, Mistral, Qwen, ...) keeps using
//!   the **OpenAI-compatible Chat Completions API**:
//!   `bedrock-mantle.{region}.api.aws/v1/chat/completions`, Bearer token auth.
//!
//! Tool passing on the Chat Completions path follows the model's `ToolPassing`
//! strategy (P9a):
//! - `NativeParam` (default): tools go in the request `tools` field, history keeps
//!   structured `tool_calls` / `role: "tool"` messages, and native response
//!   `tool_calls` take precedence (text parsing stays as fallback).
//! - `SystemPromptEmbed` (Bedrock+MiniMax): tools are rendered as text into the
//!   system prompt in the model's trained dialect (`ToolCallFormat`, P2) and
//!   calls are parsed from response text in that dialect.
//!
//! The Converse path always uses real `toolUse`/`toolResult` blocks —
//! `ToolPassing`/`ToolCallFormat` don't apply to it.

use super::provider::*;
use super::tool_dialects::{
    build_schema_map, parse_tool_blocks_with, render_tools_as_text, strip_tool_blocks,
};
use super::tracking::TokenUsage;
use serde::{Deserialize, Serialize};

/// Default region for Bedrock API.
pub const DEFAULT_REGION: &str = "eu-west-1";
/// Default model ID.
pub const DEFAULT_MODEL_ID: &str = "eu.amazon.nova-lite-v1:0";

pub struct BedrockProvider {
    bearer_token: String,
    region: String,
    client: reqwest::Client,
    max_retries: u32,
    pub verbose: bool,
}

impl BedrockProvider {
    pub fn new(bearer_token: String, region: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            bearer_token,
            region: region.unwrap_or_else(|| DEFAULT_REGION.into()),
            client,
            max_retries: 3,
            verbose: false,
        }
    }

    /// Chat Completions endpoint on bedrock-mantle.
    fn endpoint(&self) -> String {
        format!(
            "https://bedrock-mantle.{}.api.aws/v1/chat/completions",
            self.region
        )
    }

    /// Bedrock Runtime Converse endpoint for a given model ID.
    fn converse_endpoint(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse",
            self.region, model_id
        )
    }
}

/// Whether a model ID should be routed through the native Anthropic Messages
/// API rather than the OpenAI-compatible Chat Completions endpoint. Bedrock's
/// Chat Completions endpoint rejects Claude models outright, so this isn't a
/// preference — it's the only path that works for them.
fn is_claude_model(model_id: &str) -> bool {
    model_id.to_lowercase().contains("claude")
}

/// Strip `<think>…</think>` spans (D2.6): MiniMax emits them in its native
/// dialect. An unterminated `<think>` drops the rest of the content (it is
/// all thinking).
#[cfg(test)]
fn strip_think_tags(content: &str) -> String {
    extract_think_tags(content).0
}

/// Like `strip_think_tags`, but also returns the captured thinking text
/// (bugs.md Bug 1.8: surfaced in the chat UI instead of silently dropped).
fn extract_think_tags(content: &str) -> (String, Option<String>) {
    let mut result = content.to_string();
    let mut thinking = String::new();
    loop {
        let Some(open) = result.find("<think>") else { break };
        let inner_start = open + "<think>".len();
        match result[open..].find("</think>") {
            Some(rel) => {
                let end = open + rel + "</think>".len();
                if !thinking.is_empty() {
                    thinking.push('\n');
                }
                thinking.push_str(result[inner_start..open + rel].trim());
                result.replace_range(open..end, "");
            }
            None => {
                if !thinking.is_empty() {
                    thinking.push('\n');
                }
                thinking.push_str(result[inner_start..].trim());
                result.truncate(open);
                break;
            }
        }
    }
    let thinking = if thinking.trim().is_empty() { None } else { Some(thinking) };
    (result.trim().to_string(), thinking)
}


// ---------------------------------------------------------------------------
// Chat Completions request/response types (OpenAI-compatible)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMsg>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    /// Native tool passing (P9a) — only set for `ToolPassing::NativeParam`.
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolSchema>>,
}

#[derive(Serialize)]
struct ChatMsg {
    role: String,
    content: String,
    /// Structured tool calls on assistant messages (native history, P9a).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallResponse>>,
    /// Set on `role: "tool"` result messages (native history, P9a).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl ChatMsg {
    fn text(role: &str, content: String) -> Self {
        Self { role: role.into(), content, tool_calls: None, tool_call_id: None }
    }
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Option<Vec<ChatChoice>>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize, Debug)]
struct ChatChoice {
    message: Option<ChatChoiceMessage>,
    finish_reason: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ChatChoiceMessage {
    content: Option<String>,
    reasoning: Option<String>,
    /// Native tool calls (P9a). Previously not deserialized at all, which made
    /// every natively-tool-calling Bedrock model appear tool-less.
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallResponse>>,
}

#[derive(Deserialize, Debug)]
struct ChatUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize, Debug, Default)]
struct PromptTokensDetails {
    cached_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// Provider implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl AiProvider for BedrockProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        if is_claude_model(&request.model.model_id) {
            return self.complete_anthropic(request).await;
        }

        // Build messages: embed tools in system prompt, flatten tool_call/tool history
        let messages = self.build_messages(request);

        // Verbose: log the ACTUAL payload being sent to the API
        if self.verbose {
            eprintln!("\n--- [bedrock] ACTUAL PAYLOAD ({} messages) ---", messages.len());
            eprintln!(
                "[tool_passing: {:?} | tool_call_format: {:?}]",
                request.model.tool_passing, request.model.tool_call_format
            );
            if let Some(ref tools) = request.tools {
                let names: Vec<&str> = tools.iter().map(|t| t.function.name.as_str()).collect();
                eprintln!("[tools: {}]", names.join(", "));
            }
            for (i, m) in messages.iter().enumerate() {
                let preview = if m.content.len() > 800 {
                    format!("{}… ({} chars)", &m.content[..800], m.content.len())
                } else {
                    m.content.clone()
                };
                eprintln!("[{}] role={}\n{}\n", i, m.role, preview);
            }
            eprintln!("--- [bedrock] END PAYLOAD ---\n");
        }

        // Native tool passing (P9a): tools go in the request field.
        // Dynamic tools (bugs.md Feature 3) merge into the param on the native
        // path — Chat Completions has no defer_loading, so this is the only
        // native option and carries the documented invalidation cost.
        let native = request.model.tool_passing == ToolPassing::NativeParam;
        let body = ChatRequest {
            model: request.model.model_id.clone(),
            messages,
            max_tokens: request.model.max_tokens,
            temperature: request.model.temperature,
            stop: request.stop.clone(),
            tools: if native {
                let mut all = request.tools.clone().unwrap_or_default();
                if let Some(dyn_tools) = &request.dynamic_tools {
                    all.extend(dyn_tools.iter().cloned());
                }
                if all.is_empty() { None } else { Some(all) }
            } else {
                None
            },
        };

        // bugs.md Bug 3: capture the exact wire body for the log's "copy raw
        // request" button — `body` is a locally-owned struct, so this is a
        // plain serialize with no extra request work.
        let raw_request = serde_json::to_string_pretty(&body).ok();

        let url = self.endpoint();

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
            }

            let resp = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.bearer_token))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let status = r.status();
                    let raw_text = r.text().await.unwrap_or_default();

                    if status == 429 {
                        last_err = Some(AiError {
                            kind: AiErrorKind::RateLimit,
                            message: format!("Throttled (attempt {})", attempt + 1),
                            retryable: true,
                        });
                        continue;
                    }

                    if status == 401 || status == 403 {
                        return Err(AiError {
                            kind: AiErrorKind::Authentication,
                            message: format!("Auth failed ({}): {}", status, &raw_text[..raw_text.len().min(300)]),
                            retryable: false,
                        });
                    }

                    if !status.is_success() {
                        let retryable = status.is_server_error();
                        last_err = Some(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("HTTP {}: {}", status, &raw_text[..raw_text.len().min(400)]),
                            retryable,
                        });
                        if !retryable { return Err(last_err.unwrap()); }
                        continue;
                    }

                    // Parse response
                    let parsed: ChatResponse = serde_json::from_str(&raw_text).map_err(|e| AiError {
                        kind: AiErrorKind::ProviderError,
                        message: format!("Parse error: {} | body: {}", e, &raw_text[..raw_text.len().min(400)]),
                        retryable: false,
                    })?;

                    let choice = parsed.choices.as_ref()
                        .and_then(|c| c.first())
                        .ok_or(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("No choices in response | body: {}", &raw_text[..raw_text.len().min(400)]),
                            retryable: false,
                        })?;

                    let message = choice.message.as_ref().ok_or(AiError {
                        kind: AiErrorKind::ProviderError,
                        message: "No message in choice".into(),
                        retryable: false,
                    })?;

                    // Strip <think>…</think> before tool parsing and display
                    // (D2.6), capturing the thinking text for the UI (Bug 1.8).
                    let (raw_content, think_text) =
                        extract_think_tags(&message.content.clone().unwrap_or_default());
                    let thinking = message
                        .reasoning
                        .clone()
                        .filter(|r| !r.trim().is_empty())
                        .or(think_text);
                    let truncated = choice.finish_reason.as_deref() == Some("length");

                    // Native tool calls take precedence (D9a.2); text parsing
                    // stays as fallback so a model answering in dialect text
                    // still works.
                    let native_calls: Vec<ToolCallResponse> = message
                        .tool_calls
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .enumerate()
                        .map(|(i, mut tc)| {
                            if tc.id.is_empty() {
                                tc.id = format!("call_{}", i);
                            }
                            if tc.call_type.is_empty() {
                                tc.call_type = "function".into();
                            }
                            tc
                        })
                        .collect();

                    let (tool_calls, mut content) = if !native_calls.is_empty() {
                        (native_calls, raw_content)
                    } else {
                        // Parse tool calls from text, dialect-first (P2)
                        let schemas = build_schema_map(request.tools.as_deref());
                        let tool_parsed = parse_tool_blocks_with(
                            &raw_content,
                            request.model.tool_call_format,
                            &schemas,
                        );

                        // Strip tool blocks from content (leave only natural text)
                        let mut content = if !tool_parsed.calls.is_empty()
                            || !tool_parsed.errors.is_empty()
                        {
                            strip_tool_blocks(&raw_content)
                        } else {
                            raw_content
                        };

                        // If there were parse errors, append them to content so runtime
                        // sees them and can feed back to the model on next iteration
                        if !tool_parsed.errors.is_empty() {
                            let registry = super::tool_registry::ToolRegistry::embedded();
                            for err in &tool_parsed.errors {
                                content.push_str("\n\n");
                                content.push_str(&super::tool_errors::format_tool_call_error(
                                    None,
                                    err,
                                    "tool call could not be parsed",
                                    registry,
                                    request.model.tool_call_format,
                                ));
                            }
                        }

                        (tool_parsed.calls, content)
                    };

                    // A thinking model can burn the whole output budget inside
                    // an unterminated <think> span; stripping then leaves
                    // nothing. Surface that instead of an empty answer.
                    if truncated && content.is_empty() && tool_calls.is_empty() {
                        content = "[No answer: the model used its entire output budget on internal thinking before being cut off. Retry, simplify the request, or raise max_tokens.]".to_string();
                    }

                    // Token usage
                    let usage = parsed.usage.as_ref();
                    let cached = usage
                        .and_then(|u| u.prompt_tokens_details.as_ref())
                        .and_then(|d| d.cached_tokens)
                        .unwrap_or(0);

                    // Count reasoning/thinking tokens if present (covers both
                    // the `reasoning` field and captured <think> spans)
                    let thinking_tokens = thinking.as_ref()
                        .map(|r| (r.len() as u32) / 4) // rough estimate: ~4 chars per token
                        .unwrap_or(0);

                    let token_usage = TokenUsage {
                        input_tokens: usage.and_then(|u| u.prompt_tokens).unwrap_or(0),
                        output_tokens: usage.and_then(|u| u.completion_tokens).unwrap_or(0),
                        thinking_tokens,
                        cached_tokens: cached,
                    };

                    return Ok(AiResponse {
                        content,
                        usage: token_usage,
                        raw_response: Some(raw_text),
                        raw_request,
                        truncated,
                        tool_calls,
                        thinking,
                    });
                }
                Err(e) => {
                    let kind = if e.is_timeout() {
                        AiErrorKind::Timeout
                    } else {
                        AiErrorKind::Network
                    };
                    last_err = Some(AiError {
                        kind,
                        message: e.to_string(),
                        retryable: true,
                    });
                }
            }
        }

        Err(last_err.unwrap_or(AiError {
            kind: AiErrorKind::Network,
            message: "All retries exhausted".into(),
            retryable: false,
        }))
    }

    fn name(&self) -> &str {
        "Amazon Bedrock (Mantle)"
    }

    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError> {
        let url = format!(
            "https://bedrock-mantle.{}.api.aws/v1/models",
            self.region
        );

        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.bearer_token))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| AiError {
                kind: AiErrorKind::Network,
                message: e.to_string(),
                retryable: true,
            })?;

        if resp.status() == 401 || resp.status() == 403 {
            return Err(AiError {
                kind: AiErrorKind::Authentication,
                message: format!("Auth failed fetching models ({})", resp.status()),
                retryable: false,
            });
        }

        if !resp.status().is_success() {
            return Err(AiError {
                kind: AiErrorKind::ProviderError,
                message: format!("HTTP {} fetching models", resp.status()),
                retryable: false,
            });
        }

        let body: ListModelsResponse = resp.json().await.map_err(|e| AiError {
            kind: AiErrorKind::ProviderError,
            message: format!("Parse models: {}", e),
            retryable: false,
        })?;

        let models = body.data.into_iter()
            .filter(|m| m.status.as_deref() != Some("unavailable"))
            .map(|m| model_for_id(&m.id))
            .collect();

        Ok(models)
    }
}

// ---------------------------------------------------------------------------
// Bedrock Runtime Converse API (Claude models only)
// ---------------------------------------------------------------------------

/// A single Converse content block. Distinguished by JSON key presence, not
/// a `"type"` tag — Converse's wire format differs from Anthropic's native
/// Messages API in exactly this way (no discriminator field).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
enum ConverseBlock {
    ToolUse {
        #[serde(rename = "toolUse")]
        tool_use: ConverseToolUse,
    },
    ToolResult {
        #[serde(rename = "toolResult")]
        tool_result: ConverseToolResult,
    },
    CachePoint {
        #[serde(rename = "cachePoint")]
        cache_point: ConverseCachePoint,
    },
    Text {
        text: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ConverseCachePoint {
    #[serde(rename = "type")]
    point_type: String,
}

impl ConverseCachePoint {
    fn default_ephemeral() -> Self {
        Self { point_type: "default".into() }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ConverseToolUse {
    #[serde(rename = "toolUseId")]
    tool_use_id: String,
    name: String,
    #[serde(default)]
    input: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ConverseToolResult {
    #[serde(rename = "toolUseId")]
    tool_use_id: String,
    content: Vec<ConverseTextOnly>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ConverseTextOnly {
    text: String,
}

#[derive(Serialize, Debug)]
struct ConverseMessage {
    role: String,
    content: Vec<ConverseBlock>,
}

#[derive(Serialize, Debug)]
struct ConverseToolSpec {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: ConverseInputSchema,
}

#[derive(Serialize, Debug)]
struct ConverseInputSchema {
    json: serde_json::Value,
}

#[derive(Serialize, Debug)]
struct ConverseTool {
    #[serde(rename = "toolSpec")]
    tool_spec: ConverseToolSpec,
}

#[derive(Serialize, Debug)]
struct ConverseToolConfig {
    tools: Vec<ConverseTool>,
}

#[derive(Serialize, Debug)]
struct ConverseInferenceConfig {
    #[serde(rename = "maxTokens")]
    max_tokens: u32,
    temperature: f32,
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Serialize, Debug)]
struct ConverseRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<ConverseBlock>>,
    messages: Vec<ConverseMessage>,
    #[serde(rename = "inferenceConfig")]
    inference_config: ConverseInferenceConfig,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    tool_config: Option<ConverseToolConfig>,
}

#[derive(Deserialize, Debug, Default)]
struct ConverseResponse {
    output: Option<ConverseOutput>,
    #[serde(rename = "stopReason")]
    stop_reason: Option<String>,
    usage: Option<ConverseUsage>,
}

#[derive(Deserialize, Debug)]
struct ConverseOutput {
    message: Option<ConverseResponseMessage>,
}

#[derive(Deserialize, Debug)]
struct ConverseResponseMessage {
    #[serde(default)]
    content: Vec<ConverseBlock>,
}

#[derive(Deserialize, Debug, Default)]
struct ConverseUsage {
    #[serde(rename = "inputTokens", default)]
    input_tokens: u32,
    #[serde(rename = "outputTokens", default)]
    output_tokens: u32,
    #[serde(rename = "cacheReadInputTokens", default)]
    cache_read_input_tokens: u32,
    #[serde(rename = "cacheWriteInputTokens", default)]
    cache_write_input_tokens: u32,
}

#[derive(Deserialize, Debug)]
struct ConverseErrorBody {
    message: String,
}

/// Where a source `ChatMessage` (by original index) landed in the built
/// Converse request, so `cache_breakpoints` (indices into the original
/// message list) can be translated into an inserted `cachePoint` block after
/// system-message extraction and tool-result merging shift things around.
enum BlockLocation {
    System { block_idx: usize },
    Message { msg_idx: usize, block_idx: usize },
}

impl BedrockProvider {
    /// Convert the OpenAI-shaped static+dynamic tool schemas into Converse's
    /// `toolSpec` shape.
    fn converse_tools(request: &AiRequest) -> Option<ConverseToolConfig> {
        let mut all = request.tools.clone().unwrap_or_default();
        if let Some(dyn_tools) = &request.dynamic_tools {
            all.extend(dyn_tools.iter().cloned());
        }
        if all.is_empty() {
            return None;
        }
        Some(ConverseToolConfig {
            tools: all
                .into_iter()
                .map(|t| ConverseTool {
                    tool_spec: ConverseToolSpec {
                        name: t.function.name,
                        description: t.function.description,
                        input_schema: ConverseInputSchema { json: t.function.parameters },
                    },
                })
                .collect(),
        })
    }

    /// Build `system` + `messages` for the Converse request, inserting a
    /// `cachePoint` block right after each location named by
    /// `request.cache_breakpoints` — same planning input `openrouter.rs`
    /// consumes, translated into Converse's insert-a-sibling-block cache
    /// mechanism (Converse has no per-block `cache_control` property; caching
    /// is a distinct block type inserted into the content array, unlike
    /// Anthropic's native Messages API which mutates a property on an
    /// existing block).
    ///
    /// Consecutive `Tool`-role messages (parallel tool results from one
    /// assistant turn) are merged into a single `user` turn with multiple
    /// `toolResult` blocks — required by the Converse API, and needed so
    /// parallel tool calls round-trip instead of training the model to stop
    /// batching them (returning results split across messages has that
    /// effect).
    fn build_converse_messages(
        request: &AiRequest,
    ) -> (Option<Vec<ConverseBlock>>, Vec<ConverseMessage>) {
        let mut system_blocks: Vec<ConverseBlock> = Vec::new();
        let mut out: Vec<ConverseMessage> = Vec::new();
        let mut locations: Vec<BlockLocation> = Vec::with_capacity(request.messages.len());
        let mut prev_role: Option<&MessageRole> = None;

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    system_blocks.push(ConverseBlock::Text { text: msg.content.clone() });
                    locations.push(BlockLocation::System {
                        block_idx: system_blocks.len() - 1,
                    });
                }
                MessageRole::User => {
                    out.push(ConverseMessage {
                        role: "user".into(),
                        content: vec![ConverseBlock::Text { text: msg.content.clone() }],
                    });
                    locations.push(BlockLocation::Message {
                        msg_idx: out.len() - 1,
                        block_idx: 0,
                    });
                }
                MessageRole::Assistant => {
                    let mut content: Vec<ConverseBlock> = Vec::new();
                    if !msg.content.is_empty() {
                        content.push(ConverseBlock::Text { text: msg.content.clone() });
                    }
                    for tc in &msg.tool_calls {
                        let input: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(Default::default()));
                        content.push(ConverseBlock::ToolUse {
                            tool_use: ConverseToolUse {
                                tool_use_id: tc.id.clone(),
                                name: tc.function.name.clone(),
                                input,
                            },
                        });
                    }
                    if content.is_empty() {
                        // The API rejects an empty content array.
                        content.push(ConverseBlock::Text { text: " ".into() });
                    }
                    let block_idx = content.len() - 1;
                    out.push(ConverseMessage {
                        role: "assistant".into(),
                        content,
                    });
                    locations.push(BlockLocation::Message {
                        msg_idx: out.len() - 1,
                        block_idx,
                    });
                }
                MessageRole::Tool => {
                    let block = ConverseBlock::ToolResult {
                        tool_result: ConverseToolResult {
                            tool_use_id: msg.tool_call_id.clone().unwrap_or_default(),
                            content: vec![ConverseTextOnly { text: msg.content.clone() }],
                        },
                    };
                    // Merge into the previous message only if it, too, was a
                    // Tool-role message in the *original* sequence (i.e. this
                    // is another parallel result for the same assistant
                    // turn) — not merely because the last built message
                    // happens to have role "user".
                    let merge = matches!(prev_role, Some(MessageRole::Tool));
                    if merge {
                        let msg_idx = out.len() - 1;
                        out[msg_idx].content.push(block);
                        let block_idx = out[msg_idx].content.len() - 1;
                        locations.push(BlockLocation::Message { msg_idx, block_idx });
                    } else {
                        out.push(ConverseMessage {
                            role: "user".into(),
                            content: vec![block],
                        });
                        locations.push(BlockLocation::Message {
                            msg_idx: out.len() - 1,
                            block_idx: 0,
                        });
                    }
                }
            }
            prev_role = Some(&msg.role);
        }

        // Insert cachePoint blocks. Converse caches via a distinct sibling
        // block placed right after the target block, so — unlike the native
        // Anthropic path, which mutates a `cache_control` property in
        // place — insertion shifts every later index in the same container.
        // Group by container, sort ascending, then insert highest-index-first
        // so earlier insertions don't shift indices still pending.
        let mut system_inserts: Vec<usize> = Vec::new();
        let mut message_inserts: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for &bp in &request.cache_breakpoints {
            let Some(loc) = locations.get(bp) else { continue };
            match loc {
                BlockLocation::System { block_idx } => system_inserts.push(*block_idx),
                BlockLocation::Message { msg_idx, block_idx } => {
                    message_inserts.entry(*msg_idx).or_default().push(*block_idx)
                }
            }
        }

        system_inserts.sort_unstable();
        system_inserts.dedup();
        for &idx in system_inserts.iter().rev() {
            if idx < system_blocks.len() {
                system_blocks.insert(
                    idx + 1,
                    ConverseBlock::CachePoint { cache_point: ConverseCachePoint::default_ephemeral() },
                );
            }
        }

        for (msg_idx, mut idxs) in message_inserts {
            idxs.sort_unstable();
            idxs.dedup();
            if let Some(m) = out.get_mut(msg_idx) {
                for &idx in idxs.iter().rev() {
                    if idx < m.content.len() {
                        m.content.insert(
                            idx + 1,
                            ConverseBlock::CachePoint { cache_point: ConverseCachePoint::default_ephemeral() },
                        );
                    }
                }
            }
        }

        let system = if system_blocks.is_empty() {
            None
        } else {
            Some(system_blocks)
        };
        (system, out)
    }

    /// Append the batching reminder as a trailing text block on the last
    /// `user`-role message, same intent as `append_batching_reminder` on the
    /// Chat Completions path.
    fn append_batching_reminder_converse(request: &AiRequest, out: &mut [ConverseMessage]) {
        if request.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false) {
            if let Some(last) = out.iter_mut().rev().find(|m| m.role == "user") {
                last.content.push(ConverseBlock::Text {
                    text: "[IMPORTANT: Return ALL independent tool calls in ONE response. Do NOT call one tool then wait.]".into(),
                });
            }
        }
    }

    /// Whether a Bedrock Converse error message is the "needs a cross-region
    /// inference profile" 400 for newer Claude models — distinct from a
    /// plain "not accessible" account/model-access gate, so the two produce
    /// different (equally non-retried) error messages.
    fn is_inference_profile_required_error(message: &str) -> bool {
        let m = message.to_lowercase();
        m.contains("inference profile")
            && (m.contains("on-demand throughput") || m.contains("on demand throughput"))
    }

    async fn complete_anthropic(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let (system, mut messages) = Self::build_converse_messages(request);
        Self::append_batching_reminder_converse(request, &mut messages);
        let tool_config = Self::converse_tools(request);

        let body = ConverseRequest {
            system,
            messages,
            inference_config: ConverseInferenceConfig {
                max_tokens: request.model.max_tokens,
                temperature: request.model.temperature,
                stop_sequences: request.stop.clone(),
            },
            tool_config,
        };

        if self.verbose {
            eprintln!("\n--- [bedrock/converse] ACTUAL PAYLOAD ---");
            if let Ok(pretty) = serde_json::to_string_pretty(&body) {
                eprintln!("{}", pretty);
            }
            eprintln!("--- [bedrock/converse] END PAYLOAD ---\n");
        }

        let raw_request = serde_json::to_string_pretty(&body).ok();
        let url = self.converse_endpoint(&request.model.model_id);

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
            }

            let resp = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.bearer_token))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(r) => {
                    let status = r.status();
                    let raw_text = r.text().await.unwrap_or_default();

                    if status == 429 {
                        last_err = Some(AiError {
                            kind: AiErrorKind::RateLimit,
                            message: format!("Throttled (attempt {})", attempt + 1),
                            retryable: true,
                        });
                        continue;
                    }

                    if status == 401 || status == 403 {
                        let message = serde_json::from_str::<ConverseErrorBody>(&raw_text)
                            .map(|e| e.message)
                            .unwrap_or_else(|_| raw_text[..raw_text.len().min(300)].to_string());
                        return Err(AiError {
                            kind: AiErrorKind::Authentication,
                            message: format!(
                                "Claude model '{}' is not accessible on this Bedrock account/region ({}): {}",
                                request.model.model_id, status, message
                            ),
                            retryable: false,
                        });
                    }

                    if !status.is_success() {
                        let message = serde_json::from_str::<ConverseErrorBody>(&raw_text)
                            .map(|e| e.message)
                            .unwrap_or_else(|_| raw_text[..raw_text.len().min(400)].to_string());

                        if Self::is_inference_profile_required_error(&message) {
                            return Err(AiError {
                                kind: AiErrorKind::InvalidRequest,
                                message: format!(
                                    "Claude model '{}' is not supported without a cross-region inference profile on this account. Use a profile-prefixed model ID instead (e.g. \"global.{}\" or \"eu.{}\"). Bedrock said: {}",
                                    request.model.model_id,
                                    request.model.model_id,
                                    request.model.model_id,
                                    message
                                ),
                                retryable: false,
                            });
                        }

                        let retryable = status.is_server_error();
                        last_err = Some(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("HTTP {}: {}", status, message),
                            retryable,
                        });
                        if !retryable {
                            return Err(last_err.unwrap());
                        }
                        continue;
                    }

                    let parsed: ConverseResponse =
                        serde_json::from_str(&raw_text).map_err(|e| AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!(
                                "Parse error: {} | body: {}",
                                e,
                                &raw_text[..raw_text.len().min(400)]
                            ),
                            retryable: false,
                        })?;

                    let mut content_text = String::new();
                    let mut tool_calls: Vec<ToolCallResponse> = Vec::new();
                    let blocks = parsed
                        .output
                        .as_ref()
                        .and_then(|o| o.message.as_ref())
                        .map(|m| m.content.as_slice())
                        .unwrap_or(&[]);
                    for (i, block) in blocks.iter().enumerate() {
                        match block {
                            ConverseBlock::Text { text } => {
                                if !content_text.is_empty() {
                                    content_text.push('\n');
                                }
                                content_text.push_str(text);
                            }
                            ConverseBlock::ToolUse { tool_use } => {
                                tool_calls.push(ToolCallResponse {
                                    id: if tool_use.tool_use_id.is_empty() {
                                        format!("call_{}", i)
                                    } else {
                                        tool_use.tool_use_id.clone()
                                    },
                                    call_type: "function".into(),
                                    function: ToolCallFunction {
                                        name: tool_use.name.clone(),
                                        arguments: serde_json::to_string(&tool_use.input)
                                            .unwrap_or_else(|_| "{}".into()),
                                    },
                                });
                            }
                            ConverseBlock::ToolResult { .. } | ConverseBlock::CachePoint { .. } => {}
                        }
                    }

                    let truncated = parsed.stop_reason.as_deref() == Some("max_tokens");
                    let usage = parsed.usage.unwrap_or_default();
                    // Converse's `inputTokens` is the UNCACHED remainder only —
                    // add cache reads/writes back in so `TokenUsage.input_tokens`
                    // stays "total input, cached_tokens is a subset of it",
                    // matching every other provider's `estimate_cost` assumption.
                    // Verified live: a two-call test with an identical ~7200-token
                    // cached system prompt showed `inputTokens` stay at 12 while
                    // `cacheWriteInputTokens`/`cacheReadInputTokens` swapped
                    // between 7202/0 and 0/7202.
                    let total_input = usage
                        .input_tokens
                        .saturating_add(usage.cache_read_input_tokens)
                        .saturating_add(usage.cache_write_input_tokens);

                    return Ok(AiResponse {
                        content: content_text,
                        usage: TokenUsage {
                            input_tokens: total_input,
                            output_tokens: usage.output_tokens,
                            thinking_tokens: 0,
                            cached_tokens: usage.cache_read_input_tokens,
                        },
                        raw_response: Some(raw_text),
                        raw_request,
                        truncated,
                        tool_calls,
                        thinking: None,
                    });
                }
                Err(e) => {
                    let kind = if e.is_timeout() {
                        AiErrorKind::Timeout
                    } else {
                        AiErrorKind::Network
                    };
                    last_err = Some(AiError {
                        kind,
                        message: e.to_string(),
                        retryable: true,
                    });
                }
            }
        }

        Err(last_err.unwrap_or(AiError {
            kind: AiErrorKind::Network,
            message: "All retries exhausted".into(),
            retryable: false,
        }))
    }
}

/// Build a Bedrock model's `ModelConfig` from just its id — no network call.
/// Pricing/catalog data (`provider_cache`, `model_catalog`) is all
/// compiled-in static data, so this is cheap to call anytime a Bedrock
/// model's up-to-date info is needed (e.g. re-deriving a persisted settings
/// selection on load, instead of trusting a stale saved snapshot — bugs.md:
/// MiniMax's cached price stayed wrong in a saved selection after the
/// pricing table was fixed).
pub fn model_for_id(model_id: &str) -> ModelConfig {
    let (input_cost, output_cost) = super::model_catalog::pricing_for(model_id).unwrap_or((0.0, 0.0));
    let mut model = ModelConfig {
        provider: ProviderKind::Bedrock,
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        max_tokens: 4096,
        temperature: 0.3,
        input_cost_per_m: input_cost,
        output_cost_per_m: output_cost,
        cached_input_cost_per_m: super::provider_cache::default_cached_price_per_m(model_id, input_cost),
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: true,
        ..Default::default()
    };
    super::model_catalog::enrich(&mut model);
    model
}

// ---------------------------------------------------------------------------
// Message building: embed tools in system, flatten tool history as text
// ---------------------------------------------------------------------------

impl BedrockProvider {
    /// Build the message array for Chat Completions per the model's
    /// tool-passing strategy (P9a).
    fn build_messages(&self, request: &AiRequest) -> Vec<ChatMsg> {
        match request.model.tool_passing {
            ToolPassing::NativeParam => self.build_messages_native(request),
            ToolPassing::SystemPromptEmbed => self.build_messages_embedded(request),
        }
    }

    /// Native path (D9a.2): history keeps real `tool_calls` and `role: "tool"`
    /// messages — no text flattening. Tools go in the request `tools` field.
    fn build_messages_native(&self, request: &AiRequest) -> Vec<ChatMsg> {
        let mut out: Vec<ChatMsg> = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => out.push(ChatMsg::text("system", msg.content.clone())),
                MessageRole::User => out.push(ChatMsg::text("user", msg.content.clone())),
                MessageRole::Assistant => out.push(ChatMsg {
                    role: "assistant".into(),
                    content: msg.content.clone(),
                    tool_calls: if msg.tool_calls.is_empty() {
                        None
                    } else {
                        Some(msg.tool_calls.clone())
                    },
                    tool_call_id: None,
                }),
                MessageRole::Tool => out.push(ChatMsg {
                    role: "tool".into(),
                    content: msg.content.clone(),
                    tool_calls: None,
                    tool_call_id: msg.tool_call_id.clone(),
                }),
            }
        }

        self.append_batching_reminder(request, &mut out);
        out
    }

    /// Embedded path (P2, Bedrock+MiniMax):
    /// - Tools are rendered as text at the END of the system prompt, in the
    ///   model's tool-call dialect.
    /// - Assistant tool calls are re-rendered in the SAME dialect (D2.4) so
    ///   history matches what the model is told to emit and the cached prefix
    ///   stays byte-stable.
    /// - Tool result messages are flattened to user messages with an id label.
    fn build_messages_embedded(&self, request: &AiRequest) -> Vec<ChatMsg> {
        let format = request.model.tool_call_format;
        let tools_text = request.tools.as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| render_tools_as_text(t, format));

        let mut out: Vec<ChatMsg> = Vec::new();

        for msg in &request.messages {
            match msg.role {
                MessageRole::System => {
                    // Append tool definitions to system prompt
                    let mut content = msg.content.clone();
                    if let Some(ref tt) = tools_text {
                        content.push_str("\n\n");
                        content.push_str(tt);
                    }
                    out.push(ChatMsg::text("system", content));
                }
                MessageRole::User => {
                    out.push(ChatMsg::text("user", msg.content.clone()));
                }
                MessageRole::Assistant => {
                    // Reconstruct assistant message: text + tool calls in dialect
                    let mut content = msg.content.clone();
                    for tc in &msg.tool_calls {
                        if !content.is_empty() {
                            content.push('\n');
                        }
                        let args: serde_json::Value =
                            serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(Default::default()));
                        content.push_str(&super::tool_errors::render_tool_call(
                            &tc.function.name,
                            &args,
                            format,
                        ));
                    }
                    out.push(ChatMsg::text("assistant", content));
                }
                MessageRole::Tool => {
                    // Tool results → user message with label
                    let tool_id = msg.tool_call_id.as_deref().unwrap_or("unknown");
                    let content = format!("[tool_result id={}]\n{}", tool_id, msg.content);
                    out.push(ChatMsg::text("user", content));
                }
            }
        }

        // If no system message was in the input but we have tools, prepend one
        if tools_text.is_some() && !request.messages.iter().any(|m| m.role == MessageRole::System) {
            if let Some(tt) = tools_text {
                out.insert(0, ChatMsg::text("system", tt));
            }
        }

        // bugs.md Feature 3: dynamically discovered tools are injected as a
        // trailing user message — the system prompt / static tools prefix stays
        // byte-stable, so the provider prompt cache is preserved.
        if let Some(dyn_tools) = request.dynamic_tools.as_ref().filter(|t| !t.is_empty()) {
            let mut txt = String::from(
                "[Additional tools loaded via discover_tools — callable exactly like the tools above]\n",
            );
            txt.push_str(&render_tools_as_text(dyn_tools, format));
            out.push(ChatMsg::text("user", txt));
        }

        self.append_batching_reminder(request, &mut out);
        out
    }

    /// Append the batching reminder to the last user message. Positioned at
    /// the very end of context (prefix-safe, D9b.4).
    fn append_batching_reminder(&self, request: &AiRequest, out: &mut [ChatMsg]) {
        if request.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false) {
            if let Some(last) = out.iter_mut().rev().find(|m| m.role == "user") {
                last.content.push_str("\n\n[IMPORTANT: Return ALL independent tool calls in ONE response. Do NOT call one tool then wait.]");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Model listing types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ListModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    #[serde(default)]
    status: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------


#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tools() -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "read_file".into(),
                    description: "Read a file".into(),
                    parameters: serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}),
                },
            },
            ToolSchema {
                tool_type: "function".into(),
                function: ToolFunction {
                    name: "replace_str".into(),
                    description: "Replace a string in a file".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "old_str": {"type": "string"},
                            "new_str": {"type": "string"},
                            "count": {"type": "integer"},
                            "dry_run": {"type": "boolean"}
                        },
                        "required": ["path", "old_str", "new_str"]
                    }),
                },
            },
        ]
    }

    #[test]
    fn test_strip_think_tags() {
        assert_eq!(
            strip_think_tags("<think>reasoning here</think>Hello."),
            "Hello."
        );
        assert_eq!(
            strip_think_tags("A<think>x</think>B<think>y</think>C"),
            "ABC"
        );
        // Unterminated think drops the tail
        assert_eq!(strip_think_tags("Answer.<think>still going"), "Answer.");
        assert_eq!(strip_think_tags("No tags at all."), "No tags at all.");
    }

    // ─── P9a fixtures: native tool path ──────────────────────────────────────

    #[test]
    fn test_native_response_tool_calls_deserialize() {
        let body = r#"{
            "choices": [{
                "message": {
                    "content": "",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{\"path\": \"a.rs\"}"}
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        }"#;
        let parsed: ChatResponse = serde_json::from_str(body).unwrap();
        let msg = parsed.choices.unwrap().remove(0).message.unwrap();
        let calls = msg.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");
    }

    /// D2.4: embedded MiniMax history re-renders past tool calls in the
    /// MiniMax dialect (not Hermes), and is byte-stable across identical calls.
    #[test]
    fn test_embedded_minimax_history_dialect() {
        use super::super::provider::{ChatMessage, MessageRole, ToolPassing};
        let provider = BedrockProvider::new("test-token".into(), None);
        let mut model = ModelConfig {
            provider: ProviderKind::Bedrock,
            model_id: "minimax.minimax-m2".into(),
            ..Default::default()
        };
        model.tool_call_format = ToolCallFormat::MiniMaxXml;
        model.tool_passing = ToolPassing::SystemPromptEmbed;

        let request = AiRequest {
            dynamic_tools: None,
            model,
            messages: vec![
                ChatMessage {
                    role: MessageRole::System,
                    content: "You are an agent.".into(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
                ChatMessage {
                    role: MessageRole::Assistant,
                    content: "".into(),
                    tool_call_id: None,
                    tool_calls: vec![ToolCallResponse {
                        id: "call_0".into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: "read_file".into(),
                            arguments: "{\"path\":\"a.rs\"}".into(),
                        },
                    }],
                },
                ChatMessage {
                    role: MessageRole::Tool,
                    content: "contents".into(),
                    tool_call_id: Some("call_0".into()),
                    tool_calls: Vec::new(),
                },
                ChatMessage {
                    role: MessageRole::User,
                    content: "now edit it".into(),
                    tool_call_id: None,
                    tool_calls: Vec::new(),
                },
            ],
            stop: None,
            tools: Some(sample_tools()),
            cache_breakpoints: Vec::new(),
        };

        let msgs = provider.build_messages(&request);
        // System prompt carries the MiniMax-dialect tool block
        assert!(msgs[0].content.contains("<tools>"));
        assert!(msgs[0].content.contains("<minimax:tool_call>"));
        // Assistant history re-rendered as MiniMax XML, not Hermes JSON
        assert!(msgs[1].content.contains("<invoke name=\"read_file\">"));
        assert!(msgs[1].content.contains("<parameter name=\"path\">a.rs</parameter>"));
        assert!(!msgs[1].content.contains("<tool_call>"));
        // Tool result flattened to user message
        assert!(msgs[2].content.starts_with("[tool_result id=call_0]"));
        assert_eq!(msgs[2].role, "user");
        // Byte-stable across identical calls (cache precondition)
        let again = provider.build_messages(&request);
        for (a, b) in msgs.iter().zip(again.iter()) {
            assert_eq!(a.content, b.content);
        }
    }

    #[test]
    fn test_chat_request_serializes_native_tools_and_history() {
        let req = ChatRequest {
            model: "eu.amazon.nova-pro-v1:0".into(),
            messages: vec![
                ChatMsg::text("system", "sys".into()),
                ChatMsg {
                    role: "assistant".into(),
                    content: "".into(),
                    tool_calls: Some(vec![ToolCallResponse {
                        id: "call_0".into(),
                        call_type: "function".into(),
                        function: ToolCallFunction {
                            name: "read_file".into(),
                            arguments: "{\"path\":\"a.rs\"}".into(),
                        },
                    }]),
                    tool_call_id: None,
                },
                ChatMsg {
                    role: "tool".into(),
                    content: "file contents".into(),
                    tool_calls: None,
                    tool_call_id: Some("call_0".into()),
                },
            ],
            max_tokens: 100,
            temperature: 0.2,
            stop: None,
            tools: Some(sample_tools()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["tools"].as_array().unwrap().len(), 2);
        assert_eq!(json["messages"][1]["tool_calls"][0]["id"], "call_0");
        assert_eq!(json["messages"][2]["role"], "tool");
        assert_eq!(json["messages"][2]["tool_call_id"], "call_0");
        // Text messages must not carry null tool fields
        assert!(json["messages"][0].get("tool_calls").is_none());
    }

    // ─── Bedrock Runtime Converse API path ───────────────────────────────────

    fn claude_request(messages: Vec<ChatMessage>, cache_breakpoints: Vec<usize>) -> AiRequest {
        AiRequest {
            dynamic_tools: None,
            model: ModelConfig {
                provider: ProviderKind::Bedrock,
                model_id: "anthropic.claude-sonnet-5".into(),
                ..Default::default()
            },
            messages,
            stop: None,
            tools: Some(sample_tools()),
            cache_breakpoints,
        }
    }

    #[test]
    fn is_claude_model_detects_bedrock_claude_ids() {
        assert!(is_claude_model("anthropic.claude-sonnet-5"));
        assert!(is_claude_model("anthropic.claude-opus-4-8"));
        assert!(!is_claude_model("eu.amazon.nova-lite-v1:0"));
        assert!(!is_claude_model("qwen.qwen3-32b"));
        assert!(!is_claude_model("minimax.minimax-m2"));
    }

    #[test]
    fn is_inference_profile_required_error_detects_bedrock_message() {
        assert!(BedrockProvider::is_inference_profile_required_error(
            "Invocation of model ID anthropic.claude-sonnet-4-6 with on-demand throughput isn\u{2019}t supported. Retry your request with the ID or ARN of an inference profile that contains this model."
        ));
        assert!(!BedrockProvider::is_inference_profile_required_error(
            "The provided model identifier is invalid."
        ));
    }

    #[test]
    fn converse_cache_breakpoint_inserts_cache_point_after_system_block() {
        let request = claude_request(
            vec![
                ChatMessage { role: MessageRole::System, content: "You are an agent.".into(), tool_call_id: None, tool_calls: Vec::new() },
                ChatMessage { role: MessageRole::User, content: "hi".into(), tool_call_id: None, tool_calls: Vec::new() },
            ],
            vec![0],
        );
        let (system, messages) = BedrockProvider::build_converse_messages(&request);
        let system = system.expect("system block expected");
        assert_eq!(system.len(), 2);
        let json = serde_json::to_value(&system).unwrap();
        assert_eq!(json[0]["text"], "You are an agent.");
        assert_eq!(json[1]["cachePoint"]["type"], "default");

        // The unmarked user message must not pick up a cache marker.
        assert_eq!(messages[0].content.len(), 1);
    }

    #[test]
    fn converse_cache_breakpoint_inserts_cache_point_after_message_block() {
        let request = claude_request(
            vec![
                ChatMessage { role: MessageRole::System, content: "sys".into(), tool_call_id: None, tool_calls: Vec::new() },
                ChatMessage { role: MessageRole::User, content: "first turn".into(), tool_call_id: None, tool_calls: Vec::new() },
                ChatMessage { role: MessageRole::Assistant, content: "answer".into(), tool_call_id: None, tool_calls: Vec::new() },
                ChatMessage { role: MessageRole::User, content: "second turn".into(), tool_call_id: None, tool_calls: Vec::new() },
            ],
            vec![1], // "after first turn"
        );
        let (_, messages) = BedrockProvider::build_converse_messages(&request);
        assert_eq!(messages[0].content.len(), 2);
        let json = serde_json::to_value(&messages[0].content).unwrap();
        assert_eq!(json[0]["text"], "first turn");
        assert_eq!(json[1]["cachePoint"]["type"], "default");
        // Later messages stay unmarked.
        assert_eq!(messages[1].content.len(), 1);
        assert_eq!(messages[2].content.len(), 1);
    }

    /// Parallel tool results (multiple consecutive `Tool`-role messages from
    /// one assistant turn) must merge into a single `user` message with
    /// multiple `toolResult` blocks — the API requires this shape, and
    /// splitting them across messages trains the model to stop batching
    /// parallel tool calls.
    #[test]
    fn converse_merges_parallel_tool_results_into_one_user_turn() {
        let request = claude_request(
            vec![
                ChatMessage { role: MessageRole::User, content: "read both files".into(), tool_call_id: None, tool_calls: Vec::new() },
                ChatMessage {
                    role: MessageRole::Assistant,
                    content: "".into(),
                    tool_call_id: None,
                    tool_calls: vec![
                        ToolCallResponse { id: "call_0".into(), call_type: "function".into(), function: ToolCallFunction { name: "read_file".into(), arguments: "{\"path\":\"a.rs\"}".into() } },
                        ToolCallResponse { id: "call_1".into(), call_type: "function".into(), function: ToolCallFunction { name: "read_file".into(), arguments: "{\"path\":\"b.rs\"}".into() } },
                    ],
                },
                ChatMessage { role: MessageRole::Tool, content: "contents a".into(), tool_call_id: Some("call_0".into()), tool_calls: Vec::new() },
                ChatMessage { role: MessageRole::Tool, content: "contents b".into(), tool_call_id: Some("call_1".into()), tool_calls: Vec::new() },
                ChatMessage { role: MessageRole::User, content: "now compare them".into(), tool_call_id: None, tool_calls: Vec::new() },
            ],
            Vec::new(),
        );
        let (_, messages) = BedrockProvider::build_converse_messages(&request);
        // user, assistant(2 tool_use), user(2 tool_result merged), user
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].content.len(), 2);
        assert_eq!(messages[2].role, "user");
        assert_eq!(messages[2].content.len(), 2);
        let json = serde_json::to_value(&messages[2].content).unwrap();
        assert_eq!(json[0]["toolResult"]["toolUseId"], "call_0");
        assert_eq!(json[1]["toolResult"]["toolUseId"], "call_1");
        // The plain follow-up user turn is NOT merged into the tool-result turn.
        assert_eq!(messages[3].role, "user");
        assert_eq!(messages[3].content.len(), 1);
    }

    #[test]
    fn converse_tools_convert_to_tool_spec_shape() {
        let request = claude_request(
            vec![ChatMessage { role: MessageRole::User, content: "hi".into(), tool_call_id: None, tool_calls: Vec::new() }],
            Vec::new(),
        );
        let tool_config = BedrockProvider::converse_tools(&request).unwrap();
        let json = serde_json::to_value(&tool_config.tools).unwrap();
        assert_eq!(json[0]["toolSpec"]["name"], "read_file");
        assert_eq!(json[0]["toolSpec"]["description"], "Read a file");
        assert!(json[0]["toolSpec"]["inputSchema"]["json"]["properties"]["path"].is_object());
        // OpenAI wrapper fields must not leak through.
        assert!(json[0].get("type").is_none());
        assert!(json[0]["toolSpec"].get("parameters").is_none());
    }

    #[test]
    fn converse_response_parses_cache_usage_and_tool_use() {
        let body = r#"{
            "output": {
                "message": {
                    "role": "assistant",
                    "content": [
                        {"text": "Let me check."},
                        {"toolUse": {"toolUseId": "toolu_1", "name": "get_weather", "input": {"city": "Paris"}}}
                    ]
                }
            },
            "stopReason": "tool_use",
            "usage": {
                "inputTokens": 12,
                "outputTokens": 8,
                "cacheWriteInputTokens": 0,
                "cacheReadInputTokens": 500
            }
        }"#;
        let parsed: ConverseResponse = serde_json::from_str(body).unwrap();
        let content = &parsed.output.as_ref().unwrap().message.as_ref().unwrap().content;
        assert_eq!(content.len(), 2);
        let usage = parsed.usage.unwrap();
        assert_eq!(usage.cache_read_input_tokens, 500);
        // Emulate what complete_anthropic does with this usage: cached_tokens
        // must be a *subset* of input_tokens (estimate_cost's assumption),
        // not double-counted or reported separately.
        let total_input = usage.input_tokens + usage.cache_read_input_tokens + usage.cache_write_input_tokens;
        assert_eq!(total_input, 512);
        assert!(usage.cache_read_input_tokens <= total_input);
    }
}
