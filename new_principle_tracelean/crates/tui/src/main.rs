//! @implements REQ-VIEW.frontend_is_checkable
//! @implements REQ-VIEW.screen_is_readable

mod draw;

use tracelean_editor as editor;

use std::io::{BufRead, Write};

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{execute, terminal};

use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::screen::{Rect, EXPLORER};
use tracelean_core::surface::view::Buffer;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--protocol") {
        return answer(false);
    }
    if args.iter().any(|a| a == "--paint") {
        return answer(true);
    }
    let root = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    interactive(root, args.iter().any(|a| a == "--fresh"));
}

fn answer(painting: bool) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let asked: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                let _ = writeln!(stdout, "{}", serde_json::json!({"case": 0, "error": error.to_string()}));
                let _ = stdout.flush();
                continue;
            }
        };
        let case = asked["case"].as_u64().unwrap_or(0);
        let parsed = serde_json::from_value::<Buffer>(asked["input"].clone());
        if painting {
            let screen = match &parsed {
                Ok(buffer) => draw::screen(buffer),
                Err(_) => draw::screen(&empty()),
            };
            let _ = write!(stdout, "{screen}");
            let _ = stdout.flush();
            continue;
        }
        let reply = match parsed {
            Ok(buffer) => serde_json::json!({"case": case, "output": draw::render(&buffer)}),
            Err(error) => serde_json::json!({"case": case, "error": error.to_string()}),
        };
        let _ = writeln!(stdout, "{reply}");
        let _ = stdout.flush();
    }
}

fn empty() -> Buffer {
    tracelean_core::surface::produce::menu_buffer("empty".into(), Vec::new())
}

fn region() -> Rect {
    let (columns, rows) = terminal::size().unwrap_or((80, 24));
    Rect {
        left: 0,
        top: 0,
        width: u64::from(columns).max(1),
        height: u64::from(rows).saturating_sub(6).max(1),
    }
}

const BARS: usize = 2;

fn bar(buffer: &tracelean_core::surface::view::Buffer) -> String {
    draw::painted(buffer).join("  ")
}

fn paint(editor: &editor::Editor, region: Rect, asking: Option<&str>) -> String {
    let height = region.height as usize;
    let panes: Vec<(Rect, tracelean_core::surface::view::Buffer, bool)> = editor
        .laid_out(region)
        .into_iter()
        .map(|placed| {
            let (_, spare_below) = draw::chrome(region, placed.at);
            let rows = placed.at.height.saturating_sub(spare_below) as usize;
            let window =
                tracelean_core::surface::produce::window(placed.buffer, placed.top, rows);
            (placed.at, window, placed.focused)
        })
        .collect();

    let mut out = String::from("\u{1b}[2J\u{1b}[?7l\u{1b}[H");
    let stations = tracelean_core::surface::produce::menu_row(
        "stations".into(),
        tracelean_core::surface::screen::station_entries(),
        region.width,
    );
    out.push_str(&format!("{}\u{1b}[0m\r\n", bar(&stations)));
    out.push_str(&format!("{}\r\n", bar(&editor.strip())));
    let mut selected = std::collections::BTreeSet::new();
    if let (Some((from, to)), Some((at, window, _))) = (editor.selection_shown(), panes.iter().find(|p| p.2)) {
        let (mut row, mut column) = (0, 0);
        for (offset, character) in window.text.chars().enumerate().take(to) {
            if character == '\n' {
                (row, column) = (row + 1, 0);
                continue;
            }
            if offset >= from {
                selected.insert((at.top as usize + row, at.left as usize + column));
            }
            column += 1;
        }
    }
    let grid = draw::compose(region, &panes);
    for line in draw::paint_marked(&grid, |y, x| selected.contains(&(y, x))) {
        out.push_str(&line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    let status = draw::painted_with(&editor.status, "\u{1b}[7m");
    for line in &status {
        out.push_str(line);
        out.push_str("\u{1b}[0m\r\n");
    }
    if let Some(asking) = asking {
        let row = BARS + height + 1 + status.len() + 1;
        let column = asking.split("   → ").next().unwrap_or(asking).chars().count() + 1;
        out.push_str(asking);
        out.push_str(&format!("\r\n\u{1b}[{row};{column}H"));
        return out;
    }
    let insert = if editor.mode == tracelean_editor::INSERT { "-- insert --  " } else { "" };
    let count = match editor.problems() {
        Some(0) | None => String::new(),
        Some(n) => format!("⚠ {n}  "),
    };
    let mode = format!("{count}{insert}");
    let bar = match &editor.menu {
        Some(menu) => draw::painted(menu).join("  "),
        None => editor.offered().join("  "),
    };
    out.push_str(&format!("\u{1b}[2m{mode}\u{1b}[0m{bar}\r\n"));

    let focused = editor.laid_out(region).into_iter().find(|placed| placed.focused);
    if let (Some(placed), Some((line, column))) = (focused, editor.cursor_in(height)) {
        let row = BARS + placed.at.top as usize + line + 1;
        let column = placed.at.left as usize + column + 1;
        out.push_str(&format!("\u{1b}[{row};{column}H"));
    }
    out
}

/// @implements REQ-MYTH.actions_reachable
fn name_of(key: KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Char(c) => Some(keymap::typed(c)),
        KeyCode::Esc => Some(keymap::LEAVE.to_string()),
        KeyCode::Enter => Some("Enter".to_string()),
        KeyCode::Backspace => Some("Backspace".to_string()),
        KeyCode::Tab => Some("Tab".to_string()),
        KeyCode::Up => Some("Up".to_string()),
        KeyCode::Down => Some("Down".to_string()),
        KeyCode::Left => Some("Left".to_string()),
        KeyCode::Right => Some("Right".to_string()),
        _ => None,
    }
}

fn chord_of(key: KeyEvent) -> Option<String> {
    let shift = if key.modifiers.contains(KeyModifiers::SHIFT) { "S-" } else { "" };
    match key.code {
        KeyCode::Char('_' | '7') if key.modifiers.contains(KeyModifiers::CONTROL) => Some("C-/".into()),
        KeyCode::Up | KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
            let way = if key.code == KeyCode::Up { "Up" } else { "Down" };
            Some(format!("A-{shift}{way}"))
        }
        KeyCode::BackTab => Some("S-Tab".into()),
        KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => Some("A-Left".into()),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => Some("A-Right".into()),
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
        | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown
            if key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            let ctrl = if key.modifiers.contains(KeyModifiers::CONTROL) { "C-" } else { "" };
            Some(format!("S-{ctrl}{:?}", key.code))
        }
        KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => Some("C-Left".into()),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => Some("C-Right".into()),
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => Some("C-Backspace".into()),
        KeyCode::Delete if key.modifiers.contains(KeyModifiers::CONTROL) => Some("C-Delete".into()),
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let shift = if c.is_uppercase() { "S-" } else { shift };
            Some(format!("C-{shift}{}", c.to_ascii_lowercase()))
        }
        KeyCode::F(n) => Some(format!("{shift}F{n}")),
        KeyCode::Home => Some("Home".into()),
        KeyCode::End => Some("End".into()),
        KeyCode::PageUp => Some("PageUp".into()),
        KeyCode::PageDown => Some("PageDown".into()),
        KeyCode::Delete => Some("Delete".into()),
        _ => None,
    }
}

#[derive(Default)]
struct Asking {
    opening: bool,
    everywhere: bool,
    replacing: bool,
    wanted: Option<String>,
    text: String,
}

impl Asking {
    fn row(&self, editor: &editor::Editor) -> String {
        if self.opening {
            let found = editor.quick_open(&self.text);
            let best: Vec<&str> = found.iter().take(5).map(String::as_str).collect();
            return format!("open: {}   → {}", self.text, best.join("  "));
        }
        if self.replacing {
            return match &self.wanted {
                Some(wanted) => format!("replace every `{wanted}` with: {}", self.text),
                None => format!("replace: {}", self.text),
            };
        }
        let place = if self.everywhere { "every file" } else { "this buffer" };
        format!("find in {place} (Tab switches): {}", self.text)
    }

    fn answer(self, editor: &mut editor::Editor) {
        if self.opening {
            match editor.quick_open(&self.text).into_iter().next() {
                Some(path) => editor.choose(EXPLORER, 0, "file.open", Some(path), None),
                None => editor.say("no file matches", &self.text),
            }
        } else if self.replacing {
            if let Some(wanted) = &self.wanted {
                editor.replace_all(wanted, &self.text);
            }
        } else if self.everywhere {
            editor.list_occurrences(&self.text, false);
        } else {
            editor.find(&self.text, true);
        }
    }
}

fn interactive(root: std::path::PathBuf, fresh: bool) {
    draw::use_theme(editor::theme::load(&root));
    let mut editor = if fresh {
        editor::Editor::open_fresh(root, keymap())
    } else {
        let mut editor = editor::Editor::open(root, keymap());
        editor.track_recent();
        editor
    };
    let mut stdout = std::io::stdout();
    let _ = terminal::enable_raw_mode();
    let _ = execute!(stdout, terminal::EnterAlternateScreen);
    let mut asking: Option<Asking> = None;

    while editor.running {
        let region = region();
        editor.region = region;
        editor.follow_cursor(region.height as usize);
        let prompt = asking.as_ref().map(|a| a.row(&editor));
        let _ = write!(stdout, "{}", paint(&editor, region, prompt.as_deref()));
        let _ = stdout.flush();

        let event = loop {
            if crossterm::event::poll(std::time::Duration::from_millis(1500)).unwrap_or(true) {
                break crossterm::event::read();
            }
            if editor.tick() {
                break Ok(TermEvent::FocusGained);
            }
        };
        let Ok(TermEvent::Key(key)) = event else { continue };
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            break;
        }
        if let Some(open) = asking.as_mut() {
            match key.code {
                KeyCode::Esc => asking = None,
                KeyCode::Enter if open.replacing && open.wanted.is_none() => {
                    open.wanted = Some(std::mem::take(&mut open.text));
                }
                KeyCode::Enter => asking.take().into_iter().for_each(|a| a.answer(&mut editor)),
                KeyCode::Backspace => drop(open.text.pop()),
                KeyCode::Tab => open.everywhere = !open.everywhere,
                KeyCode::Char(c) => open.text.push(c),
                _ => {}
            }
            continue;
        }
        if let Some(chord) = chord_of(key) {
            match chord.as_str() {
                "C-f" | "C-S-f" => {
                    let everywhere = chord == "C-S-f";
                    asking = Some(Asking { everywhere, ..Asking::default() });
                }
                "C-p" => asking = Some(Asking { opening: true, ..Asking::default() }),
                "C-h" | "C-r" => asking = Some(Asking { replacing: true, ..Asking::default() }),
                other => editor.chord(other),
            }
            continue;
        }
        if editor.mode == editor.keymap.root && key.code == KeyCode::Char('q') {
            editor.running = false;
            continue;
        }
        let Some(name) = name_of(key) else { continue };
        editor.key(&name);
    }

    editor.remember();
    let _ = execute!(stdout, terminal::EnableLineWrap, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-MYTH.actions_reachable
    #[test]
    fn every_key_is_named_as_the_keymap_names_it() {
        let named = |code| name_of(KeyEvent::new(code, KeyModifiers::NONE));
        let codes = [
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Backspace,
            KeyCode::Tab,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char(' '),
        ];
        for code in codes {
            let name = named(code).expect("a key the keymap names");
            assert!(keymap::NAMED.contains(&name.as_str()), "`{name}` is no name the keymap uses");
        }
        assert_eq!(named(KeyCode::Char('a')).as_deref(), Some("a"));
        assert_eq!(named(KeyCode::F(5)), None);
    }
}
