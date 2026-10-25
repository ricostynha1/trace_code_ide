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

    editor.perform(Intent::Display { what: BufferKind::File { path: "src/i.rs".into() } });
    assert_eq!(text(&editor), before, "the buffer did not come back with the file");
    let _ = std::fs::remove_dir_all(&root);
}

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

#[test]
fn a_file_is_opened_from_the_listing_by_keys() {
    let root = project();
    let mut editor = Editor::open(root.clone(), keymap());

    assert_eq!(editor.focus().under, None, "{}", text(&editor));
    editor.key("Down");
    assert_eq!(editor.focus().under.as_deref(), Some("src"), "{}", text(&editor));
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

#[test]
fn an_unbound_key_says_nothing_happened() {
    let (root, mut editor) = opened();
    editor.key("Z");
    let said = plain_text(editor.status.clone()).join("\n");
    assert!(said.contains("nothing happened"), "an unbound key was silent: {said:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.one_path
#[test]
fn the_space_bar_types_a_space_in_insert_mode() {
    let (root, mut editor) = opened();
    editor.key("i");
    for key in ["a", "Space", "b"] {
        editor.key(key);
    }
    assert_eq!(
        editor.workspace().files.get("src/i.rs").map(String::as_str),
        Some("a bone\ntwo\nthree\n"),
        "the space bar did not type a space"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.edits_are_commands
#[test]
fn typing_after_multibyte_characters_lands_at_the_cursor() {
    let root = project();
    let original = "theorem a : ∀ x, x → x := by\n  intro x\n  exact x\n-- λ end\nlast\n";
    std::fs::write(root.join("src/m.lean"), original).unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/m.lean".into() } });
    editor.key("i");
    for key in ["Down", "Down", "Right", "Right", "z"] {
        editor.key(key);
    }
    let file = |editor: &Editor| editor.workspace().files.get("src/m.lean").cloned().expect("the file");
    assert_eq!(file(&editor), "theorem a : ∀ x, x → x := by\n  intro x\n  zexact x\n-- λ end\nlast\n");
    for key in ["Down", "Down", "y"] {
        editor.key(key);
    }
    assert_eq!(file(&editor), "theorem a : ∀ x, x → x := by\n  intro x\n  zexact x\n-- λ end\nlasyt\n");
    editor.key("Backspace");
    assert_eq!(file(&editor), "theorem a : ∀ x, x → x := by\n  intro x\n  zexact x\n-- λ end\nlast\n");
    editor.perform(Intent::Travel { move_: tracelean_core::surface::act::Move::Back });
    editor.key("q");
    assert_eq!(file(&editor), "theorem a : ∀ x, x → x := by\n  intro x\n  zexact x\n-- λ end\nlasyqt\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.edits_are_commands
#[test]
fn deleting_a_file_closes_its_buffer() {
    use tracelean_core::history::command::Command;
    let (root, mut editor) = opened();
    assert_eq!(editor.buffer().kind, BufferKind::File { path: "src/i.rs".into() });
    editor.perform(Intent::Edit {
        command: Command::DeleteFile { path: "src/i.rs".into(), content: "one\ntwo\nthree\n".into() },
    });
    assert_ne!(
        editor.buffer().kind,
        BufferKind::File { path: "src/i.rs".into() },
        "the buffer of a deleted file stayed open"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.one_path
#[test]
fn rename_by_keys_asks_for_the_name_and_renames() {
    let (root, mut editor) = opened();
    for key in ["Space", "f", "r"] {
        editor.key(key);
    }
    let said = text_of_status(&editor);
    assert!(said.contains("New name for src/i.rs"), "the rename did not ask: {said}");
    for _ in 0.."src/i.rs".len() {
        editor.key("Backspace");
    }
    for key in ["s", "r", "c", "/", "j", ".", "r", "s"] {
        editor.key(key);
    }
    editor.key("Enter");
    let files = editor.workspace().files;
    assert!(files.contains_key("src/j.rs") && !files.contains_key("src/i.rs"), "{:?}", files.keys());
    let _ = std::fs::remove_dir_all(&root);
}

fn text_of_status(editor: &Editor) -> String {
    plain_text(editor.status.clone()).join("\n")
}

/// @tests REQ-NAV.name_is_asked_for
/// @tests REQ-NAV.find_by_glob
/// @tests REQ-NAV.find_lists_lines
#[test]
fn find_asks_on_the_status_line_and_takes_a_pattern() {
    let (root, mut editor) = opened();
    for key in ["Space", "f", "f"] {
        editor.key(key);
    }
    assert!(text_of_status(&editor).contains("Find"), "{}", text_of_status(&editor));
    for key in ["t", "?", "o", "Enter"] {
        editor.key(key);
    }
    assert_eq!(editor.offset, 7, "the cursor did not go to the end of `two`");

    for key in ["Space", "f", "/"] {
        editor.key(key);
    }
    for _ in 0..3 {
        editor.key("Backspace");
    }
    for key in ["t", "h", "*", "Enter"] {
        editor.key(key);
    }
    let listed = plain_text(editor.buffer()).join("\n");
    assert!(listed.contains("three") && !listed.contains("two"), "{listed}");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-LOOK.setting_edited_in_place
#[test]
fn a_setting_is_written_to_the_override_and_a_bad_value_is_refused() {
    let (root, mut editor) = opened();
    editor.set_setting("ui.accent", "#ff0000");
    let theme = tracelean_editor::theme::load(&root);
    assert_eq!(theme["ui"]["accent"], "#ff0000");
    assert!(theme["ui"]["sidebar"].is_string(), "the rest of the theme survived");

    editor.set_setting("ui.accent", "red");
    assert_eq!(tracelean_editor::theme::load(&root)["ui"]["accent"], "#ff0000", "a bad value changed the file");
    assert!(text_of_status(&editor).contains("not a colour"), "{}", text_of_status(&editor));

    editor.perform(Intent::Display { what: BufferKind::Record { title: "definitions".into() } });
    assert!(text(&editor).contains("ui.accent  #ff0000"), "{}", text(&editor));
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-LSP.proof_state_shown
#[test]
fn the_proof_panel_names_why_a_lean_file_has_no_goals() {
    let root = project();
    std::fs::write(root.join("src/m.lean"), "theorem a : 1 = 1 := rfl\n").unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/m.lean".into() } });
    editor.perform(Intent::Display { what: BufferKind::Record { title: "proof".into() } });
    assert!(text(&editor).contains("Open a Lean file"), "{}", text(&editor));
    assert!(editor.tick(), "the panel did not change");
    assert!(text(&editor).contains("no Lean project"), "{}", text(&editor));
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.one_path
#[test]
fn scrolling_many_lines_at_once_lands_where_asked() {
    let root = project();
    let long: String = (0..50).map(|n| format!("line {n}\n")).collect();
    std::fs::write(root.join("src/long.rs"), &long).unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/long.rs".into() } });
    editor.scroll("document", 20);
    assert_eq!(editor.line_and_column().0, 20);
    editor.scroll("document", -5);
    assert_eq!(editor.line_and_column().0, 15);
    editor.scroll("document", 1000);
    assert_eq!(editor.line_and_column().0, 50, "past the last line stops on it");
    editor.scroll("document", -1000);
    assert_eq!(editor.line_and_column().0, 0);
    let _ = std::fs::remove_dir_all(&root);
}
