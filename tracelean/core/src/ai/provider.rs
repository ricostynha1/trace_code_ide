//! Core trait and types for AI providers.

use serde::{Deserialize, Serialize};

/// Which provider backend to use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenRouter,
    Bedrock,
    Mock,
}

/// Model configuration (selected by user in settings panel).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: ProviderKind,
    pub model_id: String,
    /// e.g. "claude-3.5-sonnet", "gpt-4o"
    pub display_name: String,
    pub max_tokens: u32,
    pub temperature: f32,
    /// Cost per 1M input tokens (USD)
    pub input_cost_per_m: f64,
    /// Cost per 1M output tokens (USD)
    pub output_cost_per_m: f64,
    /// Cost per 1M cached input tokens (0.0 = free)
    pub cached_input_cost_per_m: f64,
    /// Provider-specific extra params (JSON blob)
    pub extra_params: Option<serde_json::Value>,
}

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    /// For role=Tool: the tool_call_id this result corresponds to.
    /// For role=Assistant with tool calls: not used (tool_calls are in AiResponse).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    /// Tool result message (contains tool_call_id in content as prefix)
    Tool,
}

/// Tool schema in OpenAI-compatible function calling format.
/// This is what models were trained on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    /// "function" (only type supported)
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunction,
}

/// Function definition inside a ToolSchema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing parameters
    pub parameters: serde_json::Value,
}

/// A tool call returned by the model in its response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResponse {
    /// Unique ID for this tool call (assigned by model)
    pub id: String,
    /// "function" (only type supported)
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

/// Function call details in a tool call response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// JSON-encoded arguments string (as OpenAI returns it)
    pub arguments: String,
}

/// Request sent to an AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequest {
    pub model: ModelConfig,
    pub messages: Vec<ChatMessage>,
    /// Optional stop sequences
    pub stop: Option<Vec<String>>,
    /// Tools available to the model (OpenAI function calling format)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
}

/// Response from an AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiResponse {
    pub content: String,
    pub usage: super::tracking::TokenUsage,
    /// Raw response body for full transparency
    pub raw_response: Option<String>,
    /// Whether the response was truncated (hit max_tokens)
    pub truncated: bool,
    /// Tool calls requested by the model (empty if none)
    #[serde(default)]
    pub tool_calls: Vec<ToolCallResponse>,
}

/// Errors from provider calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    Network,
    RateLimit,
    Authentication,
    InvalidRequest,
    Timeout,
    ProviderError,
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

/// The core provider trait. Each backend implements this.
#[async_trait::async_trait]
pub trait AiProvider: Send + Sync {
    /// Send a request and get a response.
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError>;

    /// Human-readable provider name.
    fn name(&self) -> &str;

    /// Fetch available models from the provider (live query).
    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError>;
}
