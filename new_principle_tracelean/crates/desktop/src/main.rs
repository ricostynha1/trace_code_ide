//! TraceLean's desktop window.
//!
//! The second frontend, and deliberately the thinner one. It owns a window and
//! a page; everything the editor does is `tracelean_editor`, the same crate the
//! terminal frontend drives. A key pressed here and the same key pressed in the
//! terminal reach one keymap, one dispatch and one producer.
//!
//! So this file has three commands and no decisions:
//!
//! - `shown`  — every pane, where it goes, and the part of its buffer that fits
//! - `cursor` — where the cursor is in the focused pane
//! - `menu`   — the menu the mode offers, while it offers one
//! - `press`  — a key, interpreted by the editor
//! - `act`    — an action, dispatched from a position in a pane's buffer
//! - `place`  — a pointer pressed at a position: focus and cursor
//! - `offers` — everything that can be done at a position (the context menu)
//! - `choose` — one of those, with its target and the answer typed for it
//! - `chord`  — save, undo, redo, Home, End, PageUp, PageDown, Delete
//! - `scroll` — a wheel over a pane
//! - `paste`  — text from the clipboard
//! - `cut`    — a selection deleted, to type over it or as Ctrl+X
//! - `quick`  — the files matching a few letters, for Ctrl+P
//! - `search` — every line of the project holding some text, Ctrl+Shift+F
//! - `find`   — the next place the focused buffer holds some text
//! - `described` — each action's description and keys, for hover text
//! - `grab`   — a pointer focusing a pane or dragging its divider
//! - `bars`   — the stations and what is opened, as buffers
//! - `act_in_bar` — an action from a row of one of those
//! - `theme`  — the colours, read from `assets/theme.json` and the project's
//!   `.tracelean/theme.json`
//!
//! The page cannot ask for anything else, because there is nothing else to ask.
//!
//! It reaches them through the bridge Tauri injects as `__TAURI__`, which it
//! injects only when `withGlobalTauri` is set — and this page is loaded by a
//! browser with no bundler between the source that was checked and the file
//! that ships (`web/build.mjs`), so the global is the only bridge there is.
//! Without it the window opens on a page that can ask for no buffer and
//! therefore draws none, which is not a rendering fault: it is this frontend
//! never being connected. `tauri.conf.json` asks, and
//! `crates/core/tests/one_representation.rs` checks that it still does.
//!
//! @implements REQ-VIEW.one_representation
//! @implements REQ-ACT.one_path

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tauri::{Manager, State};
use tracelean_core::surface::act::Intent;
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::produce::window;
use tracelean_core::surface::screen::{Arrangement, Rect};
use tracelean_core::surface::view::Buffer;
use tracelean_editor::Editor;

/// The keymap ships with the binary, as it does in the terminal frontend: one
/// keymap, so one set of keys.
const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

struct Held(Mutex<Editor>);

/// One pane as the page needs it: where it goes, and the part of its buffer
/// that fits there.
///
/// A flat record rather than the editor's own `Placed`, because the page reads
/// JSON and the windowing has already happened by the time it gets here.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Pane {
    pane: String,
    at: Rect,
    buffer: Buffer,
    focused: bool,
    /// The buffer line the window starts at, so a gutter can number lines.
    top: usize,
    /// The claims marked beside the window's lines (`surface::chips`): the
    /// buffer line, the role's letter, the requirement it opens.
    chips: Vec<(usize, char, String)>,
}

/// Every pane the page should draw.
///
/// The page says how big it is in characters, because only the page knows what
/// its font does — and each pane's buffer is windowed to its own height, so the
/// page is still drawing everything it was given.
///
/// A window spends nothing on dividers: it draws a border between panes with
/// CSS, in the space between the boxes, where a terminal has to take a column
/// from the pane itself. So the height asked for here is the pane's own.
#[tauri::command]
fn shown(held: State<'_, Held>, width: u64, height: u64) -> Vec<Pane> {
    let mut editor = held.0.lock().expect("the editor");
    let region = Rect { left: 0, top: 0, width: width.max(1), height: height.max(1) };
    editor.region = region;
    editor.follow_cursor(region.height as usize);
    editor
        .laid_out(region)
        .into_iter()
        .map(|placed| Pane {
            chips: editor.chips_shown(&placed.buffer, placed.top, placed.at.height as usize),
            buffer: window(placed.buffer, placed.top, placed.at.height as usize),
            pane: placed.pane,
            at: placed.at,
            focused: placed.focused,
            top: placed.top,
        })
        .collect()
}

/// Where the cursor is in that window, so the page can show it.
#[tauri::command]
fn cursor(held: State<'_, Held>, height: usize) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").cursor_in(height)
}

/// The two bars above the panes: the stations, then what is opened.
///
/// Buffers, so the page draws them with the same code it draws a file with, and
/// their rows carry the actions a key dispatches — which is how a window offers
/// what a terminal binds to a key.
#[tauri::command]
fn bars(held: State<'_, Held>) -> (Buffer, Buffer) {
    let editor = held.0.lock().expect("the editor");
    (editor.stations(), editor.strip())
}

/// An action from a row of one of those bars.
///
/// The bars are not the focused pane, so the focus an action is resolved
/// against has to be built from the bar's own buffer. `act` resolves against
/// the pane, and using it here would open whatever the cursor happened to be
/// on in a different buffer entirely — a click that did something, just not the
/// thing clicked.
#[tauri::command]
fn act_in_bar(held: State<'_, Held>, station: bool, action: String, offset: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.act_in_bar(station, &action, offset);
    said(&editor)
}

/// The menu the mode offers, while it offers one.
///
/// A buffer like everything else, so the page draws it with the same code it
/// draws the file with — and its rows carry actions, which is how a window
/// offers what a terminal binds to a key.
#[tauri::command]
fn menu(held: State<'_, Held>) -> Option<Buffer> {
    held.0.lock().expect("the editor").menu.clone()
}

/// A key, interpreted exactly as the terminal interprets it — by the same
/// function, which is why the two frontends cannot drift.
#[tauri::command]
fn press(held: State<'_, Held>, key: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.key(&key);
    said(&editor)
}

/// An action, from the position in the buffer the person clicked.
///
/// The offset is what makes this the same call the terminal makes: the focus is
/// built from the buffer and the position, so `file.open` opens what was
/// clicked for the same reason it opens what the cursor was on.
///
/// The pane comes with it. An action resolved against whichever pane had the
/// focus would open what the cursor was on somewhere else — and the offset is
/// in the window the page was given, which the editor turns into a position in
/// the whole buffer.
#[tauri::command]
fn act(held: State<'_, Held>, pane: String, action: String, offset: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.choose(&pane, offset, &action, None, None);
    said(&editor)
}

/// Look at the sandbox again: what the status line says when anything a
/// person would see changed, so the page polls cheaply and redraws only when
/// there is something new; nothing otherwise.
#[tauri::command]
fn tick(held: State<'_, Held>) -> Option<String> {
    let mut editor = held.0.lock().expect("the editor");
    editor.tick().then(|| said(&editor))
}

/// Text the editor wants on the clipboard, taken once. Writing the clipboard
/// is the page's job; deciding what goes there is the editor's.
#[tauri::command]
fn take_clipboard(held: State<'_, Held>) -> Option<String> {
    held.0.lock().expect("the editor").clipboard.take()
}

/// An action performed where the cursor already is — a row of the which-key
/// bar, which is about whatever the cursor was on when the mode was entered.
#[tauri::command]
fn act_here(held: State<'_, Held>, action: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.act_here(&action);
    said(&editor)
}

/// A pointer pressed at a position: focus that pane and put the cursor there.
#[tauri::command]
fn place(held: State<'_, Held>, pane: String, offset: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.place(&pane, offset);
    said(&editor)
}

/// Everything that can be done at a position — what a right-click shows.
/// Computed by the core, so the menu offers nothing the dispatcher would not
/// know (`REQ-ACT.everything_is_offered`).
#[tauri::command]
fn offers(
    held: State<'_, Held>,
    pane: String,
    offset: usize,
) -> Vec<tracelean_core::surface::offer::Offer> {
    held.0.lock().expect("the editor").offers_at(&pane, offset)
}

/// One entry of the context menu, with the answer typed for it if it asked.
#[tauri::command]
fn choose(
    held: State<'_, Held>,
    pane: String,
    offset: usize,
    action: String,
    target: Option<String>,
    answer: Option<String>,
) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.choose(&pane, offset, &action, target, answer);
    said(&editor)
}

/// The keys every editor has: Ctrl+S, Ctrl+Z, Ctrl+Y, Home, End, Page keys.
#[tauri::command]
fn chord(held: State<'_, Held>, chord: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.chord(&chord);
    said(&editor)
}

/// Look for text in the focused buffer, forwards or back from the cursor.
#[tauri::command]
fn find(held: State<'_, Held>, text: String, forward: bool) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.find(&text, forward);
    said(&editor)
}

/// Replace every match in the focused file, as one change.
#[tauri::command]
fn replace(held: State<'_, Held>, text: String, with: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.replace_all(&text, &with);
    said(&editor)
}

/// What each action is called and the keys that reach it, for hover text.
#[tauri::command]
fn described(held: State<'_, Held>) -> std::collections::BTreeMap<String, String> {
    tracelean_core::surface::offer::described(&held.0.lock().expect("the editor").keymap)
}

/// A wheel over a pane.
#[tauri::command]
fn scroll(held: State<'_, Held>, pane: String, lines: i64) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.scroll(&pane, lines);
    said(&editor)
}

/// A pointer's selection deleted: before typing over it, or as a cut.
#[tauri::command]
fn cut(held: State<'_, Held>, pane: String, start: usize, end: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.cut(&pane, start, end);
    said(&editor)
}

/// A drag with the pointer in a file: the editor's selection from now on.
#[tauri::command]
fn select(held: State<'_, Held>, pane: String, start: usize, end: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.select(&pane, start, end);
    said(&editor)
}

/// What is selected, as offsets into the focused window, to mark.
#[tauri::command]
fn selection(held: State<'_, Held>) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").selection_shown()
}

/// Where the name under the cursor is written in the focused window, to mark.
#[tauri::command]
fn occurrences(held: State<'_, Held>) -> Option<(usize, Vec<usize>)> {
    held.0.lock().expect("the editor").occurrences_shown()
}

/// The text selected, to start a find with.
#[tauri::command]
fn selected(held: State<'_, Held>) -> Option<String> {
    held.0.lock().expect("the editor").selected()
}

/// The mode keys are read in — `Insert`, or the keymap's own mode names.
#[tauri::command]
fn mode(held: State<'_, Held>) -> String {
    held.0.lock().expect("the editor").mode.clone()
}

/// What the file being edited declares, by a few letters of its name.
#[tauri::command]
fn symbols(held: State<'_, Held>, query: String) -> Vec<(String, String)> {
    held.0.lock().expect("the editor").symbols(&query)
}

/// Every action whose name or keys hold what was typed: the palette.
#[tauri::command]
fn palette(held: State<'_, Held>, query: String) -> Vec<(String, String)> {
    held.0.lock().expect("the editor").palette(&query)
}

/// How many findings the checker has on the tree, once counted.
#[tauri::command]
fn problems(held: State<'_, Held>) -> Option<usize> {
    held.0.lock().expect("the editor").problems()
}

/// The bracket beside the cursor and its match, in the focused window.
#[tauri::command]
fn brackets(held: State<'_, Held>) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").bracket_pair()
}

/// Tab, Shift+Tab or Ctrl+/ over a selection: every line it touches.
#[tauri::command]
fn over_lines(held: State<'_, Held>, pane: String, start: usize, end: usize, chord: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.over_lines(&pane, start, end, &chord);
    said(&editor)
}

/// What a requirement name says, for hover text.
#[tauri::command]
fn requirement_text(held: State<'_, Held>, name: String) -> Option<String> {
    held.0.lock().expect("the editor").requirement_text(&name)
}

/// Ctrl+P: the files whose paths best match what was typed.
#[tauri::command]
fn quick(held: State<'_, Held>, query: String) -> Vec<String> {
    held.0.lock().expect("the editor").quick_open(&query)
}

/// Ctrl+Shift+F: every line of the project holding some text.
#[tauri::command]
fn search(held: State<'_, Held>, text: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.list_occurrences(&text, false);
    said(&editor)
}

/// Text from the clipboard.
#[tauri::command]
fn paste(held: State<'_, Held>, text: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.paste(&text);
    said(&editor)
}

/// A pointer, arranging: clicking inside a pane, or dragging the divider on its
/// edge.
///
/// The page measured a distance in pixels and turned it into a number of
/// characters before calling; from here on it is the same change a key makes,
/// through the same function (`REQ-SCREEN.one_arrangement_path`). A window that
/// worked out the new weights itself would be a second answer to what a resize
/// does, and the only one no test ever ran.
///
/// Grabbing a divider focuses the pane it belongs to, because the divider is
/// that pane's edge and `resize` moves the one beside the focus.
#[tauri::command]
fn grab(held: State<'_, Held>, pane: String, amount: i64) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.perform(Intent::Arrange { how: Arrangement::FocusPane { pane } });
    if amount != 0 {
        editor.perform(Intent::Arrange { how: Arrangement::Resize { amount } });
    }
    said(&editor)
}

/// How things look: the shipped theme with the project's override over it.
///
/// Read again on every call, so a colour changed in the file shows the next
/// time the window asks — which it does whenever it regains focus.
#[tauri::command]
fn theme(held: State<'_, Held>) -> serde_json::Value {
    let root = held.0.lock().expect("the editor").root.clone();
    tracelean_editor::theme::load(&root)
}

/// The status line's text. It is a buffer like everything else, so this reads
/// it rather than composing a message.
fn said(editor: &Editor) -> String {
    tracelean_core::surface::view::plain_text(editor.status.clone()).join("\n")
}

fn main() {
    let root = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    tauri::Builder::default()
        .setup(move |app| {
            let mut editor = Editor::open(root.clone(), keymap());
            editor.track_recent();
            app.manage(Held(Mutex::new(editor)));
            Ok(())
        })
        // Closing with unsaved work is refused once, and the status line says
        // why; closing again closes.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let held = window.state::<Held>();
                let mut editor = held.0.lock().expect("the editor");
                if !editor.may_close() {
                    api.prevent_close();
                    let text = serde_json::to_string(&said(&editor)).unwrap_or_default();
                    if let Some(view) = window.app_handle().get_webview_window(window.label()) {
                        let _ = view.eval(&format!("document.getElementById('status').textContent = {text}"));
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            shown, cursor, menu, press, act, grab, bars, act_in_bar, theme, place, offers, choose,
            chord, scroll, paste, act_here, tick, take_clipboard, find, described, cut, quick, search,
            requirement_text, over_lines, brackets, mode, select, selection, replace,
            problems, palette, selected, symbols, occurrences
        ])
        .run(tauri::generate_context!())
        .expect("the window opens");
}
