//! TraceLean TUI — terminal frontend.
//! Shares the same core engine (SharedApp + AiService) as the GUI.
//!
//! Usage:
//!   tracelean-tui [project-path]

use std::sync::Arc;
use tracelean_core::SharedApp;
use tracelean_tui::{app::App, new_event_queue, TuiEventSink};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let events = new_event_queue();
    let sink = TuiEventSink { events: events.clone() };
    let shared = Arc::new(SharedApp::new(Arc::new(sink)));

    // P8: same persisted settings as the GUI (~/.tracelean/ai_settings.json).
    {
        let mut settings = shared.ai_settings.lock().unwrap();
        *settings = tracelean_core::ai::service::load_settings();
    }

    let mut app = App::new(shared, events);

    // If project path passed as argument, open it immediately
    if let Some(path) = std::env::args().nth(1) {
        app.open_project(&path);
    }

    app.run().await
}
