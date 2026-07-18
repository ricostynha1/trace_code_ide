//! OpenRouter HTTP provider implementation.

use super::provider::*;
use super::tracking::TokenUsage;
use serde::{Deserialize, Serialize};

pub struct OpenRouterProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
    timeout_secs: u64,
    max_retries: u32,
}

impl OpenRouterProvider {
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            api_key,
            base_url: "https://openrouter.ai/api/v1".to_string(),
            client,
            timeout_secs: 120,
            max_retries: 3,
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self.client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(secs))
            .build()
            .unwrap_or_default();
        self
    }
}

/// OpenRouter request body (OpenAI-compatible).
/// Field order matters for prefix caching: tools → messages maximizes cache hits.
/// MiniMax docs: "prefix matching constructed in order: tool list → system prompts → user messages"
#[derive(Serialize)]
struct OrRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<super::provider::ToolSchema>>,
    messages: Vec<OrMessage>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
}

#[derive(Serialize)]
struct OrMessage {
    role: String,
    content: OrContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    /// Structured tool calls on assistant history messages (P9a parity —
    /// strict providers reject tool results without them).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<super::provider::ToolCallResponse>>,
}

/// Message content: plain string normally; content blocks when the message
/// carries an explicit cache marker (P9b, D9b.2 — Anthropic-style
/// `cache_control` on the last block, passed through by OpenRouter).
#[derive(Serialize)]
#[serde(untagged)]
enum OrContent {
    Text(String),
    Blocks(Vec<OrContentBlock>),
}

#[derive(Serialize)]
struct OrContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<serde_json::Value>,
}

/// Build the wire messages, applying cache markers at the given indexes.
fn build_or_messages(request: &AiRequest) -> Vec<OrMessage> {
    request
        .messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let marked = request.cache_breakpoints.contains(&i);
            let content = if marked {
                OrContent::Blocks(vec![OrContentBlock {
                    block_type: "text".into(),
                    text: m.content.clone(),
                    cache_control: Some(serde_json::json!({"type": "ephemeral"})),
                }])
            } else {
                OrContent::Text(m.content.clone())
            };
            OrMessage {
                role: match m.role {
                    MessageRole::System => "system".into(),
                    MessageRole::User => "user".into(),
                    MessageRole::Assistant => "assistant".into(),
                    MessageRole::Tool => "tool".into(),
                },
                content,
                tool_call_id: m.tool_call_id.clone(),
                tool_calls: if m.tool_calls.is_empty() {
                    None
                } else {
                    Some(m.tool_calls.clone())
                },
            }
        })
        .collect()
}

/// OpenRouter response body.
#[derive(Deserialize)]
struct OrResponse {
    choices: Vec<OrChoice>,
    usage: Option<OrUsage>,
}

#[derive(Deserialize)]
struct OrChoice {
    message: OrChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OrChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OrToolCall>>,
}

/// Tool call in OpenAI-compatible response.
#[derive(Deserialize)]
struct OrToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<OrToolCallFunction>,
}

#[derive(Deserialize)]
struct OrToolCallFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct OrUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<OrPromptDetails>,
}

#[derive(Deserialize, Default)]
struct OrPromptDetails {
    cached_tokens: Option<u32>,
}

/// OpenRouter /models response.
#[derive(Deserialize)]
struct OrModelsResponse {
    data: Vec<OrModelEntry>,
}

#[derive(Deserialize)]
struct OrModelEntry {
    id: String,
    name: Option<String>,
    context_length: Option<u64>,
    pricing: Option<OrPricing>,
}

#[derive(Deserialize, Default)]
struct OrPricing {
    prompt: Option<String>,
    completion: Option<String>,
}

/// Parse OpenRouter price string (per-token) → per-million-tokens USD.
fn parse_price_per_m(price_str: &Option<String>) -> f64 {
    match price_str {
        Some(s) => s.parse::<f64>().unwrap_or(0.0) * 1_000_000.0,
        None => 0.0,
    }
}

#[async_trait::async_trait]
impl AiProvider for OpenRouterProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let messages = build_or_messages(request);

        let body = OrRequest {
            model: request.model.model_id.clone(),
            tools: request.tools.clone(),
            messages,
            max_tokens: request.model.max_tokens,
            temperature: request.model.temperature,
            stop: request.stop.clone(),
        };

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
            }

            let resp = self.client
                .post(format!("{}/chat/completions", self.base_url))
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .header("HTTP-Referer", "https://tracelean.dev")
                .header("X-Title", "TraceLean IDE")
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
                            message: "Rate limited".into(),
                            retryable: true,
                        });
                        continue;
                    }

                    if status == 401 || status == 403 {
                        return Err(AiError {
                            kind: AiErrorKind::Authentication,
                            message: format!("Auth failed: {}", status),
                            retryable: false,
                        });
                    }

                    if !status.is_success() {
                        last_err = Some(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("HTTP {}: {}", status, &raw_text[..raw_text.len().min(200)]),
                            retryable: status.is_server_error(),
                        });
                        if !status.is_server_error() {
                            return Err(last_err.unwrap());
                        }
                        continue;
                    }

                    let parsed: OrResponse = serde_json::from_str(&raw_text).map_err(|e| AiError {
                        kind: AiErrorKind::ProviderError,
                        message: format!("Parse error: {}", e),
                        retryable: false,
                    })?;

                    let choice = parsed.choices.first().ok_or(AiError {
                        kind: AiErrorKind::ProviderError,
                        message: "No choices in response".into(),
                        retryable: false,
                    })?;

                    let content = choice.message.content.clone().unwrap_or_default();
                    let truncated = choice.finish_reason.as_deref() == Some("length");

                    // Parse tool_calls from response
                    let tool_calls: Vec<super::provider::ToolCallResponse> = choice.message.tool_calls
                        .as_ref()
                        .map(|calls| {
                            calls.iter().enumerate().filter_map(|(i, tc)| {
                                let func = tc.function.as_ref()?;
                                Some(super::provider::ToolCallResponse {
                                    id: tc.id.clone().unwrap_or_else(|| format!("call_{}", i)),
                                    call_type: tc.call_type.clone().unwrap_or_else(|| "function".into()),
                                    function: super::provider::ToolCallFunction {
                                        name: func.name.clone().unwrap_or_default(),
                                        arguments: func.arguments.clone().unwrap_or_else(|| "{}".into()),
                                    },
                                })
                            }).collect()
                        })
                        .unwrap_or_default();

                    let usage = parsed.usage.as_ref();
                    let token_usage = TokenUsage {
                        input_tokens: usage.and_then(|u| u.prompt_tokens).unwrap_or(0),
                        output_tokens: usage.and_then(|u| u.completion_tokens).unwrap_or(0),
                        thinking_tokens: 0,
                        cached_tokens: usage
                            .and_then(|u| u.prompt_tokens_details.as_ref())
                            .and_then(|d| d.cached_tokens)
                            .unwrap_or(0),
                    };

                    return Ok(AiResponse {
                        content,
                        usage: token_usage,
                        raw_response: Some(raw_text),
                        truncated,
                        tool_calls,
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
        "OpenRouter"
    }

    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError> {
        let resp = self.client
            .get(format!("{}/models", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| AiError {
                kind: AiErrorKind::Network,
                message: e.to_string(),
                retryable: true,
            })?;

        if !resp.status().is_success() {
            return Err(AiError {
                kind: AiErrorKind::ProviderError,
                message: format!("HTTP {} fetching models", resp.status()),
                retryable: false,
            });
        }

        let body: OrModelsResponse = resp.json().await.map_err(|e| AiError {
            kind: AiErrorKind::ProviderError,
            message: format!("Parse models: {}", e),
            retryable: false,
        })?;

        let models = body.data.into_iter().map(|m| {
            let pricing = m.pricing.unwrap_or_default();
            ModelConfig {
                provider: ProviderKind::OpenRouter,
                model_id: m.id.clone(),
                display_name: m.name.unwrap_or(m.id),
                max_tokens: m.context_length.unwrap_or(4096).min(8192) as u32,
                temperature: 0.3,
                input_cost_per_m: parse_price_per_m(&pricing.prompt),
                output_cost_per_m: parse_price_per_m(&pricing.completion),
                cached_input_cost_per_m: parse_price_per_m(&pricing.prompt) * 0.1, // estimate ~10% of input price for cached
                extra_params: None,
                coding_index: None,
                coding_rank: None,
                supports_caching: false,
                supports_tools: false,
                ..Default::default()
            }
        }).collect();

        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::provider::{ToolCallFunction, ToolCallResponse};

    fn msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage { role, content: content.into(), tool_call_id: None, tool_calls: Vec::new() }
    }

    #[test]
    fn cache_breakpoints_become_cache_control_blocks() {
        let request = AiRequest {
            model: ModelConfig::default(),
            messages: vec![
                msg(MessageRole::System, "system prompt"),
                msg(MessageRole::User, "question"),
            ],
            stop: None,
            tools: None,
            cache_breakpoints: vec![0],
        };
        let wire = build_or_messages(&request);
        let json = serde_json::to_value(&wire).unwrap();
        // Marked message → content blocks with ephemeral cache_control
        assert_eq!(json[0]["content"][0]["type"], "text");
        assert_eq!(json[0]["content"][0]["text"], "system prompt");
        assert_eq!(json[0]["content"][0]["cache_control"]["type"], "ephemeral");
        // Unmarked message → plain string content
        assert_eq!(json[1]["content"], "question");
        assert!(json[1].get("cache_control").is_none());
    }

    #[test]
    fn assistant_history_keeps_structured_tool_calls() {
        let mut assistant = msg(MessageRole::Assistant, "");
        assistant.tool_calls = vec![ToolCallResponse {
            id: "call_0".into(),
            call_type: "function".into(),
            function: ToolCallFunction { name: "read_file".into(), arguments: "{\"path\":\"a.rs\"}".into() },
        }];
        let mut tool_result = msg(MessageRole::Tool, "contents");
        tool_result.tool_call_id = Some("call_0".into());

        let request = AiRequest {
            model: ModelConfig::default(),
            messages: vec![msg(MessageRole::User, "read a.rs"), assistant, tool_result],
            stop: None,
            tools: None,
            cache_breakpoints: Vec::new(),
        };
        let json = serde_json::to_value(build_or_messages(&request)).unwrap();
        assert_eq!(json[1]["tool_calls"][0]["id"], "call_0");
        assert_eq!(json[1]["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(json[2]["role"], "tool");
        assert_eq!(json[2]["tool_call_id"], "call_0");
        // No tool_calls key leaks onto messages without calls
        assert!(json[0].get("tool_calls").is_none());
    }
}
