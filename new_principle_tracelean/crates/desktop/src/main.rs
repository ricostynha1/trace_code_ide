//! @implements REQ-VIEW.one_representation
//! @implements REQ-ACT.one_path

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use tauri::{Manager, State};
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::offer::Offer;
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
    /// A markdown file's whole text, for the formatted preview.
    markdown: Option<String>,
    /// The agent's conversation, for the sandbox panel (the buffer then holds all lines).
    said: Option<Vec<tracelean_core::observe::transcript::Event>>,
}

fn is_markdown(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
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
        .map(|placed| {
            use tracelean_core::surface::view::BufferKind;
            let markdown = match &placed.buffer.kind {
                BufferKind::File { path } if is_markdown(path) => {
                    Some(editor.workspace().files.get(path).cloned().unwrap_or_default())
                }
                _ => None,
            };
            let sandbox = placed.buffer.id == "record:sandbox";
            let said = sandbox.then(|| editor.conversation()).filter(|said| !said.is_empty());
            Pane {
            markdown,
            lines: placed.buffer.text.split('\n').count(),
            chips: editor.chips_shown(&placed.buffer, placed.top, placed.at.height as usize),
            coverage: editor.coverage_shown(&placed.buffer, placed.top, placed.at.height as usize),
            buffer: if said.is_some() { placed.buffer } else { window(placed.buffer, placed.top, placed.at.height as usize) },
            said,
            pane: placed.pane,
            at: placed.at,
            focused: placed.focused,
            top: placed.top,
        }})
        .collect()
}

#[tauri::command]
fn cursor(held: State<'_, Held>, height: usize) -> Option<(usize, usize)> {
    held.0.lock().expect("the editor").cursor_in(height)
}

macro_rules! mutating {
    ($($name:ident($($arg:ident: $ty:ty),*) => |$e:ident| $body:expr;)*) => {$(
        #[tauri::command]
        fn $name(held: State<'_, Held>, $($arg: $ty),*) -> String {
            let mut $e = held.0.lock().expect("the editor");
            $body;
            $e.hand_off(tracelean_core::launch::start, tracelean_core::launch::ask);
            said(&$e)
        }
    )*};
}

macro_rules! query {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $out:ty => |$e:ident| $body:expr;)*) => {$(
        #[tauri::command]
        fn $name(held: State<'_, Held>, $($arg: $ty),*) -> $out {
            let $e = held.0.lock().expect("the editor");
            $body
        }
    )*};
}

mutating! {
    act_in_bar(station: bool, action: String, offset: usize) => |e| e.act_in_bar(station, &action, offset);
    press(key: String) => |e| e.key(&key);
    act(pane: String, action: String, offset: usize) => |e| e.choose(&pane, offset, &action, None, None);
    act_here(action: String) => |e| e.act_here(&action);
    place(pane: String, offset: usize) => |e| e.place(&pane, offset);
    choose(pane: String, offset: usize, action: String, target: Option<String>, answer: Option<String>) => |e| e.choose(&pane, offset, &action, target, answer);
    chord(chord: String) => |e| e.chord(&chord);
    find(text: String, forward: bool) => |e| e.find(&text, forward);
    replace(text: String, with: String) => |e| e.replace_all(&text, &with);
    scroll(pane: String, lines: i64) => |e| e.scroll(&pane, lines);
    cut(pane: String, start: usize, end: usize) => |e| e.cut(&pane, start, end);
    select(pane: String, start: usize, end: usize) => |e| e.select(&pane, start, end);
    over_lines(pane: String, start: usize, end: usize, chord: String) => |e| e.over_lines(&pane, start, end, &chord);
    search(text: String) => |e| e.list_occurrences(&text, false);
    paste(text: String) => |e| e.paste(&text);
    grab(pane: String, amount: i64) => |e| e.grab(&pane, amount);
    zoom(steps: i64) => |e| e.zoom(steps);
}

query! {
    bars() -> (Buffer, Buffer) => |e| (e.stations(), e.strip());
    menu() -> Option<Buffer> => |e| e.menu.clone();
    offers(pane: String, offset: usize) -> Vec<Offer> => |e| e.offers_at(&pane, offset);
    offers_here() -> (String, usize, Vec<Offer>) => |e| e.offers_here();
    history_preview(pane: String, offset: usize) -> Option<Buffer> => |e| e.history_preview_at(&pane, offset);
    described() -> std::collections::BTreeMap<String, String> => |e| tracelean_core::surface::offer::described(&e.keymap);
    selection() -> Option<(usize, usize)> => |e| e.selection_shown();
    occurrences() -> Option<(usize, Vec<usize>)> => |e| e.occurrences_shown();
    selected() -> Option<String> => |e| e.selected();
    mode() -> String => |e| e.mode.clone();
    symbols(query: String) -> Vec<(String, String)> => |e| e.symbols(&query);
    palette(query: String) -> Vec<(String, String)> => |e| e.palette(&query);
    problems() -> Option<usize> => |e| e.problems();
    brackets() -> Option<(usize, usize)> => |e| e.bracket_pair();
    requirement_text(name: String) -> Option<String> => |e| e.requirement_text(&name);
    quick(query: String) -> Vec<String> => |e| e.quick_open(&query);
    overview() -> (Vec<tracelean_core::surface::produce::Node>, Option<usize>) => |e| e.overview();
    card(name: String) -> Option<tracelean_editor::Card> => |e| e.card(&name);
    impact() -> Vec<tracelean_editor::Impact> => |e| e.impact();
    runs() -> Vec<tracelean_editor::Run> => |e| e.runs();
    modules() -> Vec<tracelean_core::surface::figures::Module> => |e| e.modules();
    agent_steps() -> Vec<tracelean_core::surface::figures::Step> => |e| e.agent_steps();
    overview_markdown() -> String => |e| e.overview_markdown();
    leader() -> Vec<(String, Vec<(String, String, String)>)> => |e| e.leader();
    tree_marks() -> Vec<(String, String)> => |e| e.tree_marks();
}

#[tauri::command]
fn tick(held: State<'_, Held>) -> Option<String> {
    let mut editor = held.0.lock().expect("the editor");
    let changed = editor.tick();
    editor.hand_off(tracelean_core::launch::start, tracelean_core::launch::ask);
    changed.then(|| said(&editor))
}

#[tauri::command]
fn take_clipboard(held: State<'_, Held>) -> Option<String> {
    held.0.lock().expect("the editor").clipboard.take()
}

/// The theme in use. The zoom is the webview's own, so everything drawn scales together
/// and every position the page measures stays true.
///
/// @implements REQ-LOOK.zoom_steps
#[tauri::command]
fn theme(held: State<'_, Held>, window: tauri::WebviewWindow) -> serde_json::Value {
    let root = held.0.lock().expect("the editor").root.clone();
    let theme = tracelean_editor::theme::load(&root);
    let percent = theme["layout"]["zoom"].as_str().and_then(|zoom| zoom.parse::<f64>().ok()).unwrap_or(100.0);
    let _ = window.set_zoom(percent.clamp(60.0, 200.0) / 100.0);
    theme
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
            problems, palette, selected, symbols, occurrences, offers_here, history_preview, leader, zoom, overview, card, impact, runs,
            modules, agent_steps, overview_markdown, tree_marks
        ])
        .run(tauri::generate_context!())
        .expect("the window opens");
}
