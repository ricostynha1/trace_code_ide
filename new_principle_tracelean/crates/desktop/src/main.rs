//! @implements REQ-VIEW.one_representation
//! @implements REQ-ACT.one_path

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tauri::{Manager, State};
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::produce::window;
use tracelean_core::surface::screen::Rect;
use tracelean_core::surface::view::Buffer;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

struct Held(Mutex<Editor>);

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Pane {
    pane: String,
    at: Rect,
    buffer: Buffer,
    focused: bool,
    top: usize,
    lines: usize,
    chips: Vec<(usize, char, String)>,
    coverage: Vec<(usize, u64, String)>,
}

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
            lines: placed.buffer.text.split('\n').count(),
            chips: editor.chips_shown(&placed.buffer, placed.top, placed.at.height as usize),
            coverage: editor.coverage_shown(&placed.buffer, placed.top, placed.at.height as usize),
            buffer: window(placed.buffer, placed.top, placed.at.height as usize),
            pane: placed.pane,
            at: placed.at,
            focused: placed.focused,
            top: placed.top,
        })
        .collect()
}

#[tauri::command]
fn cursor(held: State<'_, Held>, height: usize) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").cursor_in(height)
}

#[tauri::command]
fn bars(held: State<'_, Held>) -> (Buffer, Buffer) {
    let editor = held.0.lock().expect("the editor");
    (editor.stations(), editor.strip())
}

#[tauri::command]
fn act_in_bar(held: State<'_, Held>, station: bool, action: String, offset: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.act_in_bar(station, &action, offset);
    said(&editor)
}

#[tauri::command]
fn menu(held: State<'_, Held>) -> Option<Buffer> {
    held.0.lock().expect("the editor").menu.clone()
}

#[tauri::command]
fn press(held: State<'_, Held>, key: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.key(&key);
    said(&editor)
}

#[tauri::command]
fn act(held: State<'_, Held>, pane: String, action: String, offset: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.choose(&pane, offset, &action, None, None);
    said(&editor)
}

#[tauri::command]
fn tick(held: State<'_, Held>) -> Option<String> {
    let mut editor = held.0.lock().expect("the editor");
    editor.tick().then(|| said(&editor))
}

#[tauri::command]
fn take_clipboard(held: State<'_, Held>) -> Option<String> {
    held.0.lock().expect("the editor").clipboard.take()
}

#[tauri::command]
fn act_here(held: State<'_, Held>, action: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.act_here(&action);
    said(&editor)
}

#[tauri::command]
fn place(held: State<'_, Held>, pane: String, offset: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.place(&pane, offset);
    said(&editor)
}

#[tauri::command]
fn offers(
    held: State<'_, Held>,
    pane: String,
    offset: usize,
) -> Vec<tracelean_core::surface::offer::Offer> {
    held.0.lock().expect("the editor").offers_at(&pane, offset)
}

#[tauri::command]
fn offers_here(held: State<'_, Held>) -> (String, usize, Vec<tracelean_core::surface::offer::Offer>) {
    held.0.lock().expect("the editor").offers_here()
}

#[tauri::command]
fn history_preview(held: State<'_, Held>, pane: String, offset: usize) -> Option<Buffer> {
    held.0.lock().expect("the editor").history_preview_at(&pane, offset)
}

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

#[tauri::command]
fn chord(held: State<'_, Held>, chord: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.chord(&chord);
    said(&editor)
}

#[tauri::command]
fn find(held: State<'_, Held>, text: String, forward: bool) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.find(&text, forward);
    said(&editor)
}

#[tauri::command]
fn replace(held: State<'_, Held>, text: String, with: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.replace_all(&text, &with);
    said(&editor)
}

#[tauri::command]
fn described(held: State<'_, Held>) -> std::collections::BTreeMap<String, String> {
    tracelean_core::surface::offer::described(&held.0.lock().expect("the editor").keymap)
}

#[tauri::command]
fn scroll(held: State<'_, Held>, pane: String, lines: i64) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.scroll(&pane, lines);
    said(&editor)
}

#[tauri::command]
fn cut(held: State<'_, Held>, pane: String, start: usize, end: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.cut(&pane, start, end);
    said(&editor)
}

#[tauri::command]
fn select(held: State<'_, Held>, pane: String, start: usize, end: usize) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.select(&pane, start, end);
    said(&editor)
}

#[tauri::command]
fn selection(held: State<'_, Held>) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").selection_shown()
}

#[tauri::command]
fn occurrences(held: State<'_, Held>) -> Option<(usize, Vec<usize>)> {
    held.0.lock().expect("the editor").occurrences_shown()
}

#[tauri::command]
fn selected(held: State<'_, Held>) -> Option<String> {
    held.0.lock().expect("the editor").selected()
}

#[tauri::command]
fn mode(held: State<'_, Held>) -> String {
    held.0.lock().expect("the editor").mode.clone()
}

#[tauri::command]
fn symbols(held: State<'_, Held>, query: String) -> Vec<(String, String)> {
    held.0.lock().expect("the editor").symbols(&query)
}

#[tauri::command]
fn palette(held: State<'_, Held>, query: String) -> Vec<(String, String)> {
    held.0.lock().expect("the editor").palette(&query)
}

#[tauri::command]
fn problems(held: State<'_, Held>) -> Option<usize> {
    held.0.lock().expect("the editor").problems()
}

#[tauri::command]
fn brackets(held: State<'_, Held>) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").bracket_pair()
}

#[tauri::command]
fn over_lines(held: State<'_, Held>, pane: String, start: usize, end: usize, chord: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.over_lines(&pane, start, end, &chord);
    said(&editor)
}

#[tauri::command]
fn requirement_text(held: State<'_, Held>, name: String) -> Option<String> {
    held.0.lock().expect("the editor").requirement_text(&name)
}

#[tauri::command]
fn quick(held: State<'_, Held>, query: String) -> Vec<String> {
    held.0.lock().expect("the editor").quick_open(&query)
}

#[tauri::command]
fn search(held: State<'_, Held>, text: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.list_occurrences(&text, false);
    said(&editor)
}

#[tauri::command]
fn paste(held: State<'_, Held>, text: String) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.paste(&text);
    said(&editor)
}

#[tauri::command]
fn grab(held: State<'_, Held>, pane: String, amount: i64) -> String {
    let mut editor = held.0.lock().expect("the editor");
    editor.grab(&pane, amount);
    said(&editor)
}

#[tauri::command]
fn theme(held: State<'_, Held>) -> serde_json::Value {
    let root = held.0.lock().expect("the editor").root.clone();
    tracelean_editor::theme::load(&root)
}

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
            problems, palette, selected, symbols, occurrences, offers_here, history_preview
        ])
        .run(tauri::generate_context!())
        .expect("the window opens");
}
