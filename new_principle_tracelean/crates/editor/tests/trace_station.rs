//! The trace station follows the file the document shows: opened after one
//! file it is that file's trace, after another the other's.

use std::path::PathBuf;

use tracelean_core::surface::act::Intent;
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::{plain_text, BufferKind};
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-trace-station-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, content).expect("a file");
    }
    dir
}

const TREE: &[(&str, &str)] = &[
    ("reqs/a.md", "---\nid: REQ-A\ntitle: A\nclauses:\n  one: It is.\n  two: It is too.\n---\n"),
    ("src/one.rs", "/// @implements REQ-A.one\npub fn one() {}\n"),
    ("src/two.rs", "/// @implements REQ-A.two\npub fn two() {}\n"),
];

fn trace_of(editor: &mut Editor, path: &str) -> String {
    editor.perform(Intent::Display { what: BufferKind::File { path: path.into() } });
    plain_text(editor.view(BufferKind::Record { title: "trace".into() })).join("\n")
}

#[test]
fn the_trace_is_of_the_file_last_opened() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    let one = trace_of(&mut editor, "src/one.rs");
    assert!(one.contains("REQ-A.one") && !one.contains("REQ-A.two"), "{one}");
    let two = trace_of(&mut editor, "src/two.rs");
    assert!(two.contains("REQ-A.two") && !two.contains("REQ-A.one"), "{two}");
    let _ = std::fs::remove_dir_all(&root);
}
