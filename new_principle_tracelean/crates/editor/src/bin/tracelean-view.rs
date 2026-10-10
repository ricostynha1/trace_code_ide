//! The editor's views, printed: what a person reads in a station or a report,
//! for an agent in a shell. Every view is the buffer the window draws,
//! produced by the same editor; this only picks one and prints its text.
//!
//! The tree is opened fresh and nothing is kept, so a person's open tabs are
//! never touched, and no view that writes (`lock`) is offered.
//!
//! ```text
//! tracelean-view [root] <view> [argument] [--json]
//! ```

use std::path::PathBuf;

use tracelean_core::surface::act::Intent;
use tracelean_core::surface::keymap;
use tracelean_core::surface::screen::Rect;
use tracelean_core::surface::view::{plain_text, BufferKind};
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../../assets/keymap.json");

/// Each view: its name, whether it takes an argument, and what it shows.
const VIEWS: &[(&str, Option<&str>, &str)] = &[
    ("requirements", None, "every requirement, its level, and how many clauses something claims"),
    ("design", None, "the same, indented along `refines`: what each requirement depends on"),
    ("requirement", Some("REQ-X"), "one requirement: each clause, its level, what claims it, coverage, judgement"),
    ("trace", Some("FILE"), "what a file claims, and everything else claiming the same clauses"),
    ("context", Some("REQ-X[.clause]"), "everything needed to change a requirement or clause"),
    ("findings", None, "every finding of the checker: Unmodeled, Unbound, Dangling, …"),
    ("evidence", None, "each clause's evidence chain and the level the lock records"),
    ("rollup", None, "coverage at floor L3, down the refinement tree"),
    ("stale", None, "every document link's state"),
    ("bindings", None, "the differential bindings in .tracelean/drt.json"),
    ("keys", None, "every key, by mode"),
];

fn usage() -> String {
    let mut out = String::from("tracelean-view [root] <view> [argument] [--json]\n\n");
    for (name, argument, said) in VIEWS {
        let call = match argument {
            Some(a) => format!("{name} {a}"),
            None => name.to_string(),
        };
        out.push_str(&format!("  {call:<28} {said}\n"));
    }
    out
}

/// The buffer kind a view names.
fn kind(view: &str, argument: Option<&str>) -> Option<BufferKind> {
    let record = |title: String| Some(BufferKind::Record { title });
    match (view, argument) {
        ("requirements" | "design", None) => Some(BufferKind::Menu { title: view.to_string() }),
        ("requirement", Some(id)) => record(format!("requirement {id}")),
        ("context", Some(target)) => record(format!("context {target}")),
        ("trace", Some(_)) => record("trace".into()),
        ("bindings", None) => record("drt bindings".into()),
        ("findings" | "evidence" | "rollup" | "stale" | "keys", None) => record(view.to_string()),
        _ => None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let words: Vec<&str> = args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    // A first word that is a directory is the root; the rest name the view.
    let (root, words) = match words.split_first() {
        Some((first, rest)) if std::path::Path::new(first).is_dir() => (PathBuf::from(first), rest.to_vec()),
        _ => (PathBuf::from("."), words),
    };
    let (view, argument) = (words.first().copied().unwrap_or(""), words.get(1).copied());
    let Some(what) = kind(view, argument) else {
        eprint!("{}", usage());
        std::process::exit(if view.is_empty() || view == "help" { 0 } else { 2 });
    };
    let keys = keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads");
    let mut editor = Editor::open_fresh(root, keys);
    // Wide and tall enough that nothing a view lays out to its pane is cut.
    editor.region = Rect { left: 0, top: 0, width: 200, height: 1000 };
    // A file's trace is of the file the document shows.
    if let ("trace", Some(file)) = (view, argument) {
        editor.perform(Intent::Display { what: BufferKind::File { path: file.to_string() } });
    }
    let buffer = editor.view(what);
    if json {
        println!("{}", serde_json::to_string_pretty(&buffer).unwrap_or_default());
    } else {
        println!("{}", plain_text(buffer).join("\n"));
    }
}
