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
pub mod embeddings;
pub mod benchmark;
pub mod model_catalog;
pub mod tool_registry;
pub mod tool_errors;
pub mod provider_cache;
pub mod retention;
pub mod cost_model;
pub mod ttl_tracking;

pub use provider::{AiProvider, AiRequest, AiResponse, ModelConfig, ProviderKind, ToolSchema, ToolCallResponse};
pub use tracking::{TokenUsage, CostEstimate};
pub use log::{InteractionEntry, InteractionLog};
pub use tools::{ToolDefinition, ToolCall, ToolResult};
pub use tool_executor::AgentPermissions;
pub use tool_selector::ToolIndex;
pub use streaming::StreamToken;
pub use embeddings::{EmbeddingsIndex, SharedIndex, new_shared_index, EmbedResult};
pub use tool_registry::{ToolRegistry, DynamicEntry, ToolJsonEntry};
pub use provider_cache::{ProviderCacheRegistry, ProviderCacheConfig, CacheMode};
pub use ttl_tracking::{TurnTimingTracker, CacheMarkerPlanner};
pub use retention::RetentionEngine;
pub use cost_model::{CostDecision, PruneContext, should_prune, should_summarize, batch_prune_decisions, summarization_decision};

/// Canonical system prompt for all TraceLean agent paths (GUI chat, ACP, headless).
/// Single source of truth — do NOT duplicate elsewhere.
pub const SYSTEM_PROMPT: &str = "\
You are TraceLean, an AI coding assistant in an IDE.\
You help users understand, modify, and verify code using the provided tools.\
\
TOOL CALLING RULES:\
- Return multiple tool calls in parallel when independent.\
- Arguments only, no extra text unless explicitly asked.\
- If you need a tool you don't have, call tool discover_tools.\
- Prefer `find` over `run_shell` for file search.";
