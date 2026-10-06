//! Drawing a buffer as text.
//!
//! Pure, and the only place this frontend decides what the screen says. The
//! interactive loop, the answer the conformance harness reads, and the screen
//! the capture harness reads all come through here — which is what stops this
//! frontend from reporting one thing and painting another.
//!
//! What it may decide is how a region *looks*: a role becomes a colour. What it
//! may not do is change the text or offer an action no span declares, and
//! neither is possible here because both come from the buffer.
//!
//! @implements REQ-VIEW.text_is_the_content
//! @implements REQ-VIEW.frontend_adds_nothing

use tracelean_core::evidence::Level;
use tracelean_core::surface::screen::Rect;
use tracelean_core::surface::view::{actions_at, plain_text, Buffer, Rendering, Role};

/// The theme this frontend paints with, set once when it starts.
///
/// Unset — in the conformance modes and in tests — it is the shipped theme, so
/// what a harness reads is painted the way a person would see it.
static THEME: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();

/// Paint with this theme from now on. Only the first call counts.
pub fn use_theme(theme: serde_json::Value) {
    let _ = THEME.set(theme);
}

fn theme() -> &'static serde_json::Value {
    THEME.get_or_init(|| {
        serde_json::from_str(tracelean_editor::theme::SHIPPED).expect("the shipped theme parses")
    })
}

/// `#rrggbb` as a terminal's 24-bit foreground.
fn truecolour(hex: &str, bold: bool) -> String {
    let digits = hex.trim_start_matches('#');
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2).unwrap_or("ff"), 16).unwrap_or(255);
    let weight = if bold { "1;" } else { "" };
    format!("\u{1b}[0;{weight}38;2;{};{};{}m", channel(0), channel(2), channel(4))
}

/// A role becomes a colour. The role says what a region *is*; the theme
/// (`assets/theme.json`, `roles`) says what colour that is, which is the
/// frontend's business and nobody else's — and the window reads the same file,
/// so the two cannot disagree about it.
///
/// A level is coloured by its grade, and the four are meant to be told apart at
/// a glance. The grade comes from the span — reading `L3` out of the text to
/// decide would be this frontend parsing the buffer.
fn colour(role: Role) -> String {
    // A token is coloured from the theme's `syntax` section, by the kind the
    // core parsed it as, spelled as it is on the wire.
    if let Role::Token { kind } = role {
        let name = serde_json::to_value(kind).ok();
        let key = name.as_ref().and_then(|v| v.as_str()).unwrap_or("");
        return match tracelean_editor::theme::colour(theme(), "syntax", key) {
            Some(hex) => truecolour(hex, false),
            None => "\u{1b}[0m".to_string(),
        };
    }
    let (key, bold) = match role {
        Role::Token { .. } => return "\u{1b}[0m".to_string(),
        Role::Plain => return "\u{1b}[0m".to_string(),
        Role::Path => ("path", false),
        Role::Entry => ("entry", false),
        Role::Heading => ("heading", true),
        Role::Requirement => ("requirement", false),
        Role::Level { grade } => match grade {
            Level::L1 => ("levelL1", false),
            Level::L2 => ("levelL2", false),
            Level::L3 => ("levelL3", false),
            Level::L4 => ("levelL4", true),
        },
        Role::Added => ("added", false),
        Role::Removed => ("removed", false),
    };
    match tracelean_editor::theme::colour(theme(), "roles", key) {
        Some(hex) => truecolour(hex, bold),
        None => "\u{1b}[0m".to_string(),
    }
}

/// The buffer's lines, exactly as the buffer says them.
pub fn lines(buffer: &Buffer) -> Vec<String> {
    plain_text(buffer.clone())
}

/// The same lines with the escape sequences a terminal needs to colour them.
///
/// Every escape inserted is one a terminal acts on rather than prints, so the
/// visible text is unchanged — which is what the capture harness checks.
pub fn painted(buffer: &Buffer) -> Vec<String> {
    let mut out = String::new();
    for (offset, character) in buffer.text.chars().enumerate() {
        for span in &buffer.spans {
            if span.start == offset && span.start < span.stop {
                out.push_str(&colour(span.role));
            }
            if span.stop == offset {
                out.push_str("\u{1b}[0m");
            }
        }
        out.push(character);
    }
    out.push_str("\u{1b}[0m");
    out.split('\n').map(str::to_string).collect()
}

/// Where this frontend offers actions: one position per span, which is where
/// the thing the span is about starts.
fn offer_positions(buffer: &Buffer) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for span in &buffer.spans {
        if span.start < span.stop && !out.contains(&span.start) {
            out.push(span.start);
        }
    }
    out
}

/// What this frontend drew, in the form the conformance harness checks.
///
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

// ─────────────────────────────────────────────── several panes at once

/// One cell of a composed screen: a character and what it is.
///
/// The role travels with the character so that colour survives composition. A
/// grid of plain characters would have meant a terminal showing two buffers
/// lost the colouring it has when showing one, and the role is the only thing
/// that says what a region *is*.
pub type Cell = (char, Role);

/// The character at each offset of a buffer's text, with the role of the span
/// covering it, split into lines.
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

/// How much of a pane this frontend spends on its own chrome: a column for the
/// divider on its right, a row for the one below it, where it has a neighbour
/// there at all.
///
/// A terminal has no borders of its own, so a divider has to come out of a
/// pane. Which pane it comes out of is this frontend's business — a window
/// draws a line between panes and spends nothing — and it is reported rather
/// than taken quietly: the caller windows the buffer to the room that is left,
/// so nothing is drawn over.
pub fn chrome(region: Rect, at: Rect) -> (u64, u64) {
    let right = u64::from(at.left + at.width < region.left + region.width);
    let below = u64::from(at.top + at.height < region.top + region.height);
    (right, below)
}

/// Lay several panes out into one grid of cells.
///
/// Each pane's buffer is drawn into its own rectangle and clipped to it — a
/// terminal cannot show a line wider than the pane holding it, and a pane that
/// overflowed into its neighbour would be one buffer drawing another's text.
/// The clip is why the caller windows vertically first: between them, what is
/// drawn is exactly what fits.
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
        // The divider, and the focused pane's is brighter — a heading, because
        // that is the role the stylesheet of a terminal already makes stand
        // out, and inventing a role for it would put something in the
        // vocabulary that no buffer can carry.
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
    grid
}

/// A composed grid as plain lines: what a person reads off the screen.
///
/// The interactive loop paints rather than reads, so nothing in the binary
/// calls this — what calls it is the suite that checks a composed screen says
/// what the buffers in it say, which is the one thing the single-buffer
/// conformance check cannot see.
#[allow(dead_code)]
pub fn text_grid(grid: &[Vec<Cell>]) -> Vec<String> {
    grid.iter().map(|row| row.iter().map(|(character, _)| *character).collect()).collect()
}

/// A composed grid with the escape sequences a terminal needs to colour it.
///
/// One escape per run of a role rather than one per character, so the bytes
/// stay readable and the visible text is still the grid's own.
///
/// The cells `marked` says are selected are drawn in reverse video, the one
/// mark every terminal draws without a colour of its own.
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

/// The bytes a terminal receives for one screen.
///
/// The markers are the ones the capture harness reads: a clear at the front and
/// a NUL at the end, so one painted screen can be told from the next.
///
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

    /// Colour is added and text is not: what a terminal would print is the
    /// buffer's own text.
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

    /// And what it says it drew is what the buffer says.
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

    /// Two panes side by side, and each reads back its own buffer.
    ///
    /// The composition is where a tiling frontend can quietly lose text — a
    /// pane overflowing into its neighbour, a divider drawn over a line, an
    /// off-by-one in the clip — and none of that shows up in the single-buffer
    /// conformance check, because none of it happens to a single buffer.
    ///
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
        // Five columns of text and the sixth a divider; the right-hand pane
        // reaches the region's edge, so it keeps all six.
        assert_eq!(rows, vec!["one  │abc   ".to_string(), "two  │de    ".to_string()]);
    }

    /// A line wider than its pane is cut at the pane's edge, not drawn over the
    /// pane beside it.
    ///
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

    /// Painting a grid adds colour and no characters, the same guarantee
    /// `painted` gives for one buffer.
    ///
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
