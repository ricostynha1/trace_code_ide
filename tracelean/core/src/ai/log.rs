//! AI interaction log — stores all prompts/responses for browsing and debugging.

use super::provider::{AiRequest, AiResponse, AiError};
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
    pub error: Option<String>,
    pub usage: TokenUsage,
    pub cost: CostEstimate,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Was the response truncated?
    pub truncated: bool,
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

        let entry = InteractionEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            agent: agent.to_string(),
            model_id: request.model.model_id.clone(),
            model_display_name: request.model.display_name.clone(),
            request_messages: request.messages.clone(),
            response_content: Some(response.content.clone()),
            error: None,
            usage: response.usage.clone(),
            cost,
            duration_ms,
            truncated: response.truncated,
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
        let entry = InteractionEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            agent: agent.to_string(),
            model_id: request.model.model_id.clone(),
            model_display_name: request.model.display_name.clone(),
            request_messages: request.messages.clone(),
            response_content: None,
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
        };

        self.entries.push(entry);
        self.trim();
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

    /// Persist log to disk.
    pub fn save_to_disk(&self, project_root: &PathBuf) -> Result<(), String> {
        let dir = project_root.join(".tracelean").join("ai_log");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join("interactions.json");
        let json = serde_json::to_string_pretty(&self.entries).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())?;
        Ok(())
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
