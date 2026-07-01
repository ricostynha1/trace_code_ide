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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// Request sent to an AI provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRequest {
    pub model: ModelConfig,
    pub messages: Vec<ChatMessage>,
    /// Optional stop sequences
    pub stop: Option<Vec<String>>,
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
