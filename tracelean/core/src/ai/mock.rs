//! Mock AI provider — shows the exact JSON that would be sent to an LLM API.
//! User inspects the full request (messages + tools) and pastes back the full JSON response.
//! Used for debugging: copy request to web chat, get response, paste it back.

use super::provider::*;
use super::tracking::TokenUsage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Pending mock request waiting for user response.
/// Contains the full raw JSON that would be sent to the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockPendingRequest {
    pub id: String,
    /// The full OpenAI-compatible request JSON (messages + tools + model etc.)
    pub raw_request_json: String,
    pub model: ModelConfig,
    pub timestamp: String,
}

/// The expected response format (OpenAI-compatible).
/// User pastes back a JSON response matching this structure.
#[derive(Debug, Clone, Deserialize)]
struct MockResponseJson {
    choices: Option<Vec<MockChoice>>,
    /// Alternative: just content string for simple responses
    content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct MockChoice {
    message: Option<MockMessage>,
    finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MockMessage {
    content: Option<String>,
    tool_calls: Option<Vec<MockToolCall>>,
}

#[derive(Debug, Clone, Deserialize)]
struct MockToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<MockToolCallFunction>,
}

#[derive(Debug, Clone, Deserialize)]
struct MockToolCallFunction {
    name: Option<String>,
    arguments: Option<String>,
}

/// The mock provider queues requests and waits for manual responses.
pub struct MockProvider {
    /// Channel to send pending requests to the UI
    pending_tx: mpsc::UnboundedSender<(MockPendingRequest, oneshot::Sender<String>)>,
    /// Shared list of pending requests (for UI polling)
    pub pending: Arc<Mutex<Vec<MockPendingRequest>>>,
    /// Response channels keyed by request ID
    response_channels: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<String>>>>,
}

impl MockProvider {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<(MockPendingRequest, oneshot::Sender<String>)>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let provider = Self {
            pending_tx: tx,
            pending: Arc::new(Mutex::new(Vec::new())),
            response_channels: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        (provider, rx)
    }

    /// Submit a user response to a pending mock request.
    /// Accepts either:
    /// 1. Full OpenAI-compatible JSON response
    /// 2. Plain text (treated as simple content response)
    pub async fn submit_response(&self, request_id: &str, response: String) -> Result<(), String> {
        let mut channels = self.response_channels.lock().await;
        if let Some(tx) = channels.remove(request_id) {
            tx.send(response).map_err(|_| "Response channel closed".to_string())?;
            // Remove from pending list
            let mut pending = self.pending.lock().await;
            pending.retain(|p| p.id != request_id);
            Ok(())
        } else {
            Err(format!("No pending request with id: {}", request_id))
        }
    }

    /// Get all pending requests (for UI display).
    pub async fn get_pending(&self) -> Vec<MockPendingRequest> {
        self.pending.lock().await.clone()
    }
}

/// Build the OpenAI-compatible request JSON that would be sent to the API.
fn build_raw_request_json(request: &AiRequest) -> String {
    #[derive(Serialize)]
    struct RawRequest<'a> {
        model: &'a str,
        messages: Vec<RawMessage<'a>>,
        max_tokens: u32,
        temperature: f32,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop: &'a Option<Vec<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tools: &'a Option<Vec<ToolSchema>>,
    }

    #[derive(Serialize)]
    struct RawMessage<'a> {
        role: &'a str,
        content: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<&'a str>,
    }

    let messages: Vec<RawMessage> = request.messages.iter().map(|m| RawMessage {
        role: match m.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        },
        content: &m.content,
        tool_call_id: m.tool_call_id.as_deref(),
    }).collect();

    let raw = RawRequest {
        model: &request.model.model_id,
        messages,
        max_tokens: request.model.max_tokens,
        temperature: request.model.temperature,
        stop: &request.stop,
        tools: &request.tools,
    };

    serde_json::to_string_pretty(&raw).unwrap_or_else(|_| "{}".into())
}

/// Parse the user's response. Accepts full OpenAI JSON or plain text.
fn parse_mock_response(raw: &str) -> (String, Vec<ToolCallResponse>) {
    // Try parsing as full OpenAI response JSON
    if let Ok(resp) = serde_json::from_str::<MockResponseJson>(raw) {
        // Full response with choices
        if let Some(choices) = resp.choices {
            if let Some(choice) = choices.first() {
                if let Some(msg) = &choice.message {
                    let content = msg.content.clone().unwrap_or_default();
                    let tool_calls: Vec<ToolCallResponse> = msg.tool_calls.as_ref()
                        .map(|tcs| tcs.iter().enumerate().filter_map(|(i, tc)| {
                            let func = tc.function.as_ref()?;
                            Some(ToolCallResponse {
                                id: tc.id.clone().unwrap_or_else(|| format!("call_{}", i)),
                                call_type: tc.call_type.clone().unwrap_or_else(|| "function".into()),
                                function: ToolCallFunction {
                                    name: func.name.clone().unwrap_or_default(),
                                    arguments: func.arguments.clone().unwrap_or_else(|| "{}".into()),
                                },
                            })
                        }).collect())
                        .unwrap_or_default();
                    return (content, tool_calls);
                }
            }
        }
        // Simple content field
        if let Some(content) = resp.content {
            return (content, Vec::new());
        }
    }

    // Fallback: treat as plain text response
    (raw.to_string(), Vec::new())
}

#[async_trait::async_trait]
impl AiProvider for MockProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        // Headless auto-response (P8, D8.3): CI/smoke tests set
        // TRACELEAN_MOCK_AUTO to bypass the interactive queue entirely.
        if let Ok(auto) = std::env::var("TRACELEAN_MOCK_AUTO") {
            return Ok(AiResponse {
                content: auto,
                usage: TokenUsage::default(),
                raw_response: None,
                raw_request: Some(build_raw_request_json(request)),
                truncated: false,
                tool_calls: Vec::new(),
                thinking: None,
            });
        }

        let id = uuid::Uuid::new_v4().to_string();
        let raw_request_json = build_raw_request_json(request);

        let pending_req = MockPendingRequest {
            id: id.clone(),
            raw_request_json: raw_request_json.clone(),
            model: request.model.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        let (response_tx, response_rx) = oneshot::channel();

        // Store in pending list
        {
            let mut pending = self.pending.lock().await;
            pending.push(pending_req.clone());
        }
        {
            let mut channels = self.response_channels.lock().await;
            channels.insert(id.clone(), response_tx);
        }

        // Notify UI via channel
        let _ = self.pending_tx.send((pending_req, {
            let (tx, _rx) = oneshot::channel();
            tx
        }));

        // Wait for user response (or timeout)
        let raw_response = tokio::time::timeout(
            std::time::Duration::from_secs(600), // 10 min timeout
            response_rx,
        )
        .await
        .map_err(|_| AiError {
            kind: AiErrorKind::Timeout,
            message: "Mock response timed out (10 min)".into(),
            retryable: false,
        })?
        .map_err(|_| AiError {
            kind: AiErrorKind::ProviderError,
            message: "Response channel dropped".into(),
            retryable: false,
        })?;

        // Parse the response (full JSON or plain text)
        let (content, tool_calls) = parse_mock_response(&raw_response);

        // Estimate tokens (rough: 4 chars per token)
        let input_tokens: u32 = request.messages.iter()
            .map(|m| m.content.len() as u32 / 4)
            .sum();
        let output_tokens = content.len() as u32 / 4;

        Ok(AiResponse {
            thinking: None,
            content,
            usage: TokenUsage {
                input_tokens,
                output_tokens,
                thinking_tokens: 0,
                cached_tokens: 0,
            },
            raw_response: Some(raw_response),
            raw_request: Some(raw_request_json),
            truncated: false,
            tool_calls,
        })
    }

    fn name(&self) -> &str {
        "Mock (Debug)"
    }

    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError> {
        Ok(vec![model_for_id("mock-debug")])
    }
}

/// The single static mock model config — no network, no id lookup needed
/// (there's only ever one). Used both by `list_models` and to re-derive a
/// persisted settings selection on load.
pub fn model_for_id(_model_id: &str) -> ModelConfig {
    ModelConfig {
        provider: ProviderKind::Mock,
        model_id: "mock-debug".into(),
        display_name: "Mock Agent (Debug)".into(),
        max_tokens: 99999,
        temperature: 0.0,
        input_cost_per_m: 15.0,
        output_cost_per_m: 75.0,
        cached_input_cost_per_m: 1.875,
        extra_params: None,
        coding_index: None,
        coding_rank: None,
        supports_caching: false,
        supports_tools: false,
        ..Default::default()
    }
}
