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

/// @tests REQ-REPORT.static_page
/// @tests REQ-SCREEN.station_produces_a_buffer
#[test]
fn the_report_station_writes_the_page_and_offers_the_revisions_to_compare() {
    let (root, mut editor) = opened();
    for key in ["Space", "w", "t", "e"] {
        editor.key(key);
    }
    let page = root.join("target/tracelean-report/index.html");
    let shown = plain_text(editor.buffer()).join("\n");
    assert!(page.is_file(), "{shown}");
    assert!(std::fs::read_to_string(&page).unwrap().contains("<html"), "not a page");
    assert!(shown.contains(&page.display().to_string()) && shown.contains("clauses"), "{shown}");
    let at = editor.buffer().text.find("main").expect("main is offered");
    let span = editor.buffer().spans.iter().find(|s| s.start == at).expect("main is a span").clone();
    assert_eq!(span.actions, vec!["trace.report_since".to_string()]);
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-REPORT.static_page
#[test]
fn the_report_names_its_page_by_an_absolute_path_when_the_project_is_relative() {
    let made = project();
    let here = std::env::current_dir().unwrap();
    let relative = pathdiff(&made, &here);
    assert!(relative.is_relative(), "{}", relative.display());
    let mut editor = Editor::open(relative, keymap());
    for key in ["Space", "w", "t", "e"] {
        editor.key(key);
    }
    let shown = plain_text(editor.buffer()).join("\n");
    let page = made.canonicalize().unwrap().join("target/tracelean-report/index.html");
    assert!(shown.contains(&format!("file://{}", page.display())), "{shown}");
    let _ = std::fs::remove_dir_all(&made);
}

fn pathdiff(to: &std::path::Path, from: &std::path::Path) -> PathBuf {
    let ups = from.components().count() - 1;
    let mut out: PathBuf = (0..ups).map(|_| "..").collect();
    out.extend(to.components().skip(1));
    out
}

/// @tests REQ-HANDOFF.file_to_vscode
#[test]
fn space_f_v_hands_the_file_to_vscode_at_the_cursor() {
    let (root, mut editor) = opened();
    editor.offset = "one\ntw".chars().count();
    for key in ["Space", "f", "v"] {
        editor.key(key);
    }
    let file = root.canonicalize().unwrap().join("src/i.rs").display().to_string();
    assert_eq!(editor.handed.last().cloned().unwrap_or_default().last().cloned(), Some(format!("{file}:2:3")), "{:?}", editor.handed);
    let mut started = Vec::new();
    editor.hand_off(
        |argv| {
            started.push(argv.to_vec());
            Ok(())
        },
        |_| unreachable!("nothing asked for a folder"),
    );
    assert_eq!(started.len(), 1);
    assert!(editor.handed.is_empty(), "a hand-off was kept after it was started");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-HANDOFF.failure_named
#[test]
fn a_hand_off_that_fails_is_named_on_the_status_line() {
    let (root, mut editor) = opened();
    for key in ["Space", "f", "v"] {
        editor.key(key);
    }
    editor.hand_off(|_| Err("cannot start code: not found".into()), |_| unreachable!("nothing asked for a folder"));
    let status = plain_text(editor.status.clone()).join("\n");
    assert!(status.contains("cannot start code"), "{status}");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-HANDOFF.report_to_browser
#[test]
fn the_report_opens_in_the_browser_from_the_editor_and_not_from_a_view() {
    let (root, mut editor) = opened();
    editor.view(BufferKind::Record { title: "report".into() });
    assert!(editor.handed.is_empty(), "the text view started a browser: {:?}", editor.handed);
    for key in ["Space", "w", "t", "e"] {
        editor.key(key);
    }
    let page = root.canonicalize().unwrap().join("target/tracelean-report/index.html").display().to_string();
    assert_eq!(editor.handed.last().and_then(|argv| argv.last()).cloned(), Some(page), "{:?}", editor.handed);
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-HANDOFF.folder_picked
/// @tests REQ-HANDOFF.choice_opened
#[test]
fn space_p_opens_the_folder_the_picker_chose_and_nothing_when_closed() {
    let (root, mut editor) = opened();
    let other = project();
    std::fs::write(other.join("src/chosen.rs"), "fn chosen() {}\n").unwrap();
    for key in ["Space", "p"] {
        editor.key(key);
    }
    assert!(editor.picker.is_some(), "no picker was asked for");
    let said = format!("{}\n", other.display());
    editor.hand_off(|_| Ok(()), |_| {
        let (tell, answer) = std::sync::mpsc::channel();
        tell.send(Ok(said)).unwrap();
        answer
    });
    assert!(editor.tick(), "the chosen folder was not picked up");
    assert_eq!(editor.root, other, "the chosen folder was not opened");

    for key in ["Space", "p"] {
        editor.key(key);
    }
    editor.hand_off(|_| Ok(()), |_| {
        let (tell, answer) = std::sync::mpsc::channel();
        tell.send(Ok(String::new())).unwrap();
        answer
    });
    assert!(editor.tick());
    assert_eq!(editor.root, other, "closing the picker opened something");
    assert!(plain_text(editor.status.clone()).join("\n").contains("no folder was chosen"));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}
