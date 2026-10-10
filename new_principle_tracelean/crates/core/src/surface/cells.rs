//! What a terminal frontend drew, cell by cell, and whether each cell is the
//! colour the theme gives its role.
//!
//! `drt::frontend::readable` reads a painted screen as text and throws the
//! escapes away. This keeps them: the bytes a terminal received become a grid
//! of cells, each with its character and the style it was drawn in. Only the
//! sequences a frontend here sends are understood — clearing the screen,
//! placing the cursor, and the graphic renditions for weight, dimming, reverse
//! video and 24-bit colour — because a full terminal emulator is a dependency,
//! not a check.
//!
//! With the grid, "a span with role `added` is drawn in the theme's colour for
//! `added`" is a function: `miscoloured` walks the buffer, finds the cell each
//! character landed in and compares the two. A character's role is the role of
//! the first span covering it, as the page and the composed panes have it.
//!
//! @implements REQ-LOOK.roles_drawn_in_theme_colours

use serde::{Deserialize, Serialize};

use super::view::{Buffer, Role};

/// One cell of a terminal: the character and how it was drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub text: String,
    /// `#rrggbb`, or `None` for the terminal's own colour.
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub bold: bool,
    pub dim: bool,
    pub reverse: bool,
}

impl Cell {
    fn blank() -> Cell {
        Cell { text: " ".into(), fg: None, bg: None, bold: false, dim: false, reverse: false }
    }
}

/// How a role is meant to look: its foreground and whether it is bold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub role: Role,
    pub fg: Option<String>,
    pub bold: bool,
}

/// One character drawn otherwise than its role says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Miscoloured {
    pub line: u64,
    pub column: u64,
    pub text: String,
    pub role: Role,
    pub expected: Option<String>,
    pub expected_bold: bool,
    /// What the cell holds, or `None` when the character fell off the grid.
    pub drawn: Option<Cell>,
}

/// A run of decimal digits, saturating rather than wrapping: a parameter too
/// large for any screen is off every screen either way.
fn number(piece: &str, default: u64) -> u64 {
    if piece.is_empty() || !piece.chars().all(|c| c.is_ascii_digit()) {
        return default;
    }
    piece.chars().fold(0u64, |n, c| n.saturating_mul(10).saturating_add(u64::from(c as u8 - b'0')))
}

fn hex(r: u64, g: u64, b: u64) -> String {
    format!("#{:02x}{:02x}{:02x}", r.min(255), g.min(255), b.min(255))
}

/// The style a graphic rendition leaves, applied code by code.
fn rendition(style: &mut Cell, params: &str) {
    let codes: Vec<u64> = params.split(';').map(|p| number(p, 0)).collect();
    let mut at = 0;
    while at < codes.len() {
        match codes[at] {
            0 => {
                let text = std::mem::take(&mut style.text);
                *style = Cell { text, ..Cell::blank() };
            }
            1 => style.bold = true,
            2 => style.dim = true,
            22 => (style.bold, style.dim) = (false, false),
            7 => style.reverse = true,
            27 => style.reverse = false,
            39 => style.fg = None,
            49 => style.bg = None,
            ground @ (38 | 48) => match codes.get(at + 1) {
                Some(2) if at + 4 < codes.len() => {
                    let colour = Some(hex(codes[at + 2], codes[at + 3], codes[at + 4]));
                    if ground == 38 {
                        style.fg = colour;
                    } else {
                        style.bg = colour;
                    }
                    at += 4;
                }
                // A palette index: understood as skipped, not as a colour.
                Some(5) if at + 2 < codes.len() => at += 2,
                // Malformed: the rest of the sequence means nothing.
                _ => return,
            },
            _ => {}
        }
        at += 1;
    }
}

/// The cells a terminal of `rows` by `columns` shows after receiving `painted`.
///
/// As a terminal has it: a character written past the right edge wraps to the
/// next line, unless wrapping was turned off (`ESC[?7l`), when it overwrites
/// the last column; a line feed on the last row scrolls the screen up; the
/// cursor is placed no further than the last row and column. A line feed also
/// starts its line at the first column, as the capture output means it; the
/// terminal frontend sends a carriage return before it anyway.
pub fn grid(painted: String, rows: u64, columns: u64) -> Vec<Vec<Cell>> {
    let mut cells = vec![vec![Cell::blank(); columns as usize]; rows as usize];
    let mut style = Cell::blank();
    let (mut row, mut column) = (0u64, 0u64);
    let mut wraps = true;
    // The next line, scrolling when there is none.
    let down = |cells: &mut Vec<Vec<Cell>>, row: u64| -> u64 {
        if row + 1 < rows {
            return row + 1;
        }
        if rows > 0 {
            cells.remove(0);
            cells.push(vec![Cell::blank(); columns as usize]);
        }
        row
    };
    let mut chars = painted.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    let mut params = String::new();
                    let mut last = None;
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            last = Some(c);
                            break;
                        }
                        params.push(c);
                    }
                    match last {
                        Some('m') => rendition(&mut style, &params),
                        Some('H') => {
                            let mut pieces = params.split(';');
                            row = number(pieces.next().unwrap_or(""), 1).clamp(1, rows.max(1)) - 1;
                            column = number(pieces.next().unwrap_or(""), 1).clamp(1, columns.max(1)) - 1;
                        }
                        Some('J') if params == "2" => {
                            cells = vec![vec![Cell::blank(); columns as usize]; rows as usize];
                        }
                        Some('l') if params == "?7" => wraps = false,
                        Some('h') if params == "?7" => wraps = true,
                        _ => {}
                    }
                }
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                }
                _ => {}
            },
            '\r' => column = 0,
            '\n' => (row, column) = (down(&mut cells, row), 0),
            c if c < ' ' || c == '\u{7f}' => {}
            c => {
                if column >= columns {
                    if wraps {
                        (row, column) = (down(&mut cells, row), 0);
                    } else {
                        column = columns.saturating_sub(1);
                    }
                }
                if row < rows && column < columns {
                    cells[row as usize][column as usize] = Cell { text: c.to_string(), ..style.clone() };
                }
                column += 1;
            }
        }
    }
    cells
}

/// Every character of `buffer`, drawn from the top-left of `cells`, whose cell
/// is not in the look its role has; a role with no look is drawn plain.
pub fn miscoloured(buffer: Buffer, cells: Vec<Vec<Cell>>, looks: Vec<Look>) -> Vec<Miscoloured> {
    let mut out = Vec::new();
    let (mut line, mut column) = (0u64, 0u64);
    for (offset, c) in buffer.text.chars().enumerate() {
        if c == '\n' {
            (line, column) = (line + 1, 0);
            continue;
        }
        let role = buffer
            .spans
            .iter()
            .find(|s| s.start <= offset && offset < s.stop)
            .map(|s| s.role)
            .unwrap_or(Role::Plain);
        let (expected, expected_bold) =
            looks.iter().find(|l| l.role == role).map(|l| (l.fg.clone(), l.bold)).unwrap_or((None, false));
        let drawn = cells.get(line as usize).and_then(|r| r.get(column as usize)).cloned();
        let right = drawn.as_ref().is_some_and(|d| d.fg == expected && d.bold == expected_bold);
        if !right {
            out.push(Miscoloured {
                line,
                column,
                text: c.to_string(),
                role,
                expected,
                expected_bold,
                drawn,
            });
        }
        column += 1;
    }
    out
}

/// The two together: what a frontend that painted `painted` for `buffer` drew
/// in the wrong look, on a terminal of `rows` by `columns`.
///
/// @implements REQ-LOOK.roles_drawn_in_theme_colours
/// @drt REQ-LOOK.roles_drawn_in_theme_colours
pub fn drawn_wrong(buffer: Buffer, painted: String, rows: u64, columns: u64, looks: Vec<Look>) -> Vec<Miscoloured> {
    miscoloured(buffer, grid(painted, rows, columns), looks)
}

/// The look the theme gives a role: tokens from `syntax`, claims from `chips`,
/// the rest from `roles`, with headings, claims and L4 in bold. Plain text, and
/// a role the theme has no colour for, are drawn in the terminal's own colour.
///
/// The colour is spelled as a terminal draws it, lowercase, with a channel
/// that is not hexadecimal read as `ff`.
pub fn look_of(theme: &serde_json::Value, role: Role) -> Look {
    use crate::evidence::Level;
    let named = |section: &str, key: &str| theme.get(section)?.get(key)?.as_str().map(str::to_string);
    let (colour, bold) = match role {
        Role::Plain => (None, false),
        Role::Token { kind } => {
            let key = serde_json::to_value(kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
            (named("syntax", &key), false)
        }
        Role::Claim { role } => (named("chips", role.as_str()), true),
        Role::Path => (named("roles", "path"), false),
        Role::Entry => (named("roles", "entry"), false),
        Role::Heading => (named("roles", "heading"), true),
        Role::Requirement => (named("roles", "requirement"), false),
        Role::Level { grade } => match grade {
            Level::L1 => (named("roles", "levelL1"), false),
            Level::L2 => (named("roles", "levelL2"), false),
            Level::L3 => (named("roles", "levelL3"), false),
            Level::L4 => (named("roles", "levelL4"), true),
        },
        Role::Added => (named("roles", "added"), false),
        Role::Removed => (named("roles", "removed"), false),
    };
    let fg = colour.map(|hexed| {
        let digits = hexed.trim_start_matches('#').to_string();
        let channel = |at: usize| u64::from(u8::from_str_radix(digits.get(at..at + 2).unwrap_or("ff"), 16).unwrap_or(255));
        hex(channel(0), channel(2), channel(4))
    });
    // An uncoloured role is drawn plain, bold or not, as a terminal has it.
    let bold = bold && fg.is_some();
    Look { role, fg, bold }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::Span;

    fn buffer(text: &str, spans: Vec<(usize, usize, Role)>) -> Buffer {
        Buffer {
            id: "b".into(),
            kind: crate::surface::view::BufferKind::File { path: "b".into() },
            text: text.into(),
            spans: spans.into_iter().map(|(start, stop, role)| Span { start, stop, role, actions: vec![] }).collect(),
        }
    }

    /// The escapes a frontend sends become the style of the cells they precede.
    ///
    /// @tests REQ-LOOK.roles_drawn_in_theme_colours
    #[test]
    fn escapes_become_styles_and_positions() {
        let cells = grid("\u{1b}[2J\u{1b}[H\u{1b}[0;1;38;2;1;2;255mab\u{1b}[0m c\r\n\u{1b}[7md\u{1b}[3;2He".into(), 3, 3);
        assert_eq!(cells[0][0].text, "a");
        assert_eq!(cells[0][0].fg.as_deref(), Some("#0102ff"));
        assert!(cells[0][1].bold);
        assert_eq!(cells[0][2], Cell { text: " ".into(), ..Cell::blank() });
        // `c` was written past the right edge and wrapped.
        assert_eq!(cells[1][0].text, "c");
        assert!(cells[2][0].reverse);
        assert_eq!(cells[2][1].text, "e");

        // With wrapping off the last column is overwritten instead.
        let clipped = grid("\u{1b}[?7labcd".into(), 1, 2);
        assert_eq!((cells_text(&clipped[0])).as_str(), "ad");
        // A line feed on the last row scrolls the first one away.
        let scrolled = grid("one\r\ntwo\r\n".into(), 2, 3);
        assert_eq!((cells_text(&scrolled[0]), cells_text(&scrolled[1])), ("two".into(), "   ".into()));
    }

    fn cells_text(row: &[Cell]) -> String {
        row.iter().map(|c| c.text.as_str()).collect()
    }

    /// A character in the look its role has is fine; one drawn plain, or in
    /// another role's colour, or off the grid, is reported.
    ///
    /// @tests REQ-LOOK.roles_drawn_in_theme_colours
    #[test]
    fn a_character_out_of_its_look_is_reported() {
        let looks = vec![Look { role: Role::Added, fg: Some("#00ff00".into()), bold: false }];
        let shown = buffer("+a\nb", vec![(0, 2, Role::Added)]);
        let right = grid("\u{1b}[38;2;0;255;0m+a\u{1b}[0m\nb".into(), 2, 4);
        assert!(miscoloured(shown.clone(), right, looks.clone()).is_empty());

        let plain = grid("+a\nb".into(), 2, 4);
        assert_eq!(miscoloured(shown.clone(), plain, looks.clone()).len(), 2);
        let leaked = grid("\u{1b}[38;2;0;255;0m+a\nb".into(), 2, 4);
        let found = miscoloured(shown.clone(), leaked, looks.clone());
        assert_eq!((found.len(), found[0].line, found[0].text.as_str()), (1, 1, "b"));
        let cut = grid("\u{1b}[38;2;0;255;0m+a".into(), 1, 1);
        assert!(miscoloured(shown, cut, looks).iter().any(|m| m.drawn.is_none()));
    }

    /// The shipped theme gives every coloured role a colour.
    #[test]
    fn the_shipped_theme_colours_the_roles() {
        let theme: serde_json::Value = serde_json::from_str(include_str!("../../../../assets/theme.json")).unwrap();
        for role in [Role::Path, Role::Heading, Role::Added, Role::Removed, Role::Requirement] {
            assert!(look_of(&theme, role).fg.is_some(), "{role:?}");
        }
        assert_eq!(look_of(&theme, Role::Plain), Look { role: Role::Plain, fg: None, bold: false });
    }
}
