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
You are TraceLean, an AI coding assistant embedded in an IDE.\n\
You help users understand, modify, and verify code using the provided tools.\n\
\n\
IMPORTANT — YOU MUST CALL MULTIPLE TOOLS IN PARALLEL:\n\
You can and SHOULD return multiple tool_calls in a single response.\n\
Do NOT call one tool then wait. Call all independent tools at once.\n\
\n\
Examples of parallel batching:\n\
- Need to read 3 files? → Return 3 read_file calls in one response.\n\
- Need to list dir + read instructions? → Return list_directory + read_file together.\n\
- Need to edit then test? → edit_file first (depends on read), then run_shell after.\n\
\n\
Only sequence tool calls when one depends on the result of another.\n\
If you return 1 tool call when 2+ could run in parallel, you are wasting resources.\n\
\n\
Other rules:\n\
- Tool calls: arguments only, no extra text unless explicitly asked.\n\
- Do not repeat file contents already visible in context. Reference by path and line range.\n\
- When a tool returns has_more=true, decide whether more data is needed before calling again.\n\
- Minimize redundant reads: if a file region is already in context and unmodified, do not re-read it.\n\
- Prefer `find` over `run_shell` for file search tasks.";
