//! Input handling — maps key events to app actions.

use crate::app::{App, Mode, Panel};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

pub fn handle_key(app: &mut App, key: KeyEvent) {
    match app.mode {
        Mode::Input => handle_input_mode(app, key),
        Mode::Normal => {
            // Editor insert mode captures everything except Esc (P8 tier 1).
            if app.active_panel == Panel::Editor && app.insert_mode {
                handle_editor_insert(app, key);
                return;
            }
            // Chat input captures printable keys (P8 tier 1).
            if app.active_panel == Panel::AiChat && handle_chat_key(app, key) {
                return;
            }
            handle_normal_mode(app, key);
        }
    }
}

fn handle_editor_insert(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.insert_mode = false;
            app.status_msg = "-- NORMAL --  u=undo r=redo s=save i=edit".into();
        }
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.editor_insert(&c.to_string());
        }
        KeyCode::Enter => app.editor_insert("\n"),
        KeyCode::Backspace => app.editor_backspace(),
        KeyCode::Up => {
            app.cursor_line = app.cursor_line.saturating_sub(1);
            app.refresh_editor_content();
        }
        KeyCode::Down => {
            app.cursor_line += 1;
            app.refresh_editor_content();
        }
        KeyCode::Left => {
            app.cursor_col = app.cursor_col.saturating_sub(1);
        }
        KeyCode::Right => {
            app.cursor_col += 1;
            app.refresh_editor_content();
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.editor_save();
        }
        _ => {}
    }
}

/// Returns true if the key was consumed by the chat input.
fn handle_chat_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c)
            if !key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.chat_input.push(c);
            true
        }
        KeyCode::Backspace => {
            app.chat_input.pop();
            true
        }
        KeyCode::Enter => {
            app.send_chat();
            true
        }
        _ => false,
    }
}

fn handle_input_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            let path = app.input_buffer.clone();
            app.input_buffer.clear();
            app.open_project(&path);
        }
        KeyCode::Esc => {
            app.input_buffer.clear();
            app.mode = Mode::Normal;
            app.status_msg = String::from("Cancelled.");
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
        }
        _ => {}
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        // Quit
        KeyCode::Char('q') => {
            if key.modifiers.contains(KeyModifiers::CONTROL) || app.active_panel != Panel::AiChat {
                app.running = false;
            }
        }

        // Open project prompt
        KeyCode::Char('o') => {
            app.mode = Mode::Input;
            app.input_buffer.clear();
            app.status_msg = String::from("Enter project path: ");
        }

        // Panel switching
        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.active_panel = Panel::FileTree;
        }
        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.active_panel = Panel::Editor;
        }
        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.active_panel = Panel::AiChat;
        }
        KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.active_panel = Panel::Trace;
        }
        KeyCode::Char('5') if key.modifiers.contains(KeyModifiers::ALT) => {
            app.active_panel = Panel::Requirements;
        }
        KeyCode::Tab => {
            app.active_panel = match app.active_panel {
                Panel::FileTree => Panel::Editor,
                Panel::Editor => Panel::AiChat,
                Panel::AiChat => Panel::Trace,
                Panel::Trace => Panel::Requirements,
                Panel::Requirements => Panel::FileTree,
            };
        }
        KeyCode::BackTab => {
            app.active_panel = match app.active_panel {
                Panel::FileTree => Panel::Requirements,
                Panel::Editor => Panel::FileTree,
                Panel::AiChat => Panel::Editor,
                Panel::Trace => Panel::AiChat,
                Panel::Requirements => Panel::Trace,
            };
        }

        // File Tree navigation
        KeyCode::Up | KeyCode::Char('k') if app.active_panel == Panel::FileTree => {
            if app.file_cursor > 0 {
                app.file_cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') if app.active_panel == Panel::FileTree => {
            if app.file_cursor + 1 < app.file_entries.len() {
                app.file_cursor += 1;
            }
        }
        KeyCode::Enter if app.active_panel == Panel::FileTree => {
            app.open_selected_file();
        }
        KeyCode::Backspace if app.active_panel == Panel::FileTree => {
            app.go_up_directory();
        }

        // Editor (P8 tier 1): modal editing over AppState::apply
        KeyCode::Char('i') if app.active_panel == Panel::Editor && app.file_content.is_some() => {
            app.insert_mode = true;
            app.status_msg = "-- INSERT --  Esc=normal, Ctrl+S=save".into();
        }
        KeyCode::Char('u') if app.active_panel == Panel::Editor => {
            app.editor_undo();
        }
        KeyCode::Char('r') if app.active_panel == Panel::Editor => {
            app.editor_redo();
        }
        KeyCode::Char('s') if app.active_panel == Panel::Editor => {
            app.editor_save();
        }

        // Editor scrolling
        KeyCode::Up | KeyCode::Char('k') if app.active_panel == Panel::Editor => {
            if app.scroll_offset > 0 {
                app.scroll_offset -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') if app.active_panel == Panel::Editor => {
            app.scroll_offset += 1;
        }
        KeyCode::PageUp if app.active_panel == Panel::Editor => {
            app.scroll_offset = app.scroll_offset.saturating_sub(20);
        }
        KeyCode::PageDown if app.active_panel == Panel::Editor => {
            app.scroll_offset += 20;
        }

        // Help
        KeyCode::Char('?') => {
            app.status_msg = String::from(
                "o=open project | Tab/S-Tab=switch panel | j/k=navigate | Enter=select | q=quit | ?=help"
            );
        }

        _ => {}
    }
}

pub fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    if app.mode != Mode::Normal {
        return;
    }

    match mouse.kind {
        // Scroll wheel
        MouseEventKind::ScrollUp => match app.active_panel {
            Panel::FileTree => {
                if app.file_cursor > 0 {
                    app.file_cursor -= 1;
                }
            }
            Panel::Editor => {
                app.scroll_offset = app.scroll_offset.saturating_sub(3);
            }
            _ => {}
        },
        MouseEventKind::ScrollDown => match app.active_panel {
            Panel::FileTree => {
                if app.file_cursor + 1 < app.file_entries.len() {
                    app.file_cursor += 1;
                }
            }
            Panel::Editor => {
                app.scroll_offset += 3;
            }
            _ => {}
        },
        // Left click — select item in file tree
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            if app.active_panel == Panel::FileTree && !app.file_entries.is_empty() {
                // row offset: 1 for top border, click row is mouse.row
                let clicked_index = mouse.row.saturating_sub(1) as usize;
                if clicked_index < app.file_entries.len() {
                    app.file_cursor = clicked_index;
                }
            }
        }
        // Double-click — open file (approximated via second Down in short time;
        // crossterm doesn't distinguish double-click, so we won't implement that.
        // Instead: single click selects, Enter opens. This is standard TUI UX.)
        _ => {}
    }
}
