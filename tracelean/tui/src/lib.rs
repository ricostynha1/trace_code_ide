//! TraceLean TUI library — exposed so headless smoke tests (ratatui
//! TestBackend) can drive the exact same app the binary runs (P8, D8.3).

pub mod app;
pub mod input;
pub mod ui;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use tracelean_core::EventSink;

/// Events pushed by core (tool chips, stats updates, file changes), drained
/// by the render loop each tick.
pub type EventQueue = Arc<Mutex<VecDeque<(String, String)>>>;

/// Queue-based EventSink for the TUI — the only outbound channel from core.
pub struct TuiEventSink {
    pub events: EventQueue,
}

impl EventSink for TuiEventSink {
    fn emit(&self, event: &str, payload: &str) {
        if let Ok(mut q) = self.events.lock() {
            q.push_back((event.to_string(), payload.to_string()));
            // Bound the queue so a burst can't grow unchecked.
            while q.len() > 512 {
                q.pop_front();
            }
        }
    }
}

pub fn new_event_queue() -> EventQueue {
    Arc::new(Mutex::new(VecDeque::new()))
}
