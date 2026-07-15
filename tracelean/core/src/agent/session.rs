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
}

impl ChatSession {
    pub fn new(id: String) -> Self {
        Self {
            id,
            user_view: Vec::new(),
            model_view: Vec::new(),
            turn: 0,
            compacted: false,
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
    }
}
