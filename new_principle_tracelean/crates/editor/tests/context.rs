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
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("tracelean-context-{}-{n}", std::process::id()));
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

/// Going to the definition of a requirement name in code opens its document
/// at the clause's own line.
#[test]
fn a_requirement_name_is_defined_at_its_clause() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    here(&mut editor, "file.open", Some("src/i.rs"));
    editor.offset = "/// @implements REQ".chars().count();
    editor.chord("F12");
    assert!(matches!(editor.buffer().kind, tracelean_core::surface::view::BufferKind::File { ref path } if path == "reqs/a.md"));
    let said = plain_text(editor.status.clone()).join("\n");
    assert!(said.contains("reqs/a.md:6"), "{said}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Inside the body of an item that claims a clause, what the cursor is on
/// offers that clause — to open, and to gather for an agent.
#[test]
fn inside_a_claiming_item_its_clause_is_offered() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    here(&mut editor, "file.open", Some("src/i.rs"));
    editor.offset = "/// @implements REQ-A.one\npub fn thing() -> u8 {\n    ".chars().count();
    let (pane, at, offered) = editor.offers_here();
    let context = offered
        .iter()
        .find(|o| o.action == "trace.context" && o.target.as_deref() == Some("REQ-A.one"))
        .unwrap_or_else(|| panic!("the clause is offered: {offered:?}"));
    assert_eq!(context.group, "here");
    editor.choose(&pane, at, "trace.context", context.target.clone(), None);
    let shown = plain_text(editor.buffer()).join("\n");
    assert!(shown.contains("Context for an agent"), "{shown}");
    let _ = std::fs::remove_dir_all(&root);
}

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
    // What will be copied reads as the Markdown it is: its headings marked.
    let headings = editor
        .buffer()
        .spans
        .iter()
        .filter(|s| s.role == tracelean_core::surface::view::Role::Token { kind: tracelean_core::surface::view::TokenKind::Heading })
        .count();
    assert!(headings >= 2, "the preview's headings are marked: {headings}");

    // Leave the code out, take what refines it in.
    here(&mut editor, "context.toggle", Some("code"));
    here(&mut editor, "context.toggle", Some("refined by"));
    here(&mut editor, "context.copy", None);
    let copied = editor.clipboard.take().expect("the context is on the clipboard");

    assert!(copied.contains("**one** (the clause being changed): It does the thing."), "{copied}");
    assert!(copied.contains("- one: no Lean model\n"), "what the clause still lacks:\n{copied}");
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
