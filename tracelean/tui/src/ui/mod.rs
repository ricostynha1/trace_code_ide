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
        Panel::AiChat => render_ai_chat(frame, app, area),
        Panel::Trace => render_placeholder(frame, "Traceability", "Trace graph panel — coming soon. Use GUI for now.", area),
        Panel::Requirements => render_requirements(frame, app, area),
    }
}

/// AI chat (P8 tier 1): transcript + tool chips + cost bar + input line —
/// rendered straight from the shared core state the GUI uses.
fn render_ai_chat(frame: &mut Frame, app: &App, area: Rect) {
    use crate::app::ChatEntry;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // transcript
            Constraint::Length(1), // cost/context bar
            Constraint::Length(3), // input
        ])
        .split(area);

    let block = Block::default()
        .title(" AI Chat (type + Enter to send) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let mut lines: Vec<Line> = Vec::new();
    {
        let entries = app.chat_entries.lock().unwrap();
        for entry in entries.iter() {
            match entry {
                ChatEntry::User(text) => {
                    lines.push(Line::from(Span::styled(
                        format!("you ▸ {}", text),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    )));
                }
                ChatEntry::Assistant(text) => {
                    for (i, l) in text.lines().enumerate() {
                        let prefix = if i == 0 { "ai  ▸ " } else { "      " };
                        lines.push(Line::from(format!("{}{}", prefix, l)));
                    }
                }
                ChatEntry::Chip { tool, status, .. } => {
                    let color = if status.starts_with('✗') {
                        Color::Red
                    } else if status == "…" {
                        Color::Yellow
                    } else {
                        Color::Green
                    };
                    lines.push(Line::from(Span::styled(
                        format!("  [⚙ {} {}]", tool, status),
                        Style::default().fg(color),
                    )));
                }
                ChatEntry::Info(text) => {
                    lines.push(Line::from(Span::styled(
                        format!("  {}", text),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                ChatEntry::Error(text) => {
                    lines.push(Line::from(Span::styled(
                        format!("  error: {}", text),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
        }
    }
    if app.chat_busy.load(std::sync::atomic::Ordering::SeqCst) {
        lines.push(Line::from(Span::styled(
            "  thinking…",
            Style::default().fg(Color::Yellow),
        )));
    }
    // Keep the tail visible.
    let inner_height = chunks[0].height.saturating_sub(2) as usize;
    let scroll = lines.len().saturating_sub(inner_height) as u16;
    let para = Paragraph::new(lines)
        .block(block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, chunks[0]);

    // Cost / model bar (P7 data from shared stats + settings)
    let (cost, cap, model) = {
        let stats = app.shared.ai_stats.lock().unwrap();
        let settings = app.shared.ai_settings.lock().unwrap();
        (
            stats.total_cost_usd,
            settings.spend_cap_usd,
            settings
                .selected_model
                .as_ref()
                .map(|m| m.display_name.clone())
                .unwrap_or_else(|| "(no model — set one in the GUI or ~/.tracelean/ai_settings.json)".into()),
        )
    };
    let bar = Paragraph::new(format!(" {} │ session ${:.4} / cap ${:.2}", model, cost, cap))
        .style(Style::default().fg(Color::Black).bg(Color::Gray));
    frame.render_widget(bar, chunks[1]);

    // Input line
    let input = Paragraph::new(format!("> {}", app.chat_input))
        .block(Block::default().borders(Borders::ALL).title(" message "))
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(input, chunks[2]);
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
        Some(f) => {
            let mode = if app.insert_mode { "INSERT" } else { "NORMAL" };
            format!(
                " Editor: {} [{} {}:{}] (i=edit, u=undo, r=redo, s=save) ",
                f,
                mode,
                app.cursor_line + 1,
                app.cursor_col + 1
            )
        }
        None => " Editor (no file open — select from File Tree) ".to_string(),
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.insert_mode { Color::Yellow } else { Color::Green }));

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
                    // Highlight the cursor line; mark the cursor column in insert mode.
                    if i == app.cursor_line {
                        let col = app.cursor_col.min(line.chars().count());
                        let before: String = line.chars().take(col).collect();
                        let at: String = line.chars().nth(col).map(|c| c.to_string()).unwrap_or_else(|| " ".into());
                        let after: String = line.chars().skip(col + 1).collect();
                        let cursor_style = if app.insert_mode {
                            Style::default().fg(Color::Black).bg(Color::Yellow)
                        } else {
                            Style::default().fg(Color::Black).bg(Color::White)
                        };
                        return Line::from(vec![
                            num,
                            Span::raw(before),
                            Span::styled(at, cursor_style),
                            Span::raw(after),
                        ]);
                    }
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
