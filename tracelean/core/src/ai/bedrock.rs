//! Amazon Bedrock provider implementation.
//! Uses the Chat Completions API (OpenAI-compatible) on bedrock-runtime endpoint.
//! Supports Bearer token authentication (AWS_BEARER_TOKEN_BEDROCK).
//! This works for ALL Bedrock models and supports native tool calling.

use super::provider::*;
use super::tracking::TokenUsage;
use super::bedrock_pricing::bedrock_pricing;
use serde::{Deserialize, Serialize};

/// Default region for Bedrock API.
pub const DEFAULT_REGION: &str = "us-east-1";
/// Default model ID (mantle format — no version suffix, no cross-region prefix).
pub const DEFAULT_MODEL_ID: &str = "qwen.qwen3-32b";

pub struct BedrockProvider {
    bearer_token: String,
    region: String,
    client: reqwest::Client,
    max_retries: u32,
}

impl BedrockProvider {
    /// Create with bearer token and optional region (defaults to eu-west-1).
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
        }
    }

    /// Chat Completions endpoint (OpenAI-compatible).
    /// Uses bedrock-mantle (recommended) which supports bearer token auth for all models.
    fn chat_completions_endpoint(&self) -> String {
        format!(
            "https://bedrock-mantle.{}.api.aws/v1/chat/completions",
            self.region
        )
    }

    /// Convert bedrock-runtime model IDs to bedrock-mantle format.
    ///
    /// bedrock-runtime ListFoundationModels returns IDs like:
    ///   qwen.qwen3-coder-30b-a3b-v1:0
    ///   qwen.qwen3-32b-v1:0
    ///   eu.amazon.nova-lite-v1:0
    ///
    /// bedrock-mantle uses the same base ID but without version suffixes.
    /// Some models already have the mantle-style ID (from settings).
    ///
    /// Strategy: strip trailing version patterns like "-v1:0", ":0".
    /// Don't transform anything else — let the endpoint validate.
    pub fn to_mantle_model_id(model_id: &str) -> String {
        // Strip leading cross-region prefix (eu., us., ap-.) for inference IDs
        let id = if let Some(rest) = model_id.strip_prefix("eu.")
            .or_else(|| model_id.strip_prefix("us."))
        {
            // Only strip if followed by a provider prefix (amazon., anthropic., etc)
            // Don't strip if it's already the provider format (e.g. "qwen.xxx")
            if rest.starts_with("amazon.") || rest.starts_with("anthropic.") || rest.starts_with("meta.") {
                rest
            } else {
                model_id
            }
        } else {
            model_id
        };

        // Strip "-v1:0" or similar version suffixes
        // Pattern: ends with "-v{digit}:{digit}" or just ":{digit}"
        if let Some(pos) = id.rfind("-v1:") {
            return id[..pos].to_string();
        }
        if let Some(pos) = id.rfind(":0") {
            // Only strip if it looks like a version suffix (after alphanumeric)
            if pos > 0 && id.as_bytes()[pos - 1].is_ascii_alphanumeric() {
                return id[..pos].to_string();
            }
        }

        id.to_string()
    }

    fn list_models_endpoint(&self) -> String {
        format!(
            "https://bedrock-mantle.{}.api.aws/v1/models",
            self.region
        )
    }
}

// ---------------------------------------------------------------------------
// Chat Completions API types (OpenAI-compatible)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CcRequest {
    model: String,
    messages: Vec<CcMessage>,
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolSchema>>,
}

#[derive(Serialize)]
struct CcMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<CcOutToolCall>>,
}

#[derive(Serialize)]
struct CcOutToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: CcOutToolCallFn,
}

#[derive(Serialize)]
struct CcOutToolCallFn {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct CcResponse {
    choices: Vec<CcChoice>,
    usage: Option<CcUsage>,
}

#[derive(Deserialize)]
struct CcChoice {
    message: CcChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct CcChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<CcToolCall>>,
}

#[derive(Deserialize)]
struct CcToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<CcToolCallFunction>,
}

#[derive(Deserialize)]
struct CcToolCallFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct CcUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<CcPromptTokensDetails>,
}

#[derive(Deserialize, Default)]
struct CcPromptTokensDetails {
    cached_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// ListModels response types (bedrock-mantle /v1/models — OpenAI format)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct MantleListModelsResponse {
    data: Vec<MantleModelEntry>,
}

#[derive(Deserialize)]
struct MantleModelEntry {
    id: String,
    #[serde(default)]
    status: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider implementation
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl AiProvider for BedrockProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        // Build messages in OpenAI Chat Completions format
        let messages: Vec<CcMessage> = request.messages.iter().map(|m| {
            let tool_calls_out = if m.role == MessageRole::Assistant && !m.tool_calls.is_empty() {
                Some(m.tool_calls.iter().map(|tc| CcOutToolCall {
                    id: tc.id.clone(),
                    call_type: "function".into(),
                    function: CcOutToolCallFn {
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    },
                }).collect())
            } else {
                None
            };

            CcMessage {
                role: match m.role {
                    MessageRole::System => "system".into(),
                    MessageRole::User => "user".into(),
                    MessageRole::Assistant => "assistant".into(),
                    MessageRole::Tool => "tool".into(),
                },
                content: if m.content.is_empty() && tool_calls_out.is_some() {
                    None // OpenAI format: assistant with tool_calls can have null content
                } else {
                    Some(m.content.clone())
                },
                tool_call_id: m.tool_call_id.clone(),
                tool_calls: tool_calls_out,
            }
        }).collect();

        let body = CcRequest {
            model: Self::to_mantle_model_id(&request.model.model_id),
            messages,
            max_tokens: request.model.max_tokens,
            temperature: request.model.temperature,
            stop: request.stop.clone(),
            tools: request.tools.clone(),
        };

        let url = self.chat_completions_endpoint();

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
                            message: "Throttled by Bedrock".into(),
                            retryable: true,
                        });
                        continue;
                    }

                    if status == 401 || status == 403 {
                        return Err(AiError {
                            kind: AiErrorKind::Authentication,
                            message: format!("Auth failed ({}): {}", status, &raw_text[..raw_text.len().min(200)]),
                            retryable: false,
                        });
                    }

                    if !status.is_success() {
                        let retryable = status.is_server_error();
                        last_err = Some(AiError {
                            kind: AiErrorKind::ProviderError,
                            message: format!("HTTP {}: {}", status, &raw_text[..raw_text.len().min(200)]),
                            retryable,
                        });
                        if !retryable { return Err(last_err.unwrap()); }
                        continue;
                    }

                    let parsed: CcResponse = serde_json::from_str(&raw_text).map_err(|e| AiError {
                        kind: AiErrorKind::ProviderError,
                        message: format!("Parse error: {} | body: {}", e, &raw_text[..raw_text.len().min(300)]),
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
                    let tool_calls: Vec<ToolCallResponse> = choice.message.tool_calls
                        .as_ref()
                        .map(|calls| {
                            calls.iter().enumerate().filter_map(|(i, tc)| {
                                let func = tc.function.as_ref()?;
                                Some(ToolCallResponse {
                                    id: tc.id.clone().unwrap_or_else(|| format!("call_{}", i)),
                                    call_type: tc.call_type.clone().unwrap_or_else(|| "function".into()),
                                    function: ToolCallFunction {
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
        "Amazon Bedrock"
    }

    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError> {
        let url = self.list_models_endpoint();

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

        let body: MantleListModelsResponse = resp.json().await.map_err(|e| AiError {
            kind: AiErrorKind::ProviderError,
            message: format!("Parse models: {}", e),
            retryable: false,
        })?;

        let models = body.data.into_iter()
            .filter(|m| m.status.as_deref() != Some("unavailable"))
            .map(|m| {
                let (input_cost, output_cost) = bedrock_pricing(&m.id);
                let (display, max_tok) = bedrock_model_meta(&m.id);
                ModelConfig {
                    provider: ProviderKind::Bedrock,
                    model_id: m.id.clone(),
                    display_name: display.unwrap_or_else(|| m.id.clone()),
                    max_tokens: max_tok,
                    temperature: 0.3,
                    input_cost_per_m: input_cost,
                    output_cost_per_m: output_cost,
                    cached_input_cost_per_m: input_cost * 0.1,
                    extra_params: None,
                }
            })
            .collect();

        Ok(models)
    }
}

/// Returns (display_name, max_tokens) for known Bedrock models.
fn bedrock_model_meta(id: &str) -> (Option<String>, u32) {
    match id {
        "amazon.nova-micro" => (Some("Nova Micro (cache✓)".into()), 5120),
        "amazon.nova-lite" => (Some("Nova Lite (cache✓)".into()), 5120),
        "amazon.nova-pro" => (Some("Nova Pro (cache✓)".into()), 5120),
        "amazon.nova-premier" => (Some("Nova Premier (cache✓)".into()), 5120),
        "anthropic.claude-sonnet-5" => (Some("Claude Sonnet 5 (cache✓)".into()), 8192),
        "anthropic.claude-haiku-4-5" => (Some("Claude Haiku 4.5 (cache✓)".into()), 8192),
        "anthropic.claude-opus-4-7" => (Some("Claude Opus 4.7".into()), 8192),
        "anthropic.claude-opus-4-8" => (Some("Claude Opus 4.8".into()), 8192),
        "anthropic.claude-fable-5" => (Some("Claude Fable 5".into()), 8192),
        _ => (None, 4096),
    }
}

// end of provider implementation
