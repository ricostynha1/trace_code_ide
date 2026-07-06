//! Mock AI provider — shows exact prompts, lets user respond manually.
//! Used for debugging prompt engineering without spending tokens.

use super::provider::*;
use super::tracking::TokenUsage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

/// Pending mock request waiting for user response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockPendingRequest {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    pub model: ModelConfig,
    pub timestamp: String,
}

/// The mock provider queues requests and waits for manual responses.
pub struct MockProvider {
    /// Channel to send pending requests to the UI
    pending_tx: mpsc::UnboundedSender<(MockPendingRequest, oneshot::Sender<String>)>,
    /// Shared list of pending requests (for UI polling)
    pub pending: Arc<Mutex<Vec<MockPendingRequest>>>,
    /// Response channels keyed by request ID
    response_channels: Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<String>>>>,
}

impl MockProvider {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<(MockPendingRequest, oneshot::Sender<String>)>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let provider = Self {
            pending_tx: tx,
            pending: Arc::new(Mutex::new(Vec::new())),
            response_channels: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };
        (provider, rx)
    }

    /// Submit a user response to a pending mock request.
    pub async fn submit_response(&self, request_id: &str, response: String) -> Result<(), String> {
        let mut channels = self.response_channels.lock().await;
        if let Some(tx) = channels.remove(request_id) {
            tx.send(response).map_err(|_| "Response channel closed".to_string())?;
            // Remove from pending list
            let mut pending = self.pending.lock().await;
            pending.retain(|p| p.id != request_id);
            Ok(())
        } else {
            Err(format!("No pending request with id: {}", request_id))
        }
    }

    /// Get all pending requests (for UI display).
    pub async fn get_pending(&self) -> Vec<MockPendingRequest> {
        self.pending.lock().await.clone()
    }
}

#[async_trait::async_trait]
impl AiProvider for MockProvider {
    async fn complete(&self, request: &AiRequest) -> Result<AiResponse, AiError> {
        let id = uuid::Uuid::new_v4().to_string();
        let pending_req = MockPendingRequest {
            id: id.clone(),
            messages: request.messages.clone(),
            model: request.model.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        let (response_tx, response_rx) = oneshot::channel();

        // Store in pending list
        {
            let mut pending = self.pending.lock().await;
            pending.push(pending_req.clone());
        }
        {
            let mut channels = self.response_channels.lock().await;
            channels.insert(id.clone(), response_tx);
        }

        // Notify UI via channel (dummy sender, real response comes via submit_response)
        let _ = self.pending_tx.send((pending_req, {
            let (tx, _rx) = oneshot::channel();
            tx
        }));

        // Wait for user response (or timeout)
        let content = tokio::time::timeout(
            std::time::Duration::from_secs(600), // 10 min timeout
            response_rx,
        )
        .await
        .map_err(|_| AiError {
            kind: AiErrorKind::Timeout,
            message: "Mock response timed out (10 min)".into(),
            retryable: false,
        })?
        .map_err(|_| AiError {
            kind: AiErrorKind::ProviderError,
            message: "Response channel dropped".into(),
            retryable: false,
        })?;

        // Estimate tokens (rough: 4 chars per token)
        let input_tokens: u32 = request.messages.iter()
            .map(|m| m.content.len() as u32 / 4)
            .sum();
        let output_tokens = content.len() as u32 / 4;

        Ok(AiResponse {
            content,
            usage: TokenUsage {
                input_tokens,
                output_tokens,
                thinking_tokens: 0,
                cached_tokens: 0,
            },
            raw_response: Some("[mock response — user-provided]".into()),
            truncated: false,
        })
    }

    fn name(&self) -> &str {
        "Mock (Debug)"
    }

    async fn list_models(&self) -> Result<Vec<ModelConfig>, AiError> {
        Ok(vec![ModelConfig {
            provider: ProviderKind::Mock,
            model_id: "mock-debug".into(),
            display_name: "Mock Agent (Debug)".into(),
            max_tokens: 99999,
            temperature: 0.0,
            input_cost_per_m: 0.0,
            output_cost_per_m: 0.0,
            cached_input_cost_per_m: 0.0,
            extra_params: None,
        }])
    }
}
