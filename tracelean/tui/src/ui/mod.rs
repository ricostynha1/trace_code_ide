//! UI rendering — dispatches to panel-specific renderers.

use crate::app::{App, Mode, Panel};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // main content
            Constraint::Length(1), // status bar
            Constraint::Length(1), // input bar (if active)
        ])
        .split(frame.area());

    // Main content area
    render_main(frame, app, chunks[0]);

    // Status bar
    let status_style = Style::default().fg(Color::Black).bg(Color::Cyan);
    let panel_name = match app.active_panel {
        Panel::FileTree => "Files",
        Panel::Editor => "Editor",
        Panel::AiChat => "AI Chat",
        Panel::Trace => "Trace",
        Panel::Requirements => "Reqs",
    };
    let project_info = match &app.project_root {
        Some(p) => p.to_string_lossy().to_string(),
        None => "(no project)".to_string(),
    };
    let status_text = format!(
        " [{}] │ {} │ ? for help",
        panel_name, project_info
    );
    let status_bar = Paragraph::new(status_text).style(status_style);
    frame.render_widget(status_bar, chunks[1]);

    // Input / message bar
    let input_bar = match app.mode {
        Mode::Input => {
            let text = format!("{}{}", app.status_msg, app.input_buffer);
            Paragraph::new(text).style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
        }
        Mode::Normal => {
            Paragraph::new(app.status_msg.as_str())
                .style(Style::default().fg(Color::Gray))
        }
    };
    frame.render_widget(input_bar, chunks[2]);
}

fn render_main(frame: &mut Frame, app: &App, area: Rect) {
    match app.active_panel {
        Panel::FileTree => render_file_tree(frame, app, area),
        Panel::Editor => render_editor(frame, app, area),
        Panel::AiChat => render_placeholder(frame, "AI Chat", "AI chat panel — coming soon. Use GUI for now.", area),
        Panel::Trace => render_placeholder(frame, "Traceability", "Trace graph panel — coming soon. Use GUI for now.", area),
        Panel::Requirements => render_requirements(frame, app, area),
    }
}

fn render_file_tree(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" File Tree (Enter=open, Backspace=up, j/k=navigate) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    if app.project_root.is_none() {
        let welcome = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled("  TraceLean IDE", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from("  No project open."),
            Line::from(""),
            Line::from(Span::styled("  Keybindings:", Style::default().add_modifier(Modifier::BOLD))),
            Line::from("    o         Open a project folder"),
            Line::from("    Tab       Switch panel"),
            Line::from("    Shift+Tab Switch panel (reverse)"),
            Line::from("    Alt+1-5   Jump to panel"),
            Line::from("    j/k ↑↓    Navigate lists / scroll"),
            Line::from("    Enter     Open file / confirm"),
            Line::from("    Backspace Go up directory"),
            Line::from("    ?         Show help in status bar"),
            Line::from("    q         Quit"),
            Line::from(""),
            Line::from(Span::styled("  Press 'o' to open a project.", Style::default().fg(Color::Yellow))),
        ])
        .block(block);
        frame.render_widget(welcome, area);
        return;
    }

    if app.file_entries.is_empty() {
        let empty = Paragraph::new("  (empty directory)")
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .file_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let icon = if entry.is_dir { "📁 " } else { "  " };
            let name = format!("{}{}", icon, entry.name);
            let style = if i == app.file_cursor {
                Style::default().fg(Color::Black).bg(Color::White)
            } else if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(name).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_editor(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.current_file {
        Some(f) => format!(" Editor: {} (j/k=scroll, PgUp/PgDn) ", f),
        None => " Editor (no file open — select from File Tree) ".to_string(),
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    match &app.file_content {
        Some(content) => {
            let lines: Vec<Line> = content
                .lines()
                .enumerate()
                .map(|(i, line)| {
                    let num = Span::styled(
                        format!("{:4} │ ", i + 1),
                        Style::default().fg(Color::DarkGray),
                    );
                    let text = Span::raw(line);
                    Line::from(vec![num, text])
                })
                .collect();

            let para = Paragraph::new(lines)
                .block(block)
                .scroll((app.scroll_offset, 0))
                .wrap(Wrap { trim: false });
            frame.render_widget(para, area);
        }
        None => {
            let hint = Paragraph::new(vec![
                Line::from(""),
                Line::from("  No file open."),
                Line::from("  Switch to File Tree (Alt+1 or Tab) and select a file."),
            ])
            .block(block);
            frame.render_widget(hint, area);
        }
    }
}

fn render_requirements(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Requirements ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    if app.project_root.is_none() {
        let hint = Paragraph::new("  Open a project first (press 'o')").block(block);
        frame.render_widget(hint, area);
        return;
    }

    // Try to list requirements
    let reqs = {
        let state = app.shared.state.lock().unwrap();
        tracelean_core::service::list_requirements(&state)
    };

    match reqs {
        Ok(reqs) if !reqs.is_empty() => {
            let items: Vec<ListItem> = reqs
                .iter()
                .map(|r| {
                    let line = format!("  [{}] {} — {}", r.status.as_str(), r.id, r.title);
                    ListItem::new(line)
                })
                .collect();
            let list = List::new(items).block(block);
            frame.render_widget(list, area);
        }
        Ok(_) => {
            let hint = Paragraph::new("  No requirements found (reqs/ directory empty)").block(block);
            frame.render_widget(hint, area);
        }
        Err(e) => {
            let hint = Paragraph::new(format!("  Error: {}", e)).block(block);
            frame.render_widget(hint, area);
        }
    }
}

fn render_placeholder(frame: &mut Frame, title: &str, msg: &str, area: Rect) {
    let block = Block::default()
        .title(format!(" {} ", title))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let para = Paragraph::new(vec![
        Line::from(""),
        Line::from(format!("  {}", msg)),
    ])
    .block(block);
    frame.render_widget(para, area);
}
