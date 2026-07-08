//! Amazon Bedrock provider implementation.
//! Uses the Bedrock Converse API with Bearer token authentication.
//! No SigV4 signing needed — uses AWS_BEARER_TOKEN_BEDROCK.

use super::provider::*;
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

    fn converse_endpoint(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse",
            self.region, model_id
        )
    }

    fn list_models_endpoint(&self) -> String {
        format!(
            "https://bedrock.{}.amazonaws.com/foundation-models",
            self.region
        )
    }
}

/// Bedrock Converse API request.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrConverseRequest {
    messages: Vec<BrMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<BrSystemBlock>>,
    inference_config: BrInferenceConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_config: Option<BrToolConfig>,
}

/// Bedrock tool configuration.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrToolConfig {
    tools: Vec<BrToolDef>,
}

/// A single tool definition for Bedrock.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrToolDef {
    tool_spec: BrToolSpec,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrToolSpec {
    name: String,
    description: String,
    input_schema: BrInputSchema,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrInputSchema {
    json: serde_json::Value,
}

#[derive(Serialize)]
struct BrMessage {
    role: String,
    content: Vec<BrContentBlock>,
}

#[derive(Serialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum BrContentBlock {
    Text { text: String },
    ToolResult { tool_result: BrToolResultBlock },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct BrToolResultBlock {
    tool_use_id: String,
    content: Vec<BrToolResultContent>,
}

#[derive(Serialize)]
#[allow(dead_code)]
struct BrToolResultContent {
    text: String,
}

#[derive(Serialize)]
struct BrSystemBlock {
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BrInferenceConfig {
    max_tokens: u32,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

/// Bedrock Converse API response.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrConverseResponse {
    output: BrOutput,
    usage: BrUsage,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct BrOutput {
    message: Option<BrOutMessage>,
}

#[derive(Deserialize)]
struct BrOutMessage {
    content: Vec<BrOutContent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrOutContent {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tool_use: Option<BrToolUse>,
    #[serde(default)]
    reasoning_content: Option<BrReasoningContent>,
}

/// Reasoning content block from Bedrock Converse API (extended thinking).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrReasoningContent {
    #[serde(default)]
    reasoning_text: Option<BrReasoningText>,
    /// Encrypted/redacted content (base64) — we just note its presence.
    #[serde(default)]
    redacted_content: Option<String>,
}

#[derive(Deserialize)]
struct BrReasoningText {
    text: String,
    #[serde(default)]
    #[allow(dead_code)]
    signature: Option<String>,
}

/// Tool use block in Bedrock response.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrToolUse {
    tool_use_id: String,
    name: String,
    input: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct BrUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    total_tokens: Option<u32>,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
    #[serde(default)]
    cache_write_input_tokens: Option<u32>,
}

/// Bedrock ListFoundationModels response.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrListModelsResponse {
    model_summaries: Vec<BrModelSummary>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrModelSummary {
    model_id: String,
    model_name: Option<String>,
    #[serde(default)]
    inference_types_supported: Vec<String>,
}

#[async_trait::async_trait]
impl AiProvider for BedrockProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        // Separate system message from conversation
        let system_msgs: Vec<BrSystemBlock> = request.messages.iter()
            .filter(|m| m.role == MessageRole::System)
            .map(|m| BrSystemBlock { text: m.content.clone() })
            .collect();

        // Build messages, converting Tool messages to user messages with toolResult blocks
        let mut messages: Vec<BrMessage> = Vec::new();
        let mut pending_tool_results: Vec<BrContentBlock> = Vec::new();

        for m in request.messages.iter().filter(|m| m.role != MessageRole::System) {
            match m.role {
                MessageRole::Tool => {
                    // Accumulate tool results — they'll be sent as a user message
                    let tool_call_id = m.tool_call_id.clone().unwrap_or_else(|| "unknown".into());
                    pending_tool_results.push(BrContentBlock::ToolResult {
                        tool_result: BrToolResultBlock {
                            tool_use_id: tool_call_id,
                            content: vec![BrToolResultContent { text: m.content.clone() }],
                        },
                    });
                }
                _ => {
                    // If we have pending tool results, flush them as a user message first
                    if !pending_tool_results.is_empty() {
                        messages.push(BrMessage {
                            role: "user".into(),
                            content: std::mem::take(&mut pending_tool_results),
                        });
                    }
                    messages.push(BrMessage {
                        role: match m.role {
                            MessageRole::User => "user".into(),
                            MessageRole::Assistant => "assistant".into(),
                            _ => "user".into(),
                        },
                        content: vec![BrContentBlock::Text { text: m.content.clone() }],
                    });
                }
            }
        }
        // Flush any remaining tool results
        if !pending_tool_results.is_empty() {
            messages.push(BrMessage {
                role: "user".into(),
                content: pending_tool_results,
            });
        }

        // Convert ToolSchema to Bedrock toolConfig format
        let tool_config = request.tools.as_ref().and_then(|tools| {
            if tools.is_empty() { return None; }
            let br_tools: Vec<BrToolDef> = tools.iter().map(|ts| {
                BrToolDef {
                    tool_spec: BrToolSpec {
                        name: ts.function.name.clone(),
                        description: ts.function.description.clone(),
                        input_schema: BrInputSchema {
                            json: ts.function.parameters.clone(),
                        },
                    },
                }
            }).collect();
            Some(BrToolConfig { tools: br_tools })
        });

        let body = BrConverseRequest {
            messages,
            system: if system_msgs.is_empty() { None } else { Some(system_msgs) },
            inference_config: BrInferenceConfig {
                max_tokens: request.model.max_tokens,
                temperature: request.model.temperature,
                stop_sequences: request.stop.clone(),
            },
            tool_config,
        };

        let url = self.converse_endpoint(&request.model.model_id);

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
            }

            let resp = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.bearer_token))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
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

                    let parsed: BrConverseResponse = serde_json::from_str(&raw_text).map_err(|e| AiError {
                        kind: AiErrorKind::ProviderError,
                        message: format!("Parse error: {}", e),
                        retryable: false,
                    })?;

                    let mut content = String::new();
                    let mut tool_calls = Vec::new();
                    let mut thinking_text_len: usize = 0;

                    if let Some(msg) = parsed.output.message {
                        for block in msg.content {
                            if let Some(text) = block.text {
                                if !content.is_empty() {
                                    content.push('\n');
                                }
                                content.push_str(&text);
                            }
                            if let Some(tu) = block.tool_use {
                                tool_calls.push(super::provider::ToolCallResponse {
                                    id: tu.tool_use_id,
                                    call_type: "function".into(),
                                    function: super::provider::ToolCallFunction {
                                        name: tu.name,
                                        arguments: serde_json::to_string(&tu.input).unwrap_or_else(|_| "{}".into()),
                                    },
                                });
                            }
                            if let Some(rc) = block.reasoning_content {
                                if let Some(rt) = rc.reasoning_text {
                                    thinking_text_len += rt.text.len();
                                } else if rc.redacted_content.is_some() {
                                    // Redacted reasoning — we can't measure but note presence
                                    thinking_text_len += 100; // minimal estimate
                                }
                            }
                        }
                    }

                    let truncated = parsed.stop_reason.as_deref() == Some("max_tokens");

                    // Estimate thinking tokens: ~4 chars per token (rough heuristic).
                    // Bedrock Converse API includes thinking in outputTokens, so we
                    // estimate thinking_tokens as a subset of output_tokens.
                    let estimated_thinking_tokens = if thinking_text_len > 0 {
                        let estimate = (thinking_text_len as u32) / 4;
                        // Don't exceed output_tokens
                        estimate.min(parsed.usage.output_tokens)
                    } else {
                        0
                    };

                    let token_usage = TokenUsage {
                        input_tokens: parsed.usage.input_tokens,
                        output_tokens: parsed.usage.output_tokens,
                        thinking_tokens: estimated_thinking_tokens,
                        cached_tokens: parsed.usage.cache_read_input_tokens.unwrap_or(0),
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

        let body: BrListModelsResponse = resp.json().await.map_err(|e| AiError {
            kind: AiErrorKind::ProviderError,
            message: format!("Parse models: {}", e),
            retryable: false,
        })?;

        // Fetch pricing from AWS public bulk pricing API (no auth needed)
        let pricing = fetch_bedrock_pricing(&self.client, &self.region).await;

        let models = body.model_summaries.into_iter()
            .filter(|m| m.inference_types_supported.iter().any(|t| t == "ON_DEMAND"))
            .map(|m| {
                let (input_cost, output_cost) = pricing.get(&m.model_id)
                    .copied()
                    .unwrap_or((0.0, 0.0));
                ModelConfig {
                    provider: ProviderKind::Bedrock,
                    model_id: m.model_id.clone(),
                    display_name: m.model_name.unwrap_or_else(|| m.model_id.clone()),
                    max_tokens: 4096,
                    temperature: 0.3,
                    input_cost_per_m: input_cost,
                    output_cost_per_m: output_cost,
                    cached_input_cost_per_m: input_cost * 0.1, // estimate: ~10% of input for cached
                    extra_params: None,
                }
            })
            .collect();

        Ok(models)
    }
}

// ---------------------------------------------------------------------------
// Pricing fetch from AWS public bulk pricing API
// ---------------------------------------------------------------------------

/// Fetch per-model pricing from AWS public pricing endpoint (no auth needed).
/// Returns map of model_id -> (input_cost_per_1M_tokens, output_cost_per_1M_tokens).
async fn fetch_bedrock_pricing(
    client: &reqwest::Client,
    region: &str,
) -> std::collections::HashMap<String, (f64, f64)> {
    let mut pricing = std::collections::HashMap::new();

    // AWS public pricing bulk API — region-specific file
    let pricing_url = format!(
        "https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonBedrock/current/{}/index.json",
        region
    );

    let resp = match client
        .get(&pricing_url)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return pricing, // Silently fail — pricing is optional
    };

    let body = match resp.text().await {
        Ok(t) => t,
        Err(_) => return pricing,
    };

    // Parse the pricing JSON — structure:
    // { "products": { "<sku>": { "attributes": { "model": "...", "usagetype": "..." } } },
    //   "terms": { "OnDemand": { "<sku>": { "<offerTermCode>": { "priceDimensions": { ... } } } } } }
    let val: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return pricing,
    };

    let products = match val.get("products").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return pricing,
    };
    let terms = match val.get("terms")
        .and_then(|t| t.get("OnDemand"))
        .and_then(|o| o.as_object())
    {
        Some(t) => t,
        None => return pricing,
    };

    // Build sku -> (model_id, is_input) map
    let mut sku_map: std::collections::HashMap<String, (String, bool)> = std::collections::HashMap::new();
    for (sku, product) in products {
        let attrs = match product.get("attributes").and_then(|a| a.as_object()) {
            Some(a) => a,
            None => continue,
        };
        let model_id = match attrs.get("model").or(attrs.get("modelId")).and_then(|m| m.as_str()) {
            Some(m) => m.to_string(),
            None => continue,
        };
        let usage_type = attrs.get("usagetype").and_then(|u| u.as_str()).unwrap_or("");
        let is_input = usage_type.contains("Input") || usage_type.contains("input");
        sku_map.insert(sku.clone(), (model_id, is_input));
    }

    // Extract prices per SKU from terms
    for (sku, (model_id, is_input)) in &sku_map {
        if let Some(term) = terms.get(sku).and_then(|t| t.as_object()) {
            for (_offer_code, offer) in term {
                if let Some(dims) = offer.get("priceDimensions").and_then(|d| d.as_object()) {
                    for (_dim_key, dim) in dims {
                        let price_str = dim.get("pricePerUnit")
                            .and_then(|p| p.get("USD"))
                            .and_then(|u| u.as_str())
                            .unwrap_or("0");
                        let price_per_unit: f64 = price_str.parse().unwrap_or(0.0);
                        // Price is per token; convert to per-million
                        let price_per_m = price_per_unit * 1_000_000.0;

                        let entry = pricing.entry(model_id.clone()).or_insert((0.0, 0.0));
                        if *is_input {
                            entry.0 = price_per_m;
                        } else {
                            entry.1 = price_per_m;
                        }
                    }
                }
            }
        }
    }

    pricing
}
