//! ChatSession — persistent dual-view conversation state.
//!
//! user_view:  append-only, all messages as-is. What the user sees in the chat panel.
//! model_view: derived from user_view via compaction/summarization. What the LLM actually receives.
//!
//! The model_view evolves across calls — compaction is NOT recomputed from scratch each time.
//! This preserves prompt caching (stable prefix) and avoids wasted summarization work.

use serde::{Deserialize, Serialize};

use crate::ai::provider::ChatMessage;

/// A persistent chat session with dual-view architecture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    /// Append-only: every message exchanged (user, assistant, tool calls, tool results).
    /// This is what the user sees in the UI — never mutated after append.
    pub user_view: Vec<ChatMessage>,
    /// Derived: the compacted/summarized version sent to the LLM.
    /// Evolves across calls — compaction results persist.
    pub model_view: Vec<ChatMessage>,
    /// Turn counter for retention engine tracking.
    pub turn: usize,
    /// Whether model_view has diverged from user_view (i.e., compaction happened).
    pub compacted: bool,
    /// Prompt tokens of the LAST request in the last turn (P7: real context size).
    #[serde(default)]
    pub last_prompt_tokens: u32,
    /// Completion tokens of the last response (P7).
    #[serde(default)]
    pub last_completion_tokens: u32,
    /// How many times context was compacted in this session (P7).
    #[serde(default)]
    pub compaction_count: u32,
    /// Cumulative cost of this session (P11 — shown in the switcher).
    #[serde(default)]
    pub total_cost_usd: f64,
    /// RFC3339 timestamp of the last completed turn (P11).
    #[serde(default)]
    pub updated_at: String,
    /// Names of dynamically discovered tools loaded in this session
    /// (bugs.md Feature 3) — restored into the ToolRegistry each turn.
    #[serde(default)]
    pub dynamic_tools: Vec<String>,
}

/// Lightweight session listing entry for the switcher (P11).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionSummary {
    pub id: String,
    /// First user message (≤50 chars) or "Empty chat".
    pub label: String,
    pub message_count: usize,
    pub total_cost_usd: f64,
    pub updated_at: String,
}

/// Context-utilization snapshot for the UI (P7, D7.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionInfo {
    /// Estimated next-turn context = last prompt + last completion tokens
    /// (exact for the prefix; new user input adds on top).
    pub estimated_context_tokens: u32,
    pub context_window: u32,
    /// False when the window is the fallback default (UI shows `~`).
    pub context_window_known: bool,
    pub turns: usize,
    pub compaction_count: u32,
    pub model_view_len: usize,
}

impl ChatSession {
    pub fn new(id: String) -> Self {
        Self {
            id,
            user_view: Vec::new(),
            model_view: Vec::new(),
            turn: 0,
            compacted: false,
            last_prompt_tokens: 0,
            last_completion_tokens: 0,
            compaction_count: 0,
            total_cost_usd: 0.0,
            updated_at: String::new(),
            dynamic_tools: Vec::new(),
        }
    }

    /// Switcher label: first user message, truncated (P11).
    pub fn label(&self) -> String {
        self.user_view
            .iter()
            .find(|m| m.role == crate::ai::provider::MessageRole::User)
            .map(|m| {
                let text: String = m.content.chars().take(50).collect();
                if m.content.chars().count() > 50 { format!("{}…", text) } else { text }
            })
            .unwrap_or_else(|| "Empty chat".to_string())
    }

    pub fn summary(&self) -> ChatSessionSummary {
        ChatSessionSummary {
            id: self.id.clone(),
            label: self.label(),
            message_count: self.user_view.len(),
            total_cost_usd: self.total_cost_usd,
            updated_at: self.updated_at.clone(),
        }
    }

    /// Build the context-utilization snapshot against a model's window (P7).
    pub fn info(&self, context_window: u32, context_window_known: bool) -> ChatSessionInfo {
        ChatSessionInfo {
            estimated_context_tokens: self.last_prompt_tokens + self.last_completion_tokens,
            context_window,
            context_window_known,
            turns: self.turn,
            compaction_count: self.compaction_count,
            model_view_len: self.model_view.len(),
        }
    }

    /// Append a message to BOTH views (used for new user messages and assistant responses).
    /// Before compaction triggers, both views are identical.
    pub fn append(&mut self, msg: ChatMessage) {
        self.user_view.push(msg.clone());
        self.model_view.push(msg);
    }

    /// Append a message to user_view only (used when model_view already has it via tool loop).
    pub fn append_user_view_only(&mut self, msg: ChatMessage) {
        self.user_view.push(msg);
    }

    /// Append a message to model_view only (e.g., a summary insertion).
    pub fn append_model_view_only(&mut self, msg: ChatMessage) {
        self.model_view.push(msg);
        self.compacted = true;
    }

    /// Replace model_view entirely (after compaction produces a new derived view).
    pub fn set_model_view(&mut self, view: Vec<ChatMessage>) {
        self.model_view = view;
        self.compacted = true;
    }

    /// Get user_view for display (frontend).
    pub fn user_messages(&self) -> &[ChatMessage] {
        &self.user_view
    }

    /// Get model_view for LLM request.
    pub fn model_messages(&self) -> &[ChatMessage] {
        &self.model_view
    }

    /// Advance turn counter.
    pub fn advance_turn(&mut self) {
        self.turn += 1;
    }

    /// Reset session (new chat).
    pub fn reset(&mut self) {
        self.user_view.clear();
        self.model_view.clear();
        self.turn = 0;
        self.compacted = false;
        self.last_prompt_tokens = 0;
        self.last_completion_tokens = 0;
        self.compaction_count = 0;
        self.dynamic_tools.clear();
    }

    /// Reset the model context only (P7, D7.4): clears model_view so the next
    /// request starts from ~system+tools size, but keeps user_view so the UI
    /// transcript survives (a divider is rendered frontend-side).
    pub fn reset_context(&mut self) {
        self.model_view.clear();
        self.compacted = false;
        self.last_prompt_tokens = 0;
        self.last_completion_tokens = 0;
    }
}
