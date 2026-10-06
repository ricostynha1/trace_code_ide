//! Typing, moving and undoing — the path a person is on the whole time.
//!
//! Driven through `key`, which is the function both frontends call, so what is
//! checked here is what a keystroke does in the terminal and in the window
//! alike rather than what either of them does on its own.

use std::path::PathBuf;

use tracelean_core::surface::act::Intent;
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::{plain_text, BufferKind};
use tracelean_editor::{Editor, INSERT};

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn project() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tracelean-editing-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("a directory");
    std::fs::write(dir.join("src/i.rs"), "one\ntwo\nthree\n").expect("a file");
    dir
}

fn opened() -> (PathBuf, Editor) {
    let root = project();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/i.rs".into() } });
    (root, editor)
}

fn text(editor: &Editor) -> String {
    plain_text(editor.buffer()).join("\n")
}

/// `i` makes a key text, Escape makes it a command again.
#[test]
fn insert_mode_is_entered_and_left() {
    let (root, mut editor) = opened();
    assert_ne!(editor.mode, INSERT, "the editor started in insert mode");
    editor.key("i");
    assert_eq!(editor.mode, INSERT, "`i` did not enter insert mode");
    editor.key(keymap::LEAVE);
    assert_ne!(editor.mode, INSERT, "Escape did not leave insert mode");
    let _ = std::fs::remove_dir_all(&root);
}

/// Typing goes into the file, at the cursor, and comes back out with undo.
///
/// Through the history tree, which is why undo is not a special case: the
/// keystroke and an agent's change are the same kind of thing.
#[test]
fn what_is_typed_can_be_undone() {
    let (root, mut editor) = opened();
    let before = text(&editor);

    editor.key("i");
    for key in ["z", "z"] {
        editor.key(key);
    }
    editor.key(keymap::LEAVE);

    let typed = editor.workspace().files.get("src/i.rs").cloned().expect("the file");
    assert!(typed.starts_with("zz"), "what was typed is not in the file: {typed:?}");

    editor.perform(Intent::Travel { move_: tracelean_core::surface::act::Move::Back });
    editor.perform(Intent::Travel { move_: tracelean_core::surface::act::Move::Back });
    let undone = editor.workspace().files.get("src/i.rs").cloned().expect("the file");
    assert_eq!(undone, "one\ntwo\nthree\n", "undo did not put the file back");

    // And the buffer still reads as a buffer afterwards.
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/i.rs".into() } });
    assert_eq!(text(&editor), before, "the buffer did not come back with the file");
    let _ = std::fs::remove_dir_all(&root);
}

/// Backspace removes what is behind the cursor, carrying what it removed.
#[test]
fn backspace_removes_the_character_before_the_cursor() {
    let (root, mut editor) = opened();
    editor.key("i");
    editor.key("z");
    editor.key("y");
    assert_eq!(
        editor.workspace().files.get("src/i.rs").map(String::as_str),
        Some("zyone\ntwo\nthree\n"),
        "typing did not reach the file"
    );
    editor.key("Backspace");
    let after = editor.workspace().files.get("src/i.rs").cloned().expect("the file");
    assert_eq!(after, "zone\ntwo\nthree\n", "backspace removed the wrong character");
    let _ = std::fs::remove_dir_all(&root);
}

/// Moving down a line moves the cursor a line, not a character.
#[test]
fn the_cursor_moves_by_lines_and_by_characters() {
    let (root, mut editor) = opened();
    assert_eq!(editor.line_and_column(), (0, 0), "the cursor did not start at the top");
    editor.key("Down");
    assert_eq!(editor.line_and_column().0, 1, "Down did not move a line");
    editor.key("Right");
    assert_eq!(editor.line_and_column(), (1, 1), "Right did not move a character");
    editor.key("Up");
    assert_eq!(editor.line_and_column().0, 0, "Up did not move back");
    let _ = std::fs::remove_dir_all(&root);
}

/// A window shows the lines it has room for, and the cursor follows into it.
///
/// The window is produced by the core, so a frontend still draws everything it
/// is given — this checks the editor asks for the right one.
#[test]
fn the_window_follows_the_cursor() {
    let root = project();
    std::fs::write(root.join("src/long.rs"), (0..40).map(|n| format!("line {n}\n")).collect::<String>())
        .expect("a file");
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/long.rs".into() } });

    let height = 10;
    editor.follow_cursor(height);
    let top = plain_text(editor.visible(height));
    assert_eq!(top.len(), height, "the window was not the height asked for");
    assert_eq!(top[0], "line 0", "the window did not start at the top");

    for _ in 0..30 {
        editor.key("Down");
    }
    editor.follow_cursor(height);
    let down = plain_text(editor.visible(height));
    assert_eq!(down.len(), height, "the window changed height");
    assert!(down.iter().any(|line| line == "line 30"), "the window did not follow: {down:?}");
    let (line, _) = editor.cursor_in(height).expect("the cursor is on screen");
    assert!(line < height, "the cursor was reported off the window");
    let _ = std::fs::remove_dir_all(&root);
}

/// The path a person actually takes: land on a file in the listing, open it.
///
/// Through keys only — `Down` to reach the entry, then the leader sequence —
/// because that is the path a frontend offers and the one that has to work.
#[test]
fn a_file_is_opened_from_the_listing_by_keys() {
    let root = project();
    let mut editor = Editor::open(root.clone(), keymap());

    // The first line names the project; under it, a small project's folders
    // are open, so the file is in reach.
    assert_eq!(editor.focus().under, None, "{}", text(&editor));
    editor.key("Down");
    assert_eq!(editor.focus().under.as_deref(), Some("src"), "{}", text(&editor));
    // Walk down the listing until the cursor is on the file.
    let mut found = false;
    for _ in 0..20 {
        if editor.focus().under.as_deref() == Some("src/i.rs") {
            found = true;
            break;
        }
        editor.key("Down");
    }
    assert!(found, "the listing never put the cursor on the file:\n{}", text(&editor));

    editor.key("Space");
    // The menu is offered beside the buffer, not instead of it — otherwise the
    // next key would be about a menu row rather than about the file.
    assert!(editor.menu.is_some(), "the leader offered no menu");
    assert_eq!(
        editor.focus().under.as_deref(),
        Some("src/i.rs"),
        "the menu moved the cursor off the file"
    );
    for key in ["f", "o"] {
        editor.key(key);
    }
    assert!(editor.menu.is_none(), "the menu stayed up after the action ran");
    assert_eq!(
        editor.buffer().kind,
        BufferKind::File { path: "src/i.rs".into() },
        "the leader sequence did not open the file"
    );
    assert_eq!(text(&editor), "one\ntwo\nthree\n", "the file did not come up");
    let _ = std::fs::remove_dir_all(&root);
}

/// A key nothing binds says so rather than doing nothing.
#[test]
fn an_unbound_key_says_nothing_happened() {
    let (root, mut editor) = opened();
    editor.key("Z");
    let said = plain_text(editor.status.clone()).join("\n");
    assert!(said.contains("nothing happened"), "an unbound key was silent: {said:?}");
    let _ = std::fs::remove_dir_all(&root);
}
