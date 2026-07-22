//! AI interaction log — stores all prompts/responses for browsing and debugging.

use super::provider::{AiRequest, AiResponse, AiError, ToolSchema};
use super::tracking::{CostEstimate, TokenUsage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single logged AI interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionEntry {
    pub id: String,
    pub timestamp: String,
    pub agent: String, // "elicitation", "formalisation", "implementation", "repair", "chat"
    pub model_id: String,
    pub model_display_name: String,
    pub request_messages: Vec<super::provider::ChatMessage>,
    pub response_content: Option<String>,
    /// Reasoning/thinking text from the model, when the provider exposes it
    /// (bugs.md Bug 1.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_thinking: Option<String>,
    /// Tool calls returned by the model (if any).
    #[serde(default)]
    pub response_tool_calls: Vec<super::provider::ToolCallResponse>,
    pub error: Option<String>,
    pub usage: TokenUsage,
    pub cost: CostEstimate,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Was the response truncated?
    pub truncated: bool,
    /// Was context compacted/pruned before this request?
    #[serde(default)]
    pub was_compacted: bool,
    /// Number of tools provided to the model in this request.
    #[serde(default)]
    pub tools_provided: u32,
    /// Names of tools provided to the model.
    #[serde(default)]
    pub tool_names: Vec<String>,
    /// Full tool schemas sent to the model (for log inspection).
    #[serde(default)]
    pub tool_schemas: Vec<ToolSchema>,
    /// How tools were passed for this request (D9a.3: diagnosable from the Log tab).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_passing: Option<super::provider::ToolPassing>,
    /// What compaction ran before this request (bugs.md: log icons + detail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<CompactionInfo>,
    /// bugs.md Bug 3: the exact wire request body, for a "copy raw request"
    /// button — rules out the prompt itself changing between calls when
    /// cache hits look inconsistent. `None` when the provider doesn't
    /// capture it (e.g. the call failed before the body was serialized).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_raw: Option<String>,
    /// bugs.md Bug 3: cached tokens the cost model predicted for this turn,
    /// alongside `usage.cached_tokens` (the actual value the provider
    /// reported) — the ratio is the "cache health" the log shows. `None`
    /// when no prediction was made this turn (e.g. no explicit cache
    /// markers were planned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub predicted_cached_tokens: Option<usize>,
    /// FEATURE 2: the model's context window at the time of this request, so
    /// the Log tab can show "% context used" per entry (usage.input_tokens /
    /// context_window) without re-deriving it from model catalogs later.
    #[serde(default)]
    pub context_window: u32,
    /// False when `context_window` is the generic fallback, not a value the
    /// model catalog actually knows (mirrors `ChatSessionInfo::context_window_known`).
    #[serde(default)]
    pub context_window_known: bool,
}

/// Compaction that ran before a request: summarization or trimming (prune).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactionInfo {
    /// "summarized" | "trimmed" | "candidates" (nothing removed this turn,
    /// but some entries were eligible and kept — see `details`).
    pub kind: String,
    pub messages_removed: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
    /// Per-message record of what the compactor decided (bugs.md Feature 1.1:
    /// the Log tab shows candidates vs actually removed, color-coded).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<CompactionMessageDetail>,
}

/// One message the compactor looked at, and what happened to it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactionMessageDetail {
    /// "user" | "assistant" | "tool" | "system"
    pub role: String,
    /// First ~160 chars of the message content.
    pub preview: String,
    pub tokens: usize,
    /// "trimmed" — removed from context;
    /// "trim_candidate" — eligible but kept (cost math said keep);
    /// "summarized" — folded into a [Context Summary];
    /// "kept_user" — inside the trim window but preserved (user question).
    pub action: String,
}

/// Static + dynamic tools of a request, flattened for logging. Dynamic tool
/// names get a `+` prefix so the Log tab shows how each was passed.
fn tools_of(request: &AiRequest) -> (u32, Vec<String>, Vec<ToolSchema>) {
    let mut names: Vec<String> = request.tools.as_ref()
        .map(|t| t.iter().map(|s| s.function.name.clone()).collect())
        .unwrap_or_default();
    let mut schemas: Vec<ToolSchema> = request.tools.clone().unwrap_or_default();
    if let Some(dyn_tools) = &request.dynamic_tools {
        names.extend(dyn_tools.iter().map(|s| format!("+{}", s.function.name)));
        schemas.extend(dyn_tools.iter().cloned());
    }
    (schemas.len() as u32, names, schemas)
}

/// Persistent interaction log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionLog {
    entries: Vec<InteractionEntry>,
    /// Max entries to keep in memory (older ones only on disk)
    max_in_memory: usize,
}

impl InteractionLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_in_memory: 500,
        }
    }

    /// Record a successful interaction.
    pub fn record_success(
        &mut self,
        agent: &str,
        request: &AiRequest,
        response: &AiResponse,
        duration_ms: u64,
    ) {
        let cost = response.usage.estimate_cost(
            request.model.input_cost_per_m,
            request.model.output_cost_per_m,
            request.model.cached_input_cost_per_m,
        );

        let (tools_provided, tool_names, tool_schemas) = tools_of(request);

        // If model returned tool_calls with no text content, store None (not empty string)
        let response_content = if response.content.is_empty() && !response.tool_calls.is_empty() {
            None
        } else {
            Some(response.content.clone())
        };

        let entry = InteractionEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            agent: agent.to_string(),
            model_id: request.model.model_id.clone(),
            model_display_name: request.model.display_name.clone(),
            request_messages: request.messages.clone(),
            response_content,
            response_thinking: response.thinking.clone(),
            response_tool_calls: response.tool_calls.clone(),
            error: None,
            usage: response.usage.clone(),
            cost,
            duration_ms,
            truncated: response.truncated,
            was_compacted: false,
            tools_provided,
            tool_names,
            tool_schemas,
            tool_passing: Some(request.model.tool_passing),
            compaction: None,
            request_raw: response.raw_request.clone(),
            predicted_cached_tokens: None,
            context_window: request.model.context_window,
            context_window_known: request.model.context_window_known,
        };

        self.entries.push(entry);
        self.trim();
    }

    /// Record a failed interaction.
    pub fn record_failure(
        &mut self,
        agent: &str,
        request: &AiRequest,
        error: &AiError,
        duration_ms: u64,
    ) {
        let (tools_provided, tool_names, tool_schemas) = tools_of(request);

        let entry = InteractionEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            agent: agent.to_string(),
            model_id: request.model.model_id.clone(),
            model_display_name: request.model.display_name.clone(),
            request_messages: request.messages.clone(),
            response_content: None,
            response_thinking: None,
            response_tool_calls: Vec::new(),
            error: Some(error.message.clone()),
            usage: TokenUsage::default(),
            cost: CostEstimate {
                total_usd: 0.0,
                input_cost: 0.0,
                output_cost: 0.0,
                cached_savings: 0.0,
            },
            duration_ms,
            truncated: false,
            was_compacted: false,
            tools_provided,
            tool_names,
            tool_schemas,
            tool_passing: Some(request.model.tool_passing),
            compaction: None,
            request_raw: None,
            predicted_cached_tokens: None,
            context_window: request.model.context_window,
            context_window_known: request.model.context_window_known,
        };

        self.entries.push(entry);
        self.trim();
    }

    /// Attach compaction metadata to the most recent entry (set right after
    /// record_success for the request that followed the compaction).
    pub fn attach_compaction_to_last(&mut self, info: CompactionInfo) {
        if let Some(last) = self.entries.last_mut() {
            last.compaction = Some(info);
        }
    }

    /// bugs.md Bug 3: attach the cache-tracker's prediction for this turn to
    /// the entry it corresponds to, so the log can show predicted vs. actual
    /// cache hit rate ("cache health").
    pub fn set_last_entry_predicted_cache(&mut self, predicted_cached_tokens: usize) {
        if let Some(last) = self.entries.last_mut() {
            last.predicted_cached_tokens = Some(predicted_cached_tokens);
        }
    }

    /// Mark the last entry as having used compacted context.
    pub fn mark_last_entry_compacted(&mut self) {
        if let Some(entry) = self.entries.last_mut() {
            entry.was_compacted = true;
        }
    }

    /// Get recent entries (most recent first).
    pub fn recent(&self, limit: usize) -> Vec<&InteractionEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

    /// Get all entries.
    pub fn all(&self) -> &[InteractionEntry] {
        &self.entries
    }

    /// Get entry by ID.
    pub fn get(&self, id: &str) -> Option<&InteractionEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn trim(&mut self) {
        if self.entries.len() > self.max_in_memory {
            let excess = self.entries.len() - self.max_in_memory;
            self.entries.drain(0..excess);
        }
    }

    /// Persist log to disk (AI log dir).
    pub fn save_to_disk(&self, project_root: &PathBuf) -> Result<(), String> {
        let dir = project_root.join(".tracelean").join("ai_log");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("interactions.json");
        let json = serde_json::to_string_pretty(&self.entries).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Persist log to .tracelean/commands/command_log.json (structured storage).
    pub fn save_to_command_log(&self, project_root: &PathBuf) -> Result<(), String> {
        let dir = project_root.join(".tracelean").join("commands");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("command_log.json");
        let json = serde_json::to_string_pretty(&self.entries).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Save to both log locations. Call after each record_success/record_failure.
    pub fn persist(&self, project_root: &PathBuf) {
        let _ = self.save_to_disk(project_root);
        let _ = self.save_to_command_log(project_root);
    }

    /// Load log from disk.
    pub fn load_from_disk(project_root: &PathBuf) -> Self {
        let path = project_root.join(".tracelean").join("ai_log").join("interactions.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(entries) = serde_json::from_str::<Vec<InteractionEntry>>(&data) {
                return Self { entries, max_in_memory: 500 };
            }
        }
        Self::new()
    }
}
