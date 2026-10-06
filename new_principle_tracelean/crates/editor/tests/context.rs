//! An agent's context, from the editor: opened from a requirement, parts left
//! out by the person, and the rest copied to the clipboard — and nothing else.

use std::path::PathBuf;

use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::plain_text;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-context-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, content).expect("a file");
    }
    dir
}

const TREE: &[(&str, &str)] = &[
    ("reqs/a.md", "---\nid: REQ-A\ntitle: The thing\ndecomposition: complete\nclauses:\n  one: It does the thing.\n---\n"),
    ("reqs/b.md", "---\nid: REQ-B\ntitle: A finer thing\nrefines: [REQ-A]\nclauses:\n  two: It does it finely.\n---\n"),
    ("src/i.rs", "/// @implements REQ-A.one\npub fn thing() -> u8 {\n    1\n}\n"),
    ("tests/t.rs", "/// @tests REQ-A.one\n#[test]\nfn it_does() { assert_eq!(thing(), 1); }\n\n/// @tests REQ-B.two\n#[test]\nfn it_does_finely() { assert!(thing() > 0); }\n"),
    ("tests/plain.rs", "#[test]\nfn unannotated() { thing(); }\n"),
];

fn here(editor: &mut Editor, action: &str, target: Option<&str>) {
    let pane = editor.screen.focus.clone();
    editor.choose(&pane, 0, action, target.map(str::to_string), None);
}

/// @tests REQ-CONTEXT.claims_with_source
/// @tests REQ-CONTEXT.affected_tests
/// @tests REQ-CONTEXT.person_chooses
/// @tests REQ-CONTEXT.copied_not_sent
#[test]
fn a_context_is_chosen_part_by_part_and_copied() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    here(&mut editor, "trace.context", Some("REQ-A.one"));
    let shown = plain_text(editor.buffer()).join("\n");
    assert!(shown.contains("Context for an agent"), "{shown}");
    assert!(shown.contains("[ ] refined by"), "what refines it is left out until chosen:\n{shown}");

    // Leave the code out, take what refines it in.
    here(&mut editor, "context.toggle", Some("code"));
    here(&mut editor, "context.toggle", Some("refined by"));
    here(&mut editor, "context.copy", None);
    let copied = editor.clipboard.take().expect("the context is on the clipboard");

    assert!(copied.contains("**one** (the clause being changed): It does the thing."), "{copied}");
    assert!(copied.contains("REQ-B — A finer thing"), "what refines it was chosen:\n{copied}");
    assert!(!copied.contains("## Code that implements it"), "the code was left out:\n{copied}");
    // The test claiming it, with its source; the test of what refines it, and
    // an unannotated test naming the implementation, as affected.
    assert!(copied.contains("fn it_does()"), "{copied}");
    assert!(copied.contains("## Other tests that may break"), "{copied}");
    assert!(copied.contains("fn it_does_finely()"), "{copied}");
    assert!(copied.contains("tests/plain.rs:2"), "{copied}");
    let _ = std::fs::remove_dir_all(&root);
}
