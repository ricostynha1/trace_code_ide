//! TraceLean TUI — terminal frontend.
//! Shares the same core engine as the GUI.
//!
//! Usage:
//!   tracelean-tui [project-path]

mod app;
mod input;
mod ui;

use app::App;
use std::sync::Arc;
use tracelean_core::{SharedApp, EventSink};

/// Channel-based EventSink for TUI — pushes events to render loop.
pub struct TuiEventSink {
    tx: tokio::sync::broadcast::Sender<String>,
}

impl TuiEventSink {
    pub fn new() -> (Self, tokio::sync::broadcast::Receiver<String>) {
        let (tx, rx) = tokio::sync::broadcast::channel(64);
        (Self { tx }, rx)
    }
}

impl EventSink for TuiEventSink {
    fn emit(&self, event: &str, _payload: &str) {
        let _ = self.tx.send(event.to_string());
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (sink, _event_rx) = TuiEventSink::new();
    let shared = Arc::new(SharedApp::new(Arc::new(sink)));
    let mut app = App::new(shared);

    // If project path passed as argument, open it immediately
    if let Some(path) = std::env::args().nth(1) {
        app.open_project(&path);
    }

    app.run().await
}
