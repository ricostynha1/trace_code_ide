//! Amazon Bedrock provider implementation.
//! Uses the Bedrock Converse API via direct HTTP (no AWS SDK dependency).

use super::provider::*;
use super::tracking::TokenUsage;
use serde::{Deserialize, Serialize};

pub struct BedrockProvider {
    #[allow(dead_code)]
    access_key: String,
    #[allow(dead_code)]
    secret_key: String,
    region: String,
    client: reqwest::Client,
    max_retries: u32,
}

impl BedrockProvider {
    pub fn new(access_key: String, secret_key: String, region: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        Self {
            access_key,
            secret_key,
            region,
            client,
            max_retries: 3,
        }
    }

    /// Generate AWS SigV4 authorization header.
    /// Simplified — in production you'd use a proper signing lib.
    fn sign_request(&self, method: &str, url: &str, body: &[u8], service: &str) -> Vec<(String, String)> {
        // NOTE: This is a placeholder. Real implementation needs full AWS SigV4.
        // For now, we include auth info that a proper signer would produce.
        // TODO: integrate aws-sigv4 crate for proper signing.
        let _ = (method, url, body, service);
        vec![
            ("x-amz-date".into(), chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()),
            // Authorization header would go here after proper signing
        ]
    }

    fn endpoint(&self, model_id: &str) -> String {
        format!(
            "https://bedrock-runtime.{}.amazonaws.com/model/{}/converse",
            self.region, model_id
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
}

#[derive(Serialize)]
struct BrMessage {
    role: String,
    content: Vec<BrContentBlock>,
}

#[derive(Serialize)]
struct BrContentBlock {
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
struct BrOutContent {
    text: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: Option<u32>,
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

        let messages: Vec<BrMessage> = request.messages.iter()
            .filter(|m| m.role != MessageRole::System)
            .map(|m| BrMessage {
                role: match m.role {
                    MessageRole::User => "user".into(),
                    MessageRole::Assistant => "assistant".into(),
                    _ => "user".into(),
                },
                content: vec![BrContentBlock { text: m.content.clone() }],
            })
            .collect();

        let body = BrConverseRequest {
            messages,
            system: if system_msgs.is_empty() { None } else { Some(system_msgs) },
            inference_config: BrInferenceConfig {
                max_tokens: request.model.max_tokens,
                temperature: request.model.temperature,
                stop_sequences: request.stop.clone(),
            },
        };

        let body_bytes = serde_json::to_vec(&body).map_err(|e| AiError {
            kind: AiErrorKind::InvalidRequest,
            message: format!("Serialize error: {}", e),
            retryable: false,
        })?;

        let url = self.endpoint(&request.model.model_id);
        let _headers = self.sign_request("POST", &url, &body_bytes, "bedrock");

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
            }

            let mut req_builder = self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json");

            // Apply auth headers
            for (k, v) in self.sign_request("POST", &url, &body_bytes, "bedrock") {
                req_builder = req_builder.header(&k, &v);
            }

            let resp = req_builder.body(body_bytes.clone()).send().await;

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

                    let content = parsed.output.message
                        .and_then(|m| m.content.into_iter().next())
                        .and_then(|c| c.text)
                        .unwrap_or_default();

                    let truncated = parsed.stop_reason.as_deref() == Some("max_tokens");

                    let token_usage = TokenUsage {
                        input_tokens: parsed.usage.input_tokens,
                        output_tokens: parsed.usage.output_tokens,
                        thinking_tokens: 0,
                        cached_tokens: parsed.usage.cache_read_input_tokens.unwrap_or(0),
                    };

                    return Ok(AiResponse {
                        content,
                        usage: token_usage,
                        raw_response: Some(raw_text),
                        truncated,
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
        // Query Bedrock ListFoundationModels API
        let url = format!(
            "https://bedrock.{}.amazonaws.com/foundation-models",
            self.region
        );

        let _headers = self.sign_request("GET", &url, &[], "bedrock");

        let mut req_builder = self.client
            .get(&url)
            .header("Accept", "application/json");

        for (k, v) in self.sign_request("GET", &url, &[], "bedrock") {
            req_builder = req_builder.header(&k, &v);
        }

        let resp = req_builder.send().await.map_err(|e| AiError {
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

        let body: BrListModelsResponse = resp.json().await.map_err(|e| AiError {
            kind: AiErrorKind::ProviderError,
            message: format!("Parse models: {}", e),
            retryable: false,
        })?;

        let models = body.model_summaries.into_iter()
            .filter(|m| m.inference_types_supported.iter().any(|t| t == "ON_DEMAND"))
            .map(|m| ModelConfig {
                provider: ProviderKind::Bedrock,
                model_id: m.model_id.clone(),
                display_name: m.model_name.unwrap_or(m.model_id),
                max_tokens: 4096,
                temperature: 0.3,
                input_cost_per_m: 0.0, // Bedrock doesn't expose pricing in API; user configures
                output_cost_per_m: 0.0,
                cached_input_cost_per_m: 0.0,
                extra_params: None,
            })
            .collect();

        Ok(models)
    }
}
