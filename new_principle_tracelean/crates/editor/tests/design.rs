//! The design station, from the editor: opened folded to its roots, unfolded a
//! level by its mark, all of it under a node at once, and folded back.

use std::path::PathBuf;

use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::{plain_text, BufferKind};
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-design-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, content).expect("a file");
    }
    dir
}

const TREE: &[(&str, &str)] = &[
    ("reqs/arch.md", "---\nid: ARCH-X\ntitle: Root\nclauses:\n  r: It is.\n---\n"),
    ("reqs/a.md", "---\nid: REQ-A\ntitle: Middle\nrefines: [ARCH-X]\nclauses:\n  a: It is.\n---\n"),
    ("reqs/b.md", "---\nid: REQ-B\ntitle: Leaf\nrefines: [REQ-A]\nclauses:\n  b: It is.\n---\n"),
];

fn here(editor: &mut Editor, action: &str, target: Option<&str>) {
    let pane = editor.screen.focus.clone();
    editor.choose(&pane, 0, action, target.map(str::to_string), None);
}

fn rows(editor: &Editor) -> Vec<String> {
    // The id is the third word: mark, grade, id.
    plain_text(editor.buffer())
        .into_iter()
        .skip(1)
        .map(|l| l.split_whitespace().nth(2).unwrap_or("").to_string())
        .collect()
}

/// @tests REQ-SHOW.graph_from_refinement
#[test]
fn the_design_opens_on_its_roots_and_unfolds_on_request() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(tracelean_core::surface::act::Intent::Display { what: BufferKind::Menu { title: "design".into() } });
    assert_eq!(rows(&editor), vec!["ARCH-X"], "the design did not open folded");

    here(&mut editor, "design.toggle", Some("ARCH-X"));
    assert_eq!(rows(&editor), vec!["ARCH-X", "REQ-A"], "one level did not unfold");

    here(&mut editor, "design.toggle", Some("ARCH-X"));
    assert_eq!(rows(&editor), vec!["ARCH-X"], "it did not fold again");

    here(&mut editor, "design.expand_all", Some("ARCH-X"));
    assert_eq!(rows(&editor), vec!["ARCH-X", "REQ-A", "REQ-B"], "everything under it did not unfold");

    here(&mut editor, "design.fold_all", None);
    assert_eq!(rows(&editor), vec!["ARCH-X"]);
    here(&mut editor, "design.expand_everything", None);
    assert_eq!(rows(&editor).len(), 3);
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.one_path
#[test]
fn a_key_on_a_fold_mark_unfolds_its_row() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(tracelean_core::surface::act::Intent::Display { what: BufferKind::Menu { title: "design".into() } });
    // The cursor on the mark of the first row, past the header line.
    let header = plain_text(editor.buffer())[0].chars().count();
    editor.offset = header + 1;
    for key in ["Space", "g", "o"] {
        editor.key(key);
    }
    assert_eq!(rows(&editor), vec!["ARCH-X", "REQ-A"], "the key acted on the mark's glyph, not its row");
    let _ = std::fs::remove_dir_all(&root);
}
