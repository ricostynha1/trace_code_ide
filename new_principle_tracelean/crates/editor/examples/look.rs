//! The window's commands, answered by a real editor over standard input and
//! output — so the page can be looked at in a browser (`web/test/look.mjs`
//! serves it and photographs it in headless Chrome) without opening a window
//! on anybody's desktop.
//!
//! The commands answer as `crates/desktop/src/main.rs` answers them: the same
//! editor calls, the same shapes. Only the transport differs. This reads no
//! file and opens no socket; the script around it does both.
//!
//! Run: `cargo run -p tracelean-editor --example look -- demo`

use std::io::{BufRead, Write};

use serde_json::{json, Value};
use tracelean_core::surface::keymap;
use tracelean_core::surface::produce::window;
use tracelean_core::surface::screen::Rect;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn said(editor: &Editor) -> Value {
    json!(tracelean_core::surface::view::plain_text(editor.status.clone()).join("\n"))
}

fn text(args: &Value, key: &str) -> String {
    args[key].as_str().unwrap_or_default().to_string()
}

fn number(args: &Value, key: &str) -> usize {
    args[key].as_u64().unwrap_or(0) as usize
}

fn signed(args: &Value, key: &str) -> i64 {
    args[key].as_i64().unwrap_or(0)
}

fn optional(args: &Value, key: &str) -> Option<String> {
    args[key].as_str().map(str::to_string)
}

/// One command, as the window answers it.
fn answer(editor: &mut Editor, command: &str, args: &Value) -> Value {
    match command {
        "shown" => {
            let region = Rect {
                left: 0,
                top: 0,
                width: args["width"].as_u64().unwrap_or(80).max(1),
                height: args["height"].as_u64().unwrap_or(24).max(1),
            };
            editor.region = region;
            editor.follow_cursor(region.height as usize);
            let panes: Vec<Value> = editor
                .laid_out(region)
                .into_iter()
                .map(|placed| {
                    let height = placed.at.height as usize;
                    json!({
                        "chips": editor.chips_shown(&placed.buffer, placed.top, height),
                        "buffer": window(placed.buffer, placed.top, height),
                        "pane": placed.pane,
                        "at": placed.at,
                        "focused": placed.focused,
                        "top": placed.top,
                    })
                })
                .collect();
            json!(panes)
        }
        "cursor" => json!(editor.cursor_in(number(args, "height"))),
        "bars" => json!((editor.stations(), editor.strip())),
        "act_in_bar" => {
            editor.act_in_bar(args["station"].as_bool().unwrap_or(false), &text(args, "action"), number(args, "offset"));
            said(editor)
        }
        "menu" => json!(editor.menu),
        "press" => {
            editor.key(&text(args, "key"));
            said(editor)
        }
        "act" => {
            editor.choose(&text(args, "pane"), number(args, "offset"), &text(args, "action"), None, None);
            said(editor)
        }
        "tick" => {
            if editor.tick() {
                said(editor)
            } else {
                Value::Null
            }
        }
        "take_clipboard" => json!(editor.clipboard.take()),
        "act_here" => {
            editor.act_here(&text(args, "action"));
            said(editor)
        }
        "place" => {
            editor.place(&text(args, "pane"), number(args, "offset"));
            said(editor)
        }
        "offers" => json!(editor.offers_at(&text(args, "pane"), number(args, "offset"))),
        "offers_here" => json!(editor.offers_here()),
        "history_preview" => json!(editor.history_preview_at(&text(args, "pane"), number(args, "offset"))),
        "choose" => {
            editor.choose(
                &text(args, "pane"),
                number(args, "offset"),
                &text(args, "action"),
                optional(args, "target"),
                optional(args, "answer"),
            );
            said(editor)
        }
        "chord" => {
            editor.chord(&text(args, "chord"));
            said(editor)
        }
        "find" => {
            editor.find(&text(args, "text"), args["forward"].as_bool().unwrap_or(true));
            said(editor)
        }
        "replace" => {
            editor.replace_all(&text(args, "text"), &text(args, "with"));
            said(editor)
        }
        "described" => json!(tracelean_core::surface::offer::described(&editor.keymap)),
        "scroll" => {
            editor.scroll(&text(args, "pane"), signed(args, "lines"));
            said(editor)
        }
        "cut" => {
            editor.cut(&text(args, "pane"), number(args, "start"), number(args, "end"));
            said(editor)
        }
        "select" => {
            editor.select(&text(args, "pane"), number(args, "start"), number(args, "end"));
            said(editor)
        }
        "selection" => json!(editor.selection_shown()),
        "occurrences" => json!(editor.occurrences_shown()),
        "selected" => json!(editor.selected()),
        "mode" => json!(editor.mode),
        "symbols" => json!(editor.symbols(&text(args, "query"))),
        "palette" => json!(editor.palette(&text(args, "query"))),
        "problems" => json!(editor.problems()),
        "brackets" => json!(editor.bracket_pair()),
        "over_lines" => {
            editor.over_lines(&text(args, "pane"), number(args, "start"), number(args, "end"), &text(args, "chord"));
            said(editor)
        }
        "requirement_text" => json!(editor.requirement_text(&text(args, "name"))),
        "quick" => json!(editor.quick_open(&text(args, "query"))),
        "search" => {
            editor.list_occurrences(&text(args, "text"), false);
            said(editor)
        }
        "paste" => {
            editor.paste(&text(args, "text"));
            said(editor)
        }
        "grab" => {
            editor.grab(&text(args, "pane"), signed(args, "amount"));
            said(editor)
        }
        "theme" => tracelean_editor::theme::load(&editor.root),
        _ => json!(format!("no command {command}")),
    }
}

/// One request a line in — `{"command": …, "args": {…}}` — one answer a line
/// out, as JSON.
fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let keymap = keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads");
    let mut editor = Editor::open(root.into(), keymap);
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let command = request["command"].as_str().unwrap_or_default();
        let reply = answer(&mut editor, command, &request["args"]);
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}
