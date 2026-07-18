//! P8 (D8.3) — headless TUI smoke test on ratatui's TestBackend.
//!
//! Drives the real App against the shared core with zero Tauri and zero
//! terminal: open project → open file → type → undo → run a mock-provider
//! chat turn → assert the rendered buffer shows the answer and a tool chip.

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tracelean_core::ai::{ModelConfig, ProviderKind};
use tracelean_core::SharedApp;
use tracelean_tui::app::{App, Panel};
use tracelean_tui::{input, new_event_queue, ui, TuiEventSink};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn render_to_string(app: &App) -> String {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| ui::render(f, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn tui_smoke_edit_undo_and_mock_chat() {
    // Mock provider answers immediately — no interactive queue, no cost.
    std::env::set_var("TRACELEAN_MOCK_AUTO", "mock says hello");

    let events = new_event_queue();
    let shared = Arc::new(SharedApp::new(Arc::new(TuiEventSink {
        events: events.clone(),
    })));
    {
        let mut settings = shared.ai_settings.lock().unwrap();
        settings.active_provider = ProviderKind::Mock;
        settings.selected_model = Some(ModelConfig {
            provider: ProviderKind::Mock,
            model_id: "mock".into(),
            display_name: "Mock".into(),
            max_tokens: 512,
            ..Default::default()
        });
    }

    // Temp project with one file.
    let dir = std::env::temp_dir().join(format!("tracelean-tui-smoke-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hello world\n").unwrap();

    let mut app = App::new(shared.clone(), events.clone());
    app.open_project(dir.to_str().unwrap());
    assert!(!app.file_entries.is_empty(), "project should list files");

    // Open hello.txt (select it first).
    let idx = app
        .file_entries
        .iter()
        .position(|e| e.name == "hello.txt")
        .expect("hello.txt listed");
    app.file_cursor = idx;
    app.open_selected_file();
    assert_eq!(app.active_panel, Panel::Editor);
    assert_eq!(app.file_content.as_deref(), Some("hello world\n"));

    // Type "hi" at the start of the file via the real key handler.
    input::handle_key(&mut app, key(KeyCode::Char('i'))); // insert mode
    input::handle_key(&mut app, key(KeyCode::Char('h')));
    input::handle_key(&mut app, key(KeyCode::Char('i')));
    input::handle_key(&mut app, key(KeyCode::Esc));
    assert_eq!(app.file_content.as_deref(), Some("hihello world\n"));

    // Undo both single-char commands through the shared undo tree.
    input::handle_key(&mut app, key(KeyCode::Char('u')));
    input::handle_key(&mut app, key(KeyCode::Char('u')));
    assert_eq!(app.file_content.as_deref(), Some("hello world\n"));

    // Run a real agent turn on the mock provider (AiService in core).
    app.active_panel = Panel::AiChat;
    app.chat_input = "say hello".into();
    app.send_chat();
    let mut waited = 0;
    while app.chat_busy.load(std::sync::atomic::Ordering::SeqCst) && waited < 100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        waited += 1;
    }
    assert!(waited < 100, "chat turn should complete");

    // A tool chip event, as the runtime emits during turns with tool calls.
    shared.event_sink.emit(
        "tool-call",
        &serde_json::json!({
            "tool_name": "read_file",
            "status": "completed",
            "duration_ms": 3,
            "depth": 0,
            "call_id": "call_1"
        })
        .to_string(),
    );
    app.drain_events();

    let screen = render_to_string(&app);
    assert!(screen.contains("mock says hello"), "assistant answer rendered");
    assert!(screen.contains("read_file"), "tool chip rendered");
    assert!(screen.contains("session $"), "cost bar rendered");

    std::fs::remove_dir_all(&dir).ok();
}
