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

/// Text dialect the model uses for tool calls when tools are not passed
/// natively (or as a text-parsing fallback).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallFormat {
    /// MiniMax native XML: `<minimax:tool_call><invoke name=..><parameter ..>`
    MiniMaxXml,
    /// Hermes-style JSON inside `<tool_call>` tags.
    #[default]
    HermesJson,
    /// Mistral `[TOOL_CALLS] [...]` bracket format.
    MistralBrackets,
}

/// How tool definitions reach the model.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolPassing {
    /// Native `tools` request parameter (structured tool_calls in response).
    #[default]
    NativeParam,
    /// Tools rendered into the system prompt; calls parsed from response text.
    /// Workaround for providers that don't cache/return native tool calls
    /// (Bedrock Mantle + MiniMax).
    SystemPromptEmbed,
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
    /// Coding index score from llm-stats.com (TrueSkill μ−3σ). Higher = better.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_index: Option<f64>,
    /// Coding rank (1 = best)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_rank: Option<u32>,
    /// Whether the model supports prompt caching
    #[serde(default)]
    pub supports_caching: bool,
    /// Whether the model supports tool/function calling
    #[serde(default)]
    pub supports_tools: bool,
    /// Total context window in tokens (input + output).
    #[serde(default = "default_context_window")]
    pub context_window: u32,
    /// Whether context_window came from the catalog (false = fallback default,
    /// UI shows a `~` marker).
    #[serde(default)]
    pub context_window_known: bool,
    /// Text tool-call dialect for this model.
    #[serde(default)]
    pub tool_call_format: ToolCallFormat,
    /// How tools are passed to this model.
    #[serde(default)]
    pub tool_passing: ToolPassing,
}

fn default_context_window() -> u32 {
    128_000
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::Mock,
            model_id: String::new(),
            display_name: String::new(),
            max_tokens: 4096,
            temperature: 0.2,
            input_cost_per_m: 0.0,
            output_cost_per_m: 0.0,
            cached_input_cost_per_m: 0.0,
            extra_params: None,
            coding_index: None,
            coding_rank: None,
            supports_caching: false,
            supports_tools: false,
            context_window: default_context_window(),
            context_window_known: false,
            tool_call_format: ToolCallFormat::default(),
            tool_passing: ToolPassing::default(),
        }
    }
}

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    /// For role=Tool: the tool_call_id this result corresponds to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For role=Assistant: tool calls the model requested (needed by Bedrock Converse API).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallResponse>,
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
    /// Tools available to the model (OpenAI function calling format).
    /// This is the STATIC set — it must stay byte-stable across a session so
    /// the provider prompt cache prefix survives (bugs.md Feature 3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
    /// Dynamically discovered tools (bugs.md Feature 3). Providers inject these
    /// WITHOUT touching the cached prefix where possible: the embedded-text
    /// path appends them as a trailing user message; native paths merge them
    /// into the tools param (accepting that path's invalidation cost).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_tools: Option<Vec<ToolSchema>>,
    /// P9b: message indexes after which an explicit cache marker should be
    /// emitted (providers translate to their wire format; empty = none).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache_breakpoints: Vec<usize>,
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
    /// Reasoning/thinking text the model produced before the answer, when the
    /// provider exposes it (bugs.md Bug 1.8: shown in the chat UI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// Errors from provider calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
