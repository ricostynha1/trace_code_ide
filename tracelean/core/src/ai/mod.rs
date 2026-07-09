//! AI Integration module — provider-agnostic AI client with transparency.
//!
//! Provides:
//! - `AiProvider` trait (OpenRouter, Bedrock, Mock)
//! - Token/cost tracking
//! - Prompt templates
//! - Context assembly
//! - Response parsing
//! - Diff pipeline (shadow buffer → accept/reject hunks)
//! - Agent implementations (elicitation, formalisation, implementation, repair)
//! - Interaction log

pub mod provider;
pub mod openrouter;
pub mod bedrock;
pub mod bedrock_pricing;
pub mod mock;
pub mod tracking;
pub mod templates;
pub mod log;
pub mod context;
pub mod response_parser;
pub mod diff_pipeline;
pub mod agents;
pub mod tools;
pub mod tool_executor;
pub mod tool_selector;
pub mod mcp_host;
pub mod mcp_client;
pub mod streaming;

pub use provider::{AiProvider, AiRequest, AiResponse, ModelConfig, ProviderKind, ToolSchema, ToolCallResponse};
pub use tracking::{TokenUsage, CostEstimate};
pub use log::{InteractionEntry, InteractionLog};
pub use tools::{ToolDefinition, ToolCall, ToolResult};
pub use tool_executor::AgentPermissions;
pub use tool_selector::ToolIndex;
pub use streaming::StreamToken;
