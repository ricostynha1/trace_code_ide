//! AI Integration module — provider-agnostic AI client with transparency.
//!
//! Provides:
//! - `AiProvider` trait (OpenRouter, Bedrock, Mock)
//! - Token/cost tracking
//! - Prompt templates
//! - Context assembly
//! - Diff pipeline (shadow buffer → accept/reject hunks)
//! - Interaction log

pub mod provider;
pub mod openrouter;
pub mod bedrock;
pub mod tool_dialects;
pub mod mock;
pub mod tracking;
pub mod templates;
pub mod log;
pub mod context;
pub mod diff_pipeline;
pub mod tools;
pub mod tool_executor;
pub mod shell_sandbox;
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
pub mod trimmed_rules_table;
pub mod cost_trimmed_summary_model;
pub mod service;
pub mod ttl_tracking;

pub use provider::{AiProvider, AiRequest, AiResponse, ModelConfig, ProviderKind, ToolSchema, ToolCallResponse};
pub use tracking::{TokenUsage, CostEstimate};
pub use log::{InteractionEntry, InteractionLog};
pub use tools::{ToolDefinition, ToolCall, ToolResult};
pub use tool_executor::AgentPermissions;
pub use tool_selector::ToolIndex;
pub use streaming::StreamToken;
pub use embeddings::{EmbeddingsIndex, SharedIndex, new_shared_index, spawn_build, EmbedResult};
pub use tool_registry::{ToolRegistry, DynamicEntry, ToolJsonEntry};
pub use provider_cache::{ProviderCacheRegistry, ProviderCacheConfig, CacheMode};
pub use ttl_tracking::{TurnTimingTracker, CacheMarkerPlanner};
pub use retention::RetentionEngine;
pub use service::AiService;
pub use cost_trimmed_summary_model::{CostDecision, PruneContext, should_prune, should_summarize, batch_prune_decisions, summarization_decision};
pub use trimmed_rules_table::{classify_messages, ClassifiedMessages};

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
- For exact/regex/glob/filename search, use `run_shell` with `grep`/`find` — \
you already know these well. Use `find_semantic` only for meaning-based \
search (concepts, similar logic, or when you don't know the exact symbol).\
- Any tool's output larger than 500 lines or 30KB is automatically written to \
a file under `.tracelean/tool_logs/` and you get a preview plus that file's \
path — use `read_file` with an offset to page through the rest.\
- `find_semantic` drops matches below a relevance floor, so a short or empty \
result means no good match exists, not that something is broken. Each hit's \
snippet is a tight best-matching slice; use the larger file:line range shown \
alongside it (via read_file) for more context.\
- For repetitive multi-step shell or Python work (the same transformation \
across many files, a multi-stage build/check sequence, etc.), write a \
script to `.tracelean/tmp/` and run it with run_shell instead of issuing \
many small tool calls — that directory is sandboxed and safe to write \
and execute in freely, and it saves both round trips and tokens.";

#[cfg(test)]
mod system_prompt_tests {
    use super::SYSTEM_PROMPT;

    #[test]
    fn mentions_sandboxed_tmp_scripting() {
        // new_features_work_plan.md #4: point the agent at the tmp directory
        // that's already writable/executable in PROJECT_ALLOWLIST
        // (shell_sandbox.rs) instead of leaving scripting undiscoverable.
        assert!(SYSTEM_PROMPT.contains(".tracelean/tmp/"));
        assert!(SYSTEM_PROMPT.to_lowercase().contains("script"));
    }
}
