//! App — main event loop (Elm architecture: Event → Update → Render).

use crate::input;
use crate::ui;
use crate::EventQueue;
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::event::{EnableMouseCapture, DisableMouseCapture};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracelean_core::commands::Command;
use tracelean_core::{service, FileEntry, SharedApp, ToolCallEvent, ToolCallStatus};

/// One entry in the AI chat transcript (P8 tier 1).
#[derive(Debug, Clone)]
pub enum ChatEntry {
    User(String),
    Assistant(String),
    /// Tool-call chip, updated in place by call_id (same events as the GUI).
    Chip {
        tool: String,
        status: String, // "…" running, "✓" done, "✗ <err>" failed
        call_id: Option<String>,
    },
    Info(String),
    Error(String),
}

pub struct App {
    pub shared: Arc<SharedApp>,
    pub events: EventQueue,
    pub running: bool,
    pub active_panel: Panel,
    pub mode: Mode,
    // Project
    pub project_root: Option<PathBuf>,
    pub file_entries: Vec<FileEntry>,
    pub file_cursor: usize,
    // Editor
    pub current_file: Option<String>,
    pub file_content: Option<String>,
    pub scroll_offset: u16,
    pub insert_mode: bool,
    pub cursor_line: usize,
    pub cursor_col: usize,
    // AI chat
    pub chat_input: String,
    pub chat_entries: Arc<Mutex<Vec<ChatEntry>>>,
    pub chat_busy: Arc<AtomicBool>,
    // Input
    pub input_buffer: String,
    pub status_msg: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    FileTree,
    Editor,
    AiChat,
    Trace,
    Requirements,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Input,  // typing in input_buffer (e.g. path prompt)
}

impl App {
    pub fn new(shared: Arc<SharedApp>, events: EventQueue) -> Self {
        Self {
            shared,
            events,
            running: true,
            active_panel: Panel::FileTree,
            mode: Mode::Normal,
            project_root: None,
            file_entries: Vec::new(),
            file_cursor: 0,
            current_file: None,
            file_content: None,
            scroll_offset: 0,
            insert_mode: false,
            cursor_line: 0,
            cursor_col: 0,
            chat_input: String::new(),
            chat_entries: Arc::new(Mutex::new(Vec::new())),
            chat_busy: Arc::new(AtomicBool::new(false)),
            input_buffer: String::new(),
            status_msg: String::from("Press 'o' to open a project folder, 'q' or Ctrl+Q to quit"),
        }
    }

    pub fn open_project(&mut self, path: &str) {
        let result = {
            let mut state = self.shared.state.lock().unwrap();
            let mut symbols = self.shared.symbols.lock().unwrap();
            service::open_project(&mut state, &mut symbols, path)
        };
        match result {
            Ok(_) => {
                self.project_root = Some(PathBuf::from(path));
                self.refresh_file_list();
                self.status_msg = format!("Opened: {}", path);
                self.mode = Mode::Normal;
            }
            Err(e) => {
                self.status_msg = format!("Error: {}", e);
                self.mode = Mode::Normal;
            }
        }
    }

    pub fn refresh_file_list(&mut self) {
        if let Some(root) = &self.project_root {
            self.file_entries = service::list_directory_files(root, "");
            self.file_cursor = 0;
        }
    }

    pub fn open_selected_file(&mut self) {
        if self.file_entries.is_empty() {
            return;
        }
        let entry = &self.file_entries[self.file_cursor];
        if entry.is_dir {
            // Navigate into directory
            let new_entries = service::list_directory_files(
                self.project_root.as_ref().unwrap(),
                &entry.path,
            );
            if !new_entries.is_empty() {
                self.file_entries = new_entries;
                self.file_cursor = 0;
            }
            return;
        }
        let path = entry.path.clone();
        let mut state = self.shared.state.lock().unwrap();
        match service::open_file(&mut state, &path) {
            Ok(content) => {
                self.current_file = Some(path.clone());
                self.file_content = Some(content);
                self.scroll_offset = 0;
                self.insert_mode = false;
                self.cursor_line = 0;
                self.cursor_col = 0;
                self.active_panel = Panel::Editor;
                self.status_msg = format!("Viewing: {} — 'i' to edit", path);
            }
            Err(e) => {
                self.status_msg = format!("Error opening file: {}", e);
            }
        }
    }

    pub fn go_up_directory(&mut self) {
        // Re-list from project root
        self.refresh_file_list();
    }

    // ─── Editing (P8 tier 1) — every edit is a Replace through AppState ──────

    /// Re-read the buffer from backend state (divergence-free by design).
    pub fn refresh_editor_content(&mut self) {
        if let Some(file) = &self.current_file {
            let state = self.shared.state.lock().unwrap();
            self.file_content = state
                .get_content(&PathBuf::from(file))
                .map(|s| s.to_string());
        }
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        let Some(content) = &self.file_content else { return };
        let lines: Vec<&str> = content.split('\n').collect();
        if self.cursor_line >= lines.len() {
            self.cursor_line = lines.len().saturating_sub(1);
        }
        let line_len = lines.get(self.cursor_line).map(|l| l.chars().count()).unwrap_or(0);
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
        }
    }

    /// Char (code point) offset of the cursor within the buffer.
    fn cursor_char_offset(&self) -> usize {
        let Some(content) = &self.file_content else { return 0 };
        let mut offset = 0;
        for (i, line) in content.split('\n').enumerate() {
            if i == self.cursor_line {
                return offset + self.cursor_col.min(line.chars().count());
            }
            offset += line.chars().count() + 1; // +1 for '\n'
        }
        offset.saturating_sub(1)
    }

    fn apply_edit(&mut self, at: usize, old: String, new: String) {
        let Some(file) = self.current_file.clone() else { return };
        let cmd = Command::Replace { file: PathBuf::from(file), at, old, new };
        let result = {
            let mut state = self.shared.state.lock().unwrap();
            service::apply_command(&mut state, cmd)
        };
        if let Err(e) = result {
            self.status_msg = format!("Edit rejected: {}", e);
        }
        self.refresh_editor_content();
    }

    /// Insert text at the cursor and advance it.
    pub fn editor_insert(&mut self, text: &str) {
        if self.file_content.is_none() {
            return;
        }
        let at = self.cursor_char_offset();
        self.apply_edit(at, String::new(), text.to_string());
        for ch in text.chars() {
            if ch == '\n' {
                self.cursor_line += 1;
                self.cursor_col = 0;
            } else {
                self.cursor_col += 1;
            }
        }
        self.clamp_cursor();
    }

    /// Delete the char before the cursor (joining lines across '\n').
    pub fn editor_backspace(&mut self) {
        let Some(content) = self.file_content.clone() else { return };
        let at = self.cursor_char_offset();
        if at == 0 {
            return;
        }
        let old: String = content.chars().nth(at - 1).map(|c| c.to_string()).unwrap_or_default();
        if old.is_empty() {
            return;
        }
        // Move the cursor first (based on the pre-edit content).
        if old == "\n" {
            self.cursor_line = self.cursor_line.saturating_sub(1);
            self.cursor_col = content
                .split('\n')
                .nth(self.cursor_line)
                .map(|l| l.chars().count())
                .unwrap_or(0);
        } else {
            self.cursor_col = self.cursor_col.saturating_sub(1);
        }
        self.apply_edit(at - 1, old, String::new());
    }

    pub fn editor_undo(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.undo();
        }
        self.refresh_editor_content();
        self.status_msg = "Undo".into();
    }

    pub fn editor_redo(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.redo();
        }
        self.refresh_editor_content();
        self.status_msg = "Redo".into();
    }

    pub fn editor_save(&mut self) {
        let Some(file) = self.current_file.clone() else { return };
        let result = {
            let state = self.shared.state.lock().unwrap();
            let mut symbols = self.shared.symbols.lock().unwrap();
            service::save_file(&state, &mut symbols, &file)
        };
        self.status_msg = match result {
            Ok(()) => format!("Saved {}", file),
            Err(e) => format!("Save failed: {}", e),
        };
    }

    // ─── AI chat (P8 tier 1) — one shared AiService, same events as GUI ──────

    /// Send the current chat input as a turn on session "tui" (async;
    /// completion lands in chat_entries).
    pub fn send_chat(&mut self) {
        let msg = self.chat_input.trim().to_string();
        if msg.is_empty() || self.chat_busy.load(Ordering::SeqCst) {
            return;
        }
        self.chat_input.clear();
        self.chat_entries.lock().unwrap().push(ChatEntry::User(msg.clone()));
        self.chat_busy.store(true, Ordering::SeqCst);

        let svc = self.shared.ai_service();
        let entries = self.chat_entries.clone();
        let busy = self.chat_busy.clone();
        tokio::spawn(async move {
            let result = svc.chat_turn("tui", &msg, None, false).await;
            let mut e = entries.lock().unwrap();
            match result {
                Ok(r) => e.push(ChatEntry::Assistant(r.response.content)),
                Err(err) => e.push(ChatEntry::Error(err)),
            }
            busy.store(false, Ordering::SeqCst);
        });
    }

    /// Drain core events into UI state: tool chips update in place by
    /// call_id (P1 semantics), file changes refresh the editor buffer.
    pub fn drain_events(&mut self) {
        let drained: Vec<(String, String)> = {
            let Ok(mut q) = self.events.lock() else { return };
            q.drain(..).collect()
        };
        for (event, payload) in drained {
            match event.as_str() {
                "tool-call" => {
                    if let Ok(ev) = serde_json::from_str::<ToolCallEvent>(&payload) {
                        let status = match &ev.status {
                            ToolCallStatus::Running => "…".to_string(),
                            ToolCallStatus::Completed => "✓".to_string(),
                            ToolCallStatus::Failed { error } => format!("✗ {}", error),
                        };
                        let mut entries = self.chat_entries.lock().unwrap();
                        let updated = ev.call_id.as_ref().and_then(|id| {
                            entries.iter_mut().rev().find_map(|e| match e {
                                ChatEntry::Chip { call_id: Some(cid), status: s, .. }
                                    if cid == id && s == "…" =>
                                {
                                    *s = status.clone();
                                    Some(())
                                }
                                _ => None,
                            })
                        });
                        if updated.is_none() {
                            entries.push(ChatEntry::Chip {
                                tool: ev.tool_name,
                                status,
                                call_id: ev.call_id,
                            });
                        }
                    }
                }
                "ai-chat-message" => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&payload) {
                        if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                            if !content.is_empty() {
                                self.chat_entries
                                    .lock()
                                    .unwrap()
                                    .push(ChatEntry::Info(content.to_string()));
                            }
                        }
                    }
                }
                "undo-tree-changed" | "files-changed" => {
                    self.refresh_editor_content();
                }
                _ => {}
            }
        }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Panic hook: restore the terminal before printing the panic (D8.2).
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
            default_hook(info);
        }));

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        while self.running {
            self.drain_events();
            terminal.draw(|frame| ui::render(frame, self))?;

            if event::poll(std::time::Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) => input::handle_key(self, key),
                    Event::Mouse(mouse) => input::handle_mouse(self, mouse),
                    _ => {}
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
        terminal.show_cursor()?;

        Ok(())
    }
}
