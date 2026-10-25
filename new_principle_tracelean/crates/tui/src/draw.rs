//! @implements REQ-VIEW.text_is_the_content
//! @implements REQ-VIEW.frontend_adds_nothing

use tracelean_core::surface::screen::Rect;
use tracelean_core::surface::view::{actions_at, plain_text, Buffer, Rendering, Role};

static THEME: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();

pub fn use_theme(theme: serde_json::Value) {
    let _ = THEME.set(theme);
}

fn theme() -> &'static serde_json::Value {
    THEME.get_or_init(|| {
        serde_json::from_str(tracelean_editor::theme::SHIPPED).expect("the shipped theme parses")
    })
}

fn truecolour(hex: &str, bold: bool) -> String {
    let digits = hex.trim_start_matches('#');
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2).unwrap_or("ff"), 16).unwrap_or(255);
    let weight = if bold { "1;" } else { "" };
    format!("\u{1b}[0;{weight}38;2;{};{};{}m", channel(0), channel(2), channel(4))
}

fn colour(role: Role) -> String {
    let look = tracelean_core::surface::cells::look_of(theme(), role);
    match look.fg {
        Some(hex) => truecolour(&hex, look.bold),
        None => "\u{1b}[0m".to_string(),
    }
}

pub fn lines(buffer: &Buffer) -> Vec<String> {
    plain_text(buffer.clone())
}

pub fn painted(buffer: &Buffer) -> Vec<String> {
    painted_with(buffer, "")
}

pub fn painted_with(buffer: &Buffer, also: &str) -> Vec<String> {
    let mut out = String::from(also);
    for (offset, character) in buffer.text.chars().enumerate() {
        for span in &buffer.spans {
            if span.start == offset && span.start < span.stop {
                out.push_str(&colour(span.role));
                out.push_str(also);
            }
            if span.stop == offset {
                out.push_str("\u{1b}[0m");
                out.push_str(also);
            }
        }
        out.push(character);
    }
    out.push_str("\u{1b}[0m");
    out.split('\n').map(|line| if line.starts_with(also) { line.to_string() } else { format!("{also}{line}") }).collect()
}

fn offer_positions(buffer: &Buffer) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for span in &buffer.spans {
        if span.start < span.stop && !out.contains(&span.start) {
            out.push(span.start);
        }
    }
    out
}

/// @implements REQ-VIEW.frontend_is_checkable
pub fn render(buffer: &Buffer) -> Rendering {
    let mut offered = Vec::new();
    for offset in offer_positions(buffer) {
        for action in actions_at(buffer.clone(), offset) {
            offered.push((offset, action));
        }
    }
    Rendering { lines: lines(buffer), offered }
}

pub type Cell = (char, Role);

fn cells_of(buffer: &Buffer) -> Vec<Vec<Cell>> {
    let role_at = |offset: usize| -> Role {
        buffer
            .spans
            .iter()
            .find(|span| span.start <= offset && offset < span.stop)
            .map(|span| span.role)
            .unwrap_or(Role::Plain)
    };
    let mut lines = vec![Vec::new()];
    for (offset, character) in buffer.text.chars().enumerate() {
        if character == '\n' {
            lines.push(Vec::new());
            continue;
        }
        lines.last_mut().expect("a line").push((character, role_at(offset)));
    }
    lines
}

pub fn chrome(region: Rect, at: Rect) -> (u64, u64) {
    let right = u64::from(at.left + at.width < region.left + region.width);
    let below = u64::from(at.top + at.height < region.top + region.height);
    (right, below)
}

pub fn compose(region: Rect, panes: &[(Rect, Buffer, bool)]) -> Vec<Vec<Cell>> {
    let width = region.width as usize;
    let height = region.height as usize;
    let mut grid = vec![vec![(' ', Role::Plain); width]; height];
    for (at, buffer, focused) in panes {
        let left = at.left.saturating_sub(region.left) as usize;
        let top = at.top.saturating_sub(region.top) as usize;
        let (spare_right, spare_below) = chrome(region, *at);
        let room = (at.width.saturating_sub(spare_right) as usize).min(width.saturating_sub(left));
        let rows = (at.height.saturating_sub(spare_below) as usize).min(height.saturating_sub(top));
        for (row, line) in cells_of(buffer).into_iter().take(rows).enumerate() {
            for (column, cell) in line.into_iter().take(room).enumerate() {
                grid[top + row][left + column] = cell;
            }
        }
        let edge = if *focused { Role::Heading } else { Role::Plain };
        if spare_right == 1 && at.width > 0 {
            let column = left + at.width as usize - 1;
            for row in 0..at.height as usize {
                if top + row < height && column < width {
                    grid[top + row][column] = ('│', edge);
                }
            }
        }
        if spare_below == 1 && at.height > 0 {
            let row = top + at.height as usize - 1;
            for column in 0..at.width as usize {
                if row < height && left + column < width {
                    grid[row][left + column] = ('─', edge);
                }
            }
        }
    }
    for (at, _, _) in panes.iter().filter(|p| p.2) {
        let left = at.left.saturating_sub(region.left) as usize;
        let top = at.top.saturating_sub(region.top) as usize;
        if left > 0 {
            for row in top..(top + at.height as usize).min(height) {
                if grid[row][left - 1].0 == '│' {
                    grid[row][left - 1].1 = Role::Heading;
                }
            }
        }
        if top > 0 {
            for column in left..(left + at.width as usize).min(width) {
                if grid[top - 1][column].0 == '─' {
                    grid[top - 1][column].1 = Role::Heading;
                }
            }
        }
    }
    grid
}

#[allow(dead_code)]
pub fn text_grid(grid: &[Vec<Cell>]) -> Vec<String> {
    grid.iter().map(|row| row.iter().map(|(character, _)| *character).collect()).collect()
}

pub fn paint_marked(grid: &[Vec<Cell>], marked: impl Fn(usize, usize) -> bool) -> Vec<String> {
    grid.iter()
        .enumerate()
        .map(|(y, row)| {
            let mut out = String::new();
            let mut current: Option<(Role, bool)> = None;
            for (x, (character, role)) in row.iter().enumerate() {
                let now = (*role, marked(y, x));
                if current != Some(now) {
                    out.push_str(&colour(*role));
                    if now.1 {
                        out.push_str("\u{1b}[7m");
                    }
                    current = Some(now);
                }
                out.push(*character);
            }
            out.push_str("\u{1b}[0m");
            out
        })
        .collect()
}

/// @implements REQ-VIEW.screen_is_readable
pub fn screen(buffer: &Buffer) -> String {
    let mut out = String::from("\u{1b}[2J");
    for line in painted(buffer) {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("\u{1b}[0m\u{0}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracelean_core::surface::view::conformance;

    fn listing() -> Buffer {
        tracelean_core::surface::view::directory_buffer(
            "src".into(),
            vec![(0, "main.rs".into()), (1, "é.rs".into())],
        )
    }

    #[test]
    fn painting_adds_colour_and_no_characters() {
        let buffer = listing();
        let stripped: Vec<String> = painted(&buffer)
            .iter()
            .map(|line| {
                let mut out = String::new();
                let mut chars = line.chars().peekable();
                while let Some(c) = chars.next() {
                    if c != '\u{1b}' {
                        out.push(c);
                        continue;
                    }
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        for c in chars.by_ref() {
                            if c.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    }
                }
                out
            })
            .collect();
        assert_eq!(stripped, lines(&buffer));
    }

    #[test]
    fn drawing_the_buffer_is_conformant() {
        let buffer = listing();
        assert_eq!(conformance(buffer.clone(), render(&buffer)), vec![]);
    }

    fn note(id: &str, text: &str) -> Buffer {
        Buffer {
            id: id.into(),
            kind: tracelean_core::surface::view::BufferKind::Menu { title: id.into() },
            text: text.into(),
            spans: Vec::new(),
        }
    }

    /// @tests REQ-VIEW.screen_is_readable
    #[test]
    fn each_pane_reads_back_its_own_buffer() {
        let region = Rect { left: 0, top: 0, width: 12, height: 2 };
        let left = Rect { left: 0, top: 0, width: 6, height: 2 };
        let right = Rect { left: 6, top: 0, width: 6, height: 2 };
        let grid = compose(
            region,
            &[(left, note("l", "one\ntwo"), true), (right, note("r", "abc\nde"), false)],
        );
        let rows = text_grid(&grid);
        assert_eq!(rows, vec!["one  │abc   ".to_string(), "two  │de    ".to_string()]);
    }

    /// @tests REQ-VIEW.screen_is_readable
    #[test]
    fn a_long_line_stops_at_the_divider() {
        let region = Rect { left: 0, top: 0, width: 10, height: 1 };
        let left = Rect { left: 0, top: 0, width: 5, height: 1 };
        let right = Rect { left: 5, top: 0, width: 5, height: 1 };
        let grid = compose(
            region,
            &[(left, note("l", "aaaaaaaaaa"), false), (right, note("r", "bbbbb"), false)],
        );
        assert_eq!(text_grid(&grid), vec!["aaaa│bbbbb".to_string()]);
    }

    /// @tests REQ-VIEW.text_is_the_content
    #[test]
    fn painting_a_grid_adds_colour_and_no_characters() {
        let region = Rect { left: 0, top: 0, width: 8, height: 1 };
        let grid = compose(region, &[(region, listing(), true)]);
        let stripped: Vec<String> = paint_marked(&grid, |_, _| false)
            .iter()
            .map(|line| {
                let mut out = String::new();
                let mut chars = line.chars().peekable();
                while let Some(c) = chars.next() {
                    if c != '\u{1b}' {
                        out.push(c);
                        continue;
                    }
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        for c in chars.by_ref() {
                            if c.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    }
                }
                out
            })
            .collect();
        assert_eq!(stripped, text_grid(&grid));
    }
}
