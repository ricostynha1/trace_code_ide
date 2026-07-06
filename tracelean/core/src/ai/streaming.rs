//! Streaming response support (4.18) — token-by-token display in chat panel.
//! Uses Tauri events to push partial tokens to the frontend as they arrive.
//!
//! Providers that support streaming (OpenRouter SSE) emit tokens via this interface.
//! The frontend listens to "ai-stream-token" events and appends to the chat display.

use serde::{Deserialize, Serialize};

/// A streaming token event emitted to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamToken {
    /// Unique stream session ID (correlates tokens to a single response).
    pub stream_id: String,
    /// The token text (partial content).
    pub token: String,
    /// Whether this is the final token (stream complete).
    pub done: bool,
    /// Cumulative content so far (for idempotent display).
    pub accumulated: String,
}

/// Stream state tracker — accumulates tokens for a single response.
#[derive(Debug, Clone)]
pub struct StreamSession {
    pub id: String,
    pub accumulated: String,
    pub done: bool,
}

impl StreamSession {
    pub fn new(id: String) -> Self {
        Self {
            id,
            accumulated: String::new(),
            done: false,
        }
    }

    /// Append a token and return the StreamToken event to emit.
    pub fn push_token(&mut self, token: &str) -> StreamToken {
        self.accumulated.push_str(token);
        StreamToken {
            stream_id: self.id.clone(),
            token: token.to_string(),
            done: false,
            accumulated: self.accumulated.clone(),
        }
    }

    /// Mark stream as complete.
    pub fn finish(&mut self) -> StreamToken {
        self.done = true;
        StreamToken {
            stream_id: self.id.clone(),
            token: String::new(),
            done: true,
            accumulated: self.accumulated.clone(),
        }
    }
}

/// Parse an SSE (Server-Sent Events) line into content delta.
/// OpenRouter streams use `data: {...}` format.
pub fn parse_sse_delta(line: &str) -> Option<String> {
    let data = line.strip_prefix("data: ")?;
    if data == "[DONE]" {
        return None;
    }
    let json: serde_json::Value = serde_json::from_str(data).ok()?;
    json.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("delta"))
        .and_then(|delta| delta.get("content"))
        .and_then(|content| content.as_str())
        .map(|s| s.to_string())
}
