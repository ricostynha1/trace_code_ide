//! App — main event loop (Elm architecture: Event → Update → Render).

use crate::input;
use crate::ui;
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::event::{EnableMouseCapture, DisableMouseCapture};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tracelean_core::{service, FileEntry, SharedApp};

pub struct App {
    pub shared: Arc<SharedApp>,
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
    pub fn new(shared: Arc<SharedApp>) -> Self {
        Self {
            shared,
            running: true,
            active_panel: Panel::FileTree,
            mode: Mode::Normal,
            project_root: None,
            file_entries: Vec::new(),
            file_cursor: 0,
            current_file: None,
            file_content: None,
            scroll_offset: 0,
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
                self.active_panel = Panel::Editor;
                self.status_msg = format!("Viewing: {}", path);
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

    pub async fn run(&mut self) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Main loop
        while self.running {
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
