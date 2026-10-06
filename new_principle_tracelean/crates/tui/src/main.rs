//! TraceLean's terminal frontend.
//!
//! It renders buffers the core produced and produces none of its own. A key
//! becomes an action through the keymap, an action becomes an intent through
//! `surface::act`, and an intent becomes a buffer through `surface::produce` —
//! so every behaviour here is one a web frontend reaches the same way.
//!
//! ```text
//! tracelean-tui [path]      # open a working tree
//! tracelean-tui --fresh [path]  # without what was open last time, keeping nothing
//! tracelean-tui --protocol  # answer with what it drew, for the conformance harness
//! tracelean-tui --paint     # paint the screen, for the capture harness
//! ```
//!
//! The two headless modes exist so this frontend is checked rather than
//! demonstrated, and they draw through the same `draw` module the interactive
//! loop uses — a frontend whose drawing and whose reporting are two code paths
//! passes the first check and fails the second.
//!
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

/// The keymap ships with the binary: a frontend that cannot find its keymap is
/// a frontend that cannot be used, and this one is data either way.
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

/// Headless: one buffer a line in, one answer a line out.
///
/// `painting` chooses between saying what was drawn and drawing it. Both go
/// through `draw`, which is the point.
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

/// A buffer with nothing in it, for the one case where nothing arrived. Asking
/// the core for it rather than writing one keeps `core_produces` true even
/// here.
fn empty() -> Buffer {
    tracelean_core::surface::produce::menu_buffer("empty".into(), Vec::new())
}

/// The region the panes are laid out in: the whole terminal, less the two bars
/// above it and the status line, blank line and row of affordances below.
fn region() -> Rect {
    let (columns, rows) = terminal::size().unwrap_or((80, 24));
    Rect {
        left: 0,
        top: 0,
        width: u64::from(columns).max(1),
        height: u64::from(rows).saturating_sub(6).max(1),
    }
}

/// How many rows the bars above the panes take: the stations and the strip.
const BARS: usize = 2;

/// A buffer joined into the one row a terminal has to spare for it.
///
/// The stations and the strip are lists, and a terminal showing each on its own
/// line would spend a third of the screen on chrome. Joining is this frontend's
/// business: the rows are the buffer's own and nothing is added between them
/// but spacing.
fn bar(buffer: &tracelean_core::surface::view::Buffer) -> String {
    draw::painted(buffer).join("  ")
}

/// Draw the editor: every pane in its place, then the status line under them.
///
/// The panes are composed into one grid and painted once. Each pane's buffer is
/// windowed to the room left after this frontend's own chrome — a column for a
/// divider, a row for the one below — so nothing is drawn over and nothing is
/// silently cut.
///
/// Every buffer here came from the core. The status line and the menu are not
/// special cases in the renderer; they are buffers like the rest.
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

    let mut out = String::from("\u{1b}[2J\u{1b}[H");
    // The two bars, above the panes. Buffers the core produced, drawn by the
    // one function that draws a buffer — the stations first because they are
    // always there, the opened set under them because it changes.
    out.push_str(&format!("\u{1b}[2m{}\u{1b}[0m\r\n", bar(&editor.stations())));
    out.push_str(&format!("{}\r\n", bar(&editor.strip())));
    // The editor's selection, as the cells of the focused pane it covers.
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
    let status = draw::painted(&editor.status);
    for line in &status {
        out.push_str("\u{1b}[7m");
        out.push_str(line);
        out.push_str("\u{1b}[0m\r\n");
    }
    // A question being typed into takes the last row, and the cursor with it.
    if let Some(asking) = asking {
        let row = BARS + height + 1 + status.len() + 1;
        let column = asking.split("   → ").next().unwrap_or(asking).chars().count() + 1;
        out.push_str(asking);
        out.push_str(&format!("\r\n\u{1b}[{row};{column}H"));
        return out;
    }
    // The last row is either the menu the mode offers or, when no mode is open,
    // what the cursor can do where it is. Both are buffers the core produced;
    // this joins one into a row because a terminal has one row to spare.
    let insert = if editor.mode == tracelean_editor::INSERT { "-- insert --  " } else { "" };
    // The checker's count, once counted; `Space t f` lists them.
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

    // The cursor last, so the terminal leaves it where a person is typing —
    // and inside the focused pane, because that is the pane the key goes to.
    let focused = editor.laid_out(region).into_iter().find(|placed| placed.focused);
    if let (Some(placed), Some((line, column))) = (focused, editor.cursor_in(height)) {
        // Two rows down for the bars above, and one more because a terminal
        // counts its rows from one.
        let row = BARS + placed.at.top as usize + line + 1;
        let column = placed.at.left as usize + column + 1;
        out.push_str(&format!("\u{1b}[{row};{column}H"));
    }
    out
}

/// A key press, as the keymap names keys.
///
/// Every name here comes from `keymap`, which is what makes this a translation
/// out of crossterm's vocabulary rather than a second naming of the keys. A
/// name invented here is a binding that does nothing and says nothing: this
/// function spelled the space bar `" "` while the keymap spelled it `Space`, so
/// the leader menu was unreachable from the terminal and every law about
/// `actions_reachable` still held — the keymap was reachable, the editor was
/// not.
///
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

/// A chord every editor has, as `Editor::chord` names it: Ctrl with a letter,
/// a function key, Home, End, Page Up and Down, Delete.
fn chord_of(key: KeyEvent) -> Option<String> {
    let shift = if key.modifiers.contains(KeyModifiers::SHIFT) { "S-" } else { "" };
    match key.code {
        // A terminal sends Ctrl+/ as Ctrl+_ (or Ctrl+7).
        KeyCode::Char('_' | '7') if key.modifiers.contains(KeyModifiers::CONTROL) => Some("C-/".into()),
        KeyCode::Up | KeyCode::Down if key.modifiers.contains(KeyModifiers::ALT) => {
            let way = if key.code == KeyCode::Up { "Up" } else { "Down" };
            Some(format!("A-{shift}{way}"))
        }
        KeyCode::BackTab => Some("S-Tab".into()),
        KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => Some("A-Left".into()),
        KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => Some("A-Right".into()),
        // Shift with a movement selects.
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

/// What the last row is asking for while a person types into it: text to
/// find (in this buffer, or with Tab in every file), a file to open, or text
/// to replace and then what to replace it with.
#[derive(Default)]
struct Asking {
    opening: bool,
    everywhere: bool,
    replacing: bool,
    /// What is replaced, once given; the row then asks for its replacement.
    wanted: Option<String>,
    text: String,
}

impl Asking {
    /// The row as it reads: the question, what is typed, and for a file what
    /// would open.
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

    /// Enter: do what was asked.
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
        // The editor is told how big the screen is rather than guessing: moving
        // the focus is a question about geometry, and the geometry is here.
        let region = region();
        editor.region = region;
        editor.follow_cursor(region.height as usize);
        let prompt = asking.as_ref().map(|a| a.row(&editor));
        let _ = write!(stdout, "{}", paint(&editor, region, prompt.as_deref()));
        let _ = stdout.flush();

        // Between keys, look again at the disk and the sandbox, as the window
        // does; the screen is drawn again only when something there changed.
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
        // While the last row asks, keys are typed into it.
        if let Some(open) = asking.as_mut() {
            match key.code {
                KeyCode::Esc => asking = None,
                // Replacing asks twice: what, then with what.
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
                // Ctrl+H, as the window has it, arrives as Backspace in most
                // terminals; Ctrl+R is the terminal's replace.
                "C-h" | "C-r" => asking = Some(Asking { replacing: true, ..Asking::default() }),
                other => editor.chord(other),
            }
            continue;
        }
        // Leaving is the frontend's own business: closing a window is not
        // something the editor does, and the desktop one has a title bar.
        if editor.mode == editor.keymap.root && key.code == KeyCode::Char('q') {
            editor.running = false;
            continue;
        }
        let Some(name) = name_of(key) else { continue };
        editor.key(&name);
    }

    editor.remember();
    let _ = execute!(stdout, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}
