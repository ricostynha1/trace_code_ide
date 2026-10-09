//! A pointer and the keys every editor has: what a person does with a window.
//!
//! Driven through the methods the window's commands call — `choose`, `place`,
//! `offers_at`, `chord`, `scroll`, `paste` — so what is checked is what a click,
//! a right-click or Ctrl+S does, on a real tree on disk. The window itself only
//! turns a pointer into a pane and an offset, and is checked in a browser by
//! `web/test/pointer.mjs`.

use std::path::PathBuf;

use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::screen::{DOCUMENT, EXPLORER};
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
        "tracelean-pointer-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("a directory");
    let long: String = (1..=60).map(|n| format!("line {n}\n")).collect();
    std::fs::write(dir.join("src/i.rs"), "one\ntwo\nthree\n").expect("a file");
    std::fs::write(dir.join("src/long.rs"), long).expect("a file");
    dir
}

fn opened() -> (PathBuf, Editor) {
    let root = project();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 120, height: 20 };
    (root, editor)
}

/// A pane's edge dragged a few columns moves exactly that many, both ways, and
/// the panes still fill the window.
#[test]
fn a_dragged_edge_moves_by_the_columns_dragged() {
    let (root, mut editor) = opened();
    let width = |editor: &Editor, pane: &str| {
        editor.laid_out(editor.region).into_iter().find(|p| p.pane == pane).expect("the pane").at.width
    };
    let (tree, document) = (width(&editor, EXPLORER), width(&editor, DOCUMENT));
    editor.grab(EXPLORER, 3);
    assert_eq!((width(&editor, EXPLORER), width(&editor, DOCUMENT)), (tree + 3, document - 3));
    editor.grab(EXPLORER, -5);
    assert_eq!((width(&editor, EXPLORER), width(&editor, DOCUMENT)), (tree - 2, document + 2));
    let total: u64 = editor.laid_out(editor.region).iter().map(|p| p.at.width).sum();
    assert_eq!(total, 120, "the panes still fill the window");
    let _ = std::fs::remove_dir_all(&root);
}

/// Where the row naming `name` starts in the explorer's tree, if it is shown.
fn find_row(editor: &Editor, name: &str) -> Option<usize> {
    let listing = shown_in(editor, EXPLORER);
    let mut at = 0;
    for line in listing.split('\n') {
        let shown = line.trim_start().trim_start_matches(['▾', '▸']).trim_start();
        // Without the mark after a name (`✎`, `●`).
        let (shown, mark) = shown.split_once("  ").map_or((shown, 0), |(name, mark)| (name, mark.chars().count() + 2));
        if shown == name {
            return Some(at + line.chars().count() - mark - name.chars().count());
        }
        at += line.chars().count() + 1;
    }
    None
}

/// Where a path's row starts in the explorer, opening the folders above it
/// by clicking them, as a person would.
fn row_of(editor: &mut Editor, path: &str) -> usize {
    let parts: Vec<&str> = path.split('/').collect();
    for folder in &parts[..parts.len() - 1] {
        let listing = shown_in(editor, EXPLORER);
        if listing.contains(&format!("▸ {folder}")) {
            let at = find_row(editor, folder).expect("the folder is shown");
            editor.choose(EXPLORER, at, "file.open", None, None);
        }
    }
    let name = parts[parts.len() - 1];
    find_row(editor, name)
        .unwrap_or_else(|| panic!("{path} is not in the listing:\n{}", shown_in(editor, EXPLORER)))
}

/// Folders open and close from their row, and the tree shows names under
/// them; opening a file elsewhere opens the folders that hold it.
///
/// @tests REQ-SHOW.listing_is_a_tree
#[test]
fn folders_open_and_close_in_the_explorer() {
    let (root, mut editor) = opened();
    // A project this small opens with its folders open.
    assert!(shown_in(&editor, EXPLORER).ends_with("\n▾ src\n    i.rs\n    long.rs"), "{}", shown_in(&editor, EXPLORER));
    let at = find_row(&editor, "src").unwrap();
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(shown_in(&editor, EXPLORER).ends_with("\n▸ src"), "{}", shown_in(&editor, EXPLORER));
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(shown_in(&editor, EXPLORER).ends_with("\n▾ src\n    i.rs\n    long.rs"), "{}", shown_in(&editor, EXPLORER));
    let close = editor.offers_at(EXPLORER, at);
    assert!(close.iter().any(|o| o.label == "Close src"), "{close:?}");
    assert!(!close.iter().any(|o| o.action == "file.delete"), "a folder offered a file's delete");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(shown_in(&editor, EXPLORER).ends_with("\n▸ src"), "{}", shown_in(&editor, EXPLORER));

    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::File { path: "src/i.rs".into() },
    });
    assert!(shown_in(&editor, EXPLORER).contains("i.rs"), "opening a file did not reveal it");
    let _ = std::fs::remove_dir_all(&root);
}

fn shown_in(editor: &Editor, pane: &str) -> String {
    let placed = editor.laid_out(editor.region);
    let found = placed.iter().find(|p| p.pane == pane).expect("the pane is placed");
    plain_text(found.buffer.clone()).join("\n")
}

/// Clicking a row of the explorer opens that file in the document, and the
/// explorer is still there.
///
/// @tests REQ-SCREEN.buffer_goes_home
/// @tests REQ-ACT.one_path
#[test]
fn clicking_a_file_in_the_explorer_opens_it_beside_the_explorer() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert_eq!(editor.screen.focus, DOCUMENT);
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/i.rs"));
    assert!(shown_in(&editor, EXPLORER).contains("long.rs"), "the explorer was replaced");
    let _ = std::fs::remove_dir_all(&root);
}

/// A click into a file puts the cursor where it was pressed and starts typing
/// there.
#[test]
fn clicking_into_a_file_places_the_cursor_and_typing_goes_there() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    // "one\ntwo" — offset 5 is between `t` and `w`.
    editor.place(DOCUMENT, 5);
    assert_eq!(editor.mode, INSERT, "a click into a file did not start typing");
    editor.key("X");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntXwo\nthree\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// Resting on a history node shows the change it made, and the history's
/// switches keep it to the saved points or the open file.
///
/// @tests REQ-UNDO.hover_shows_change
/// @tests REQ-UNDO.filtered_view
#[test]
fn the_history_previews_a_change_and_filters_to_the_saved_points() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 5);
    editor.key("X");
    editor.chord("C-s");
    editor.key("Y");
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "history".into() },
    });
    let history = |editor: &Editor| {
        editor.laid_out(editor.region).into_iter().find(|p| p.buffer.id == "record:history").expect("the history is shown")
    };
    let shown = history(&editor);
    let text = plain_text(shown.buffer.clone()).join("\n");
    assert!(text.contains("#0") && text.contains("#1") && text.contains("#2"), "{text}");

    let offset_of = |text: &str, needle: &str| text[..text.find(needle).expect(needle)].chars().count();
    let preview = editor.history_preview_at(&shown.pane, offset_of(&text, "#1")).expect("a preview of #1");
    let said = plain_text(preview).join("\n");
    assert!(said.contains("+tXwo") && said.contains("-two"), "{said}");
    assert!(editor.history_preview_at(&shown.pane, 0).is_none(), "a switch is not a node");
    assert!(editor.history_preview_at(&shown.pane, offset_of(&text, "#0")).is_none(), "the base changed nothing");

    editor.choose(&shown.pane, offset_of(&text, "Saved"), "history.filter", None, None);
    let saved = plain_text(history(&editor).buffer).join("\n");
    assert!(saved.contains("#1") && !saved.contains("#2"), "only the saved point is left:\n{saved}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Clicking `#0` goes back to the tree as it was opened, and from there a
/// click on a node goes forward again.
///
/// @tests REQ-UNDO.reachable
#[test]
fn clicking_the_base_returns_to_the_tree_as_it_was_opened() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 5);
    editor.key("X");
    editor.key("Y");
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "history".into() },
    });
    let history = |editor: &Editor| {
        editor.laid_out(editor.region).into_iter().find(|p| p.buffer.id == "record:history").expect("the history is shown")
    };
    let offset_of = |text: &str, needle: &str| text[..text.find(needle).expect(needle)].chars().count();
    let file = |editor: &Editor| editor.workspace().files.get("src/i.rs").cloned().unwrap_or_default();
    assert_eq!(file(&editor), "one\ntXYwo\nthree\n");

    let shown = history(&editor);
    let text = plain_text(shown.buffer.clone()).join("\n");
    editor.choose(&shown.pane, offset_of(&text, "#0"), "history.jump", None, None);
    assert_eq!(file(&editor), "one\ntwo\nthree\n", "#0 is the tree as it was opened");

    let shown = history(&editor);
    let text = plain_text(shown.buffer.clone()).join("\n");
    assert!(text.contains("● #0"), "{text}");
    editor.choose(&shown.pane, offset_of(&text, "#2"), "history.jump", None, None);
    assert_eq!(file(&editor), "one\ntXYwo\nthree\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// The page reports offsets in the window it was given. Once the pane is
/// scrolled, offset 0 is the first *visible* line, not the first line.
#[test]
fn a_click_in_a_scrolled_pane_lands_on_the_line_that_was_clicked() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.scroll(DOCUMENT, 10);
    assert_eq!(editor.top, 10);
    editor.place(DOCUMENT, 0);
    assert_eq!(editor.line_and_column(), (10, 0), "the click landed on the wrong line");
    let _ = std::fs::remove_dir_all(&root);
}

/// Right-clicking a row offers what can be done to that file, and choosing
/// one does it to that file — not to whatever the focus was on.
///
/// @tests REQ-ACT.everything_is_offered
#[test]
fn a_file_is_renamed_and_deleted_from_its_row() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    let offered = editor.offers_at(EXPLORER, at);
    let rename = offered.iter().find(|o| o.action == "file.rename").expect("rename is offered");
    assert_eq!(rename.target.as_deref(), Some("src/i.rs"));
    assert!(rename.asks.is_some(), "rename does not ask for the new name");

    editor.choose(EXPLORER, at, "file.rename", Some("src/i.rs".into()), Some("src/j.rs".into()));
    let listing = shown_in(&editor, EXPLORER);
    assert!(listing.contains("j.rs") && !listing.contains("i.rs"), "{listing}");

    let at = row_of(&mut editor, "src/j.rs");
    editor.choose(EXPLORER, at, "file.delete", Some("src/j.rs".into()), None);
    assert!(!shown_in(&editor, EXPLORER).contains("j.rs"), "the file is still listed");
    let _ = std::fs::remove_dir_all(&root);
}

/// A new file, named in the menu, appears in the explorer.
#[test]
fn a_new_file_is_made_from_the_explorer() {
    let (root, mut editor) = opened();
    editor.choose(EXPLORER, 0, "file.new", None, Some("src/new.rs".into()));
    assert!(shown_in(&editor, EXPLORER).contains("new.rs"), "a new file is not shown where it was made");
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+S writes the file; Ctrl+Z takes the typing back and the document shows
/// it, with the focus left where it was.
#[test]
fn save_and_undo_by_the_keys_every_editor_has() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    editor.key("A");
    editor.chord("C-s");
    let on_disk = std::fs::read_to_string(root.join("src/i.rs")).unwrap();
    assert_eq!(on_disk, "Aone\ntwo\nthree\n", "Ctrl+S did not write the file");

    editor.chord("C-z");
    assert_eq!(editor.screen.focus, DOCUMENT, "undo pulled the focus away");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntwo\nthree\n", "undo did not reach the document");
    editor.chord("C-y");
    assert_eq!(shown_in(&editor, DOCUMENT), "Aone\ntwo\nthree\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// Home, End, Delete and pasting.
#[test]
fn moving_by_line_deleting_forwards_and_pasting() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 5);
    editor.chord("End");
    assert_eq!(editor.line_and_column(), (1, 3));
    editor.chord("Home");
    assert_eq!(editor.line_and_column(), (1, 0));
    editor.chord("Delete");
    editor.paste("T");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\nTwo\nthree\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// A tab names its buffer by title, and clicking it shows that buffer — the
/// row's text is a number and a name, never the buffer's identity, so the
/// click is resolved by row.
///
/// @tests REQ-SCREEN.strip_is_the_opened_set
#[test]
fn clicking_a_tab_shows_its_buffer() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    let strip = editor.strip();
    assert!(strip.text.contains("i.rs") && !strip.text.contains("file:"), "{}", strip.text);
    let at = strip.text[..strip.text.find("i.rs").unwrap()].chars().count();
    editor.act_in_bar(false, "screen.show", at);
    assert_eq!(editor.screen.focus, DOCUMENT);
    assert!(shown_in(&editor, DOCUMENT).starts_with("one\n"), "the tab did not show its file");
    let _ = std::fs::remove_dir_all(&root);
}

/// Enter keeps the line's indentation, one level deeper after a line that
/// opens a block; Tab goes to the next level.
#[test]
fn enter_keeps_the_indentation() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    for key in ["f", "n", "{", "Enter", "x", "Enter", "y", "Enter", "Tab", "z", "Enter"] {
        editor.key(key);
    }
    let text = shown_in(&editor, DOCUMENT);
    assert!(text.starts_with("fn{\n    x\n    y\n        z\n        one\n"), "{text:?}");
    editor.key("Backspace");
    let text = shown_in(&editor, DOCUMENT);
    assert!(text.contains("z\n    one\n"), "Backspace took back more or less than a level: {text:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+arrows move by word, Ctrl+Backspace deletes one, and Ctrl+/ comments
/// the line out and back in.
#[test]
fn words_and_comments_by_the_usual_chords() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    for key in ["l", "e", "t", " ", "a", "b", "=", "1", ";"] {
        editor.key(key);
    }
    editor.chord("C-Left");
    assert_eq!(editor.offset, 7, "Ctrl+Left stops at the start of `1`");
    editor.chord("C-Left");
    editor.chord("C-Right");
    assert_eq!(editor.offset, 6, "Ctrl+Right stops after `ab`");
    editor.chord("C-Backspace");
    assert!(shown_in(&editor, DOCUMENT).starts_with("let =1;one\n"), "{:?}", shown_in(&editor, DOCUMENT));
    editor.chord("C-/");
    assert!(shown_in(&editor, DOCUMENT).starts_with("// let =1;one\n"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.offset, 7, "the cursor stays on the same character");
    editor.chord("C-/");
    assert!(shown_in(&editor, DOCUMENT).starts_with("let =1;one\n"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.offset, 4);
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+Z takes back a word typed, not a letter; Ctrl+Y gives it back whole.
#[test]
fn undo_takes_back_a_word_at_a_time() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    for key in ["a", "b", " ", "c", "d"] {
        editor.key(key);
    }
    editor.chord("C-z");
    assert!(shown_in(&editor, DOCUMENT).starts_with("ab one\n"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.offset, 3, "the cursor goes to where the word was");
    editor.chord("C-z");
    assert!(shown_in(&editor, DOCUMENT).starts_with("one\n"), "{:?}", shown_in(&editor, DOCUMENT));
    editor.chord("C-y");
    editor.chord("C-y");
    assert!(shown_in(&editor, DOCUMENT).starts_with("ab cdone\n"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.offset, 5, "the cursor goes to the end of what came back");
    for _ in 0..3 {
        editor.key("Backspace");
    }
    editor.chord("C-z");
    assert!(shown_in(&editor, DOCUMENT).starts_with("ab cdone\n"), "{:?}", shown_in(&editor, DOCUMENT));
    let _ = std::fs::remove_dir_all(&root);
}

/// A file changed on disk by something else is shown as it now is, unless it
/// was edited here too: then the edit is kept and the person is told.
#[test]
fn a_change_on_disk_is_followed() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(!editor.follow_disk(), "nothing changed, yet something was taken in");
    std::fs::write(root.join("src/i.rs"), "changed\n").unwrap();
    std::fs::write(root.join("src/new.rs"), "fresh\n").unwrap();
    assert!(editor.follow_disk());
    assert_eq!(shown_in(&editor, DOCUMENT), "changed\n");
    assert!(editor.unsaved().is_empty(), "what is on disk is not unsaved: {:?}", editor.unsaved());
    assert!(editor.workspace().files.contains_key("src/new.rs"));
    editor.chord("C-z");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntwo\nthree\n", "Ctrl+Z did not take the reload back");
    editor.chord("C-y");

    editor.place(DOCUMENT, 0);
    editor.key("X");
    std::fs::write(root.join("src/i.rs"), "again\n").unwrap();
    assert!(editor.follow_disk());
    assert_eq!(shown_in(&editor, DOCUMENT), "Xchanged\n", "the edit here was lost");
    let said = plain_text(editor.status.clone()).join("\n");
    assert!(said.contains("src/i.rs") && said.contains("overwrites"), "{said}");
    assert!(!editor.follow_disk(), "the same change was reported twice");
    let _ = std::fs::remove_dir_all(&root);
}

/// Alt+arrows move a line, Shift+Alt+Down copies it, Ctrl+Shift+K removes it,
/// Shift+Tab takes a level of indentation off it — the cursor going with it.
#[test]
fn lines_move_copy_and_go_by_the_usual_chords() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 1);
    editor.chord("A-Down");
    assert_eq!(shown_in(&editor, DOCUMENT), "two\none\nthree\n");
    assert_eq!(editor.line_and_column(), (1, 1));
    editor.chord("A-Up");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntwo\nthree\n");
    assert_eq!(editor.line_and_column(), (0, 1));
    editor.chord("A-S-Down");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\none\ntwo\nthree\n");
    assert_eq!(editor.line_and_column(), (1, 1));
    editor.chord("C-S-k");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntwo\nthree\n");
    editor.key("Tab");
    editor.key("Tab");
    assert!(shown_in(&editor, DOCUMENT).starts_with("one\n        two\n"), "{:?}", shown_in(&editor, DOCUMENT));
    editor.chord("S-Tab");
    assert!(shown_in(&editor, DOCUMENT).starts_with("one\n    two\n"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.line_and_column(), (1, 4));
    let _ = std::fs::remove_dir_all(&root);
}

/// Tab, Shift+Tab and Ctrl+/ over a selection change every line it touches,
/// as one change.
#[test]
fn a_selection_is_indented_and_commented_line_by_line() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.over_lines(DOCUMENT, 1, 6, "Tab");
    assert_eq!(shown_in(&editor, DOCUMENT), "    one\n    two\nthree\n");
    editor.over_lines(DOCUMENT, 0, 14, "C-/");
    assert_eq!(shown_in(&editor, DOCUMENT), "    // one\n    // two\nthree\n");
    editor.over_lines(DOCUMENT, 0, 20, "C-/");
    assert_eq!(shown_in(&editor, DOCUMENT), "    one\n    two\nthree\n");
    editor.chord("C-z");
    assert_eq!(shown_in(&editor, DOCUMENT), "    // one\n    // two\nthree\n", "one change, one undo");
    editor.chord("C-y");
    editor.over_lines(DOCUMENT, 0, 16, "S-Tab");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntwo\nthree\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// The welcome page lists the folders opened before, and clicking one opens
/// it as the project.
///
/// @tests REQ-SHOW.recent_reopens
#[test]
fn a_recent_folder_reopens_from_the_welcome_page() {
    let config = project();
    std::env::set_var("XDG_CONFIG_HOME", &config);
    let (first, mut editor) = opened();
    editor.track_recent();
    let (second, mut editor) = opened();
    editor.track_recent();
    let first_named = tracelean_editor::recall::named(&first);
    let welcome = shown_in(&editor, DOCUMENT);
    let at = welcome.find(&first_named).unwrap_or_else(|| panic!("{first_named} is not listed:\n{welcome}"));
    assert!(!welcome.contains(&tracelean_editor::recall::named(&second)), "the open folder is listed as another");
    editor.choose(DOCUMENT, welcome[..at].chars().count() + 1, "file.open", None, None);
    assert_eq!(tracelean_editor::recall::named(&editor.root), first_named, "the folder did not open");
    for dir in [config, first, second] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The selection is the editor's: Shift with a movement makes it, it reaches
/// past the window, typing replaces it, Ctrl+C/X copy and cut it, Ctrl+A
/// takes everything, a plain arrow lets it go.
#[test]
fn the_selection_is_kept_by_the_editor() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    editor.chord("S-End");
    assert_eq!(editor.selected().as_deref(), Some("line 1"));
    for _ in 0..30 {
        editor.chord("S-Down");
    }
    assert!(editor.selected().unwrap().ends_with("line 30\nline 3"), "the selection stopped at the window");
    assert_eq!(editor.selection_shown().map(|(_, end)| end > 0), Some(true));
    editor.chord("C-c");
    assert!(editor.clipboard.as_deref().unwrap().starts_with("line 1\nline 2\n"));
    editor.key("X");
    assert!(shown_in(&editor, DOCUMENT).starts_with("X1\nline 32\n"), "{:?}", &shown_in(&editor, DOCUMENT)[..20]);
    assert_eq!(editor.selection(), None);
    editor.chord("S-Left");
    editor.chord("C-x");
    assert_eq!(editor.clipboard.as_deref(), Some("X"));
    assert!(shown_in(&editor, DOCUMENT).starts_with("1\nline 32\n"));
    editor.chord("C-a");
    assert_eq!(editor.selected().map(|s| s.lines().count()), Some(30));
    editor.key("Right");
    assert_eq!(editor.selection(), None, "an arrow kept the selection");
    // Drawn with the pointer, it is the same selection.
    editor.top = 0;
    editor.select(DOCUMENT, 2, 6);
    assert_eq!(editor.selected().as_deref(), Some("line"));
    editor.key("Backspace");
    assert!(shown_in(&editor, DOCUMENT).starts_with("1\n 32\n"), "{:?}", &shown_in(&editor, DOCUMENT)[..10]);
    let _ = std::fs::remove_dir_all(&root);
}

/// An opening bracket brings its closer; typing the closer steps over it;
/// Enter between them opens an indented line; Backspace removes both.
#[test]
fn brackets_come_in_pairs() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    editor.key("Enter");
    editor.key("Up");
    for key in ["f", "(", "x", ")"] {
        editor.key(key);
    }
    assert!(shown_in(&editor, DOCUMENT).starts_with("f(x)\none"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.offset, 4, "`)` stepped over the closer");
    editor.key("{");
    editor.key("Enter");
    assert!(shown_in(&editor, DOCUMENT).starts_with("f(x){\n    \n}\none"), "{:?}", shown_in(&editor, DOCUMENT));
    assert_eq!(editor.line_and_column(), (1, 4));
    editor.key("[");
    editor.key("Backspace");
    assert!(shown_in(&editor, DOCUMENT).starts_with("f(x){\n    \n}\none"), "{:?}", shown_in(&editor, DOCUMENT));
    // Quotes too, in code: closed, stepped over, removed as a pair; after a
    // word, a quote is just a quote.
    for key in ["\"", "a", "\""] {
        editor.key(key);
    }
    assert!(shown_in(&editor, DOCUMENT).starts_with("f(x){\n    \"a\"\n}"), "{:?}", shown_in(&editor, DOCUMENT));
    editor.key("\"");
    editor.key("Backspace");
    assert!(shown_in(&editor, DOCUMENT).starts_with("f(x){\n    \"a\"\n}"), "{:?}", shown_in(&editor, DOCUMENT));
    editor.key("x");
    editor.key("\"");
    assert!(shown_in(&editor, DOCUMENT).starts_with("f(x){\n    \"a\"x\"\n}"), "{:?}", shown_in(&editor, DOCUMENT));
    let _ = std::fs::remove_dir_all(&root);
}

/// Beside a bracket, the editor names it and its match for the window to mark.
#[test]
fn a_bracket_and_its_match_are_found() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    for key in ["f", "(", "a", "[", "b", "]", ")"] {
        editor.key(key);
    }
    assert_eq!(editor.bracket_pair(), Some((6, 1)), "after `)`");
    editor.offset = 1;
    assert_eq!(editor.bracket_pair(), Some((1, 6)), "on `(`");
    editor.offset = 4;
    assert_eq!(editor.bracket_pair(), Some((3, 5)), "after `[`");
    editor.offset = 9;
    assert_eq!(editor.bracket_pair(), None, "away from any");
    let _ = std::fs::remove_dir_all(&root);
}

/// A draft requirement opens with an Approve button, which makes its status
/// approved as an edit Ctrl+Z takes back.
#[test]
fn a_draft_requirement_is_approved_from_its_view() {
    let root = project();
    std::fs::create_dir_all(root.join("reqs")).unwrap();
    std::fs::write(
        root.join("reqs/REQ-D.md"),
        "---\nid: REQ-D\ntitle: Drafted\nstatus: draft\nclauses:\n  one: It shall.\n---\n\n# REQ-D\n",
    )
    .unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 30 };
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "requirement REQ-D".into() },
    });
    let shown = shown_in(&editor, DOCUMENT);
    let at = shown.find("[ Approve ]").unwrap_or_else(|| panic!("no Approve button:\n{shown}"));
    editor.choose(DOCUMENT, shown[..at].chars().count() + 2, "trace.approve", None, None);
    let text = editor.workspace().files.get("reqs/REQ-D.md").cloned().unwrap();
    assert!(text.contains("\nstatus: approved\n"), "{text}");
    editor.chord("C-z");
    let text = editor.workspace().files.get("reqs/REQ-D.md").cloned().unwrap();
    assert!(text.contains("\nstatus: draft\n"), "{text}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A tree row's right-click menu copies the row's full path.
#[test]
fn a_tree_row_copies_its_path() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    let offer = editor
        .offers_at(EXPLORER, at)
        .into_iter()
        .find(|o| o.action == "file.copy_path")
        .expect("the row offers its path");
    editor.choose(EXPLORER, at, &offer.action, offer.target.clone(), None);
    assert_eq!(editor.clipboard.as_deref(), Some("src/i.rs"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Enter on a row does what a click there does: the explorer's opens the file.
#[test]
fn enter_opens_what_the_cursor_is_on() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.place(EXPLORER, at);
    editor.key("Enter");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/i.rs"),
        "{:?}", editor.buffer().kind);
    assert_eq!(editor.screen.focus, DOCUMENT);
    let _ = std::fs::remove_dir_all(&root);
}

/// Home goes to the first character that is not blank, then the line's
/// start; Ctrl+L selects the line, and again the next one too.
#[test]
fn home_is_smart_and_ctrl_l_selects_lines() {
    let (root, mut editor) = opened();
    std::fs::write(root.join("src/i.rs"), "fn a() {\n    one\n    two\n}\n").unwrap();
    editor.tick();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 16);
    assert_eq!(editor.line_and_column(), (1, 7));
    editor.chord("Home");
    assert_eq!(editor.line_and_column(), (1, 4), "to the first character");
    editor.chord("Home");
    assert_eq!(editor.line_and_column(), (1, 0), "then the start");
    editor.chord("Home");
    assert_eq!(editor.line_and_column(), (1, 4), "and back");
    editor.chord("C-l");
    assert_eq!(editor.selected().as_deref(), Some("    one\n"));
    editor.chord("C-l");
    assert_eq!(editor.selected().as_deref(), Some("    one\n    two\n"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+D selects the word at the cursor, and again the next place holding it.
#[test]
fn ctrl_d_selects_the_word_then_the_next_one() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 2);
    editor.chord("C-d");
    assert_eq!(editor.selected().as_deref(), Some("line"));
    assert_eq!(editor.line_and_column().0, 0);
    editor.chord("C-d");
    assert_eq!(editor.selected().as_deref(), Some("line"));
    assert_eq!(editor.line_and_column(), (1, 4), "the next one");
    let _ = std::fs::remove_dir_all(&root);
}

/// The explorer marks the file the document shows, whichever way it came
/// to be shown — a tab, Ctrl+Tab.
#[test]
fn the_explorer_follows_the_shown_file() {
    let (root, mut editor) = opened();
    for path in ["src/i.rs", "src/long.rs"] {
        let at = row_of(&mut editor, path);
        editor.choose(EXPLORER, at, "file.open", None, None);
    }
    let marked = |editor: &Editor| {
        let tree = editor.laid_out(editor.region).into_iter().find(|p| p.pane == EXPLORER).expect("the tree").buffer;
        tree.spans
            .iter()
            .filter(|s| s.role == tracelean_core::surface::view::Role::Heading && s.start > 0)
            .map(|s| tree.text.chars().skip(s.start).take(s.stop - s.start).collect::<String>())
            .collect::<Vec<_>>()
    };
    assert_eq!(marked(&editor), vec!["long.rs".to_string()]);
    editor.chord("C-Tab");
    assert_eq!(marked(&editor), vec!["i.rs".to_string()], "the tree still marks the file left");
    let _ = std::fs::remove_dir_all(&root);
}

/// A file with unsaved edits is marked in the explorer, and saving clears it.
#[test]
fn the_explorer_marks_unsaved_files() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(!shown_in(&editor, EXPLORER).contains('✎'));
    editor.key("i");
    editor.key("x");
    assert!(shown_in(&editor, EXPLORER).contains("i.rs  ✎"), "{}", shown_in(&editor, EXPLORER));
    editor.chord("C-s");
    assert!(!shown_in(&editor, EXPLORER).contains('✎'), "{}", shown_in(&editor, EXPLORER));
    let _ = std::fs::remove_dir_all(&root);
}

/// The name under the cursor is found, whole, wherever the window shows it.
#[test]
fn the_name_under_the_cursor_is_found_in_the_window() {
    let (root, mut editor) = opened();
    std::fs::write(root.join("src/i.rs"), "let ab = 1;\nlet abc = ab + ab;\n").unwrap();
    editor.tick();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 5);
    assert_eq!(editor.occurrences_shown(), Some((2, vec![4, 22, 27])), "not inside `abc`");
    editor.chord("C-d");
    assert_eq!(editor.occurrences_shown(), None, "nothing while a selection stands");
    let _ = std::fs::remove_dir_all(&root);
}

/// What a file declares is listed by name, and opening one goes to its line.
#[test]
fn a_symbol_of_the_file_is_found_and_gone_to() {
    let (root, mut editor) = opened();
    std::fs::write(root.join("src/i.rs"), "use x;\n\npub fn alpha() {}\n\nstruct Beta;\nfn alphabet() {}\n").unwrap();
    editor.tick();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    let all: Vec<String> = editor.symbols("").into_iter().map(|(_, label)| label).collect();
    assert_eq!(all, vec!["alpha  :3", "Beta  :5", "alphabet  :6"]);
    let found = editor.symbols("ALPHAB");
    assert_eq!(found, vec![("src/i.rs:6".to_string(), "alphabet  :6".to_string())]);
    editor.choose(EXPLORER, 0, "file.open", Some(found[0].0.clone()), None);
    assert_eq!(editor.line_and_column(), (5, 0));
    let _ = std::fs::remove_dir_all(&root);
}

/// The palette finds an action by a few words of what it is called, and
/// doing it from there does what its key does.
#[test]
fn the_palette_finds_an_action_by_its_words() {
    let (root, mut editor) = opened();
    let found = editor.palette("histo tree");
    assert!(found.iter().any(|(action, _)| action == "history.tree"), "{found:?}");
    assert!(found.iter().all(|(_, said)| said.to_lowercase().contains("hist") || said.contains("tree")), "{found:?}");
    assert!(editor.palette("no such words at all").is_empty());
    assert_eq!(editor.palette("").len(), tracelean_core::surface::offer::described(&editor.keymap).len());
    editor.act_here("history.tree");
    assert!(matches!(editor.buffer().kind, BufferKind::Record { ref title } if title == "history"),
        "{:?}", editor.buffer().kind);
    let _ = std::fs::remove_dir_all(&root);
}

/// The checker's findings are counted between keys, and counted again once
/// a save changed the tree.
#[test]
fn the_findings_are_counted_between_keys() {
    let (root, mut editor) = opened();
    assert_eq!(editor.problems(), None, "counted on a key rather than between");
    editor.tick();
    let before = editor.problems().expect("counted");
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.key("i");
    for c in "// @implements REQ-NOWHERE.x\n".chars() {
        editor.key(&c.to_string());
    }
    editor.chord("C-s");
    assert!(editor.tick(), "a new count is something to redraw");
    assert!(editor.problems().expect("counted") > before, "{:?} after {before}", editor.problems());
    assert!(!editor.tick(), "nothing changed since");
    let _ = std::fs::remove_dir_all(&root);
}

/// Replace swaps every match in the file at once, and one undo takes it all
/// back.
#[test]
fn replace_swaps_every_match_as_one_change() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.replace_all("line", "row");
    let text = editor.buffer().text;
    assert!(!text.contains("line") && text.starts_with("row 1\nrow 2\n"), "{text}");
    assert!(editor.status.text.contains("60 ×"), "{}", editor.status.text);
    editor.chord("C-z");
    assert!(editor.buffer().text.starts_with("line 1\nline 2\n"), "one undo took it all back");
    editor.replace_all("nowhere", "x");
    assert!(editor.status.text.contains("not in this buffer"), "{}", editor.status.text);
    let _ = std::fs::remove_dir_all(&root);
}

/// Alt+Left goes back to where the document was before a jump — another
/// file, a line — and Alt+Right forward again.
#[test]
fn alt_left_goes_back_where_a_jump_left_from() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.chord("C-End");
    editor.chord("PageUp");
    let before = editor.line_and_column();
    // A link to another file's line, as a requirement's claims are.
    editor.perform(tracelean_core::surface::act::Intent::Display { what: BufferKind::File { path: "src/i.rs:2".into() } });
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/i.rs"));
    assert_eq!(editor.line_and_column().0, 1);
    editor.chord("A-Left");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/long.rs"),
        "{:?}", editor.buffer().kind);
    assert_eq!(editor.line_and_column(), before, "back to the same place");
    editor.chord("A-Right");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/i.rs"));
    assert_eq!(editor.line_and_column().0, 1, "forward to the line the link went to");
    editor.chord("A-Right");
    assert!(editor.status.text.contains("nothing to go forward"), "{}", editor.status.text);
    let _ = std::fs::remove_dir_all(&root);
}

/// Left and Right walk the explorer as a file tree: into a folder, back to
/// it, closed, and on a top-level row every folder closes.
#[test]
fn left_and_right_walk_the_tree() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.place(EXPLORER, at);
    editor.key("Left");
    let on = |editor: &Editor| {
        let text = editor.buffer().text;
        let line: String = text.chars().skip(editor.offset).take_while(|c| *c != '\n').collect();
        line
    };
    assert_eq!(on(&editor), "src", "Left goes to the folder holding the row");
    editor.key("Left");
    assert!(shown_in(&editor, EXPLORER).contains("▸ src"), "{}", shown_in(&editor, EXPLORER));
    editor.key("Right");
    assert!(shown_in(&editor, EXPLORER).contains("▾ src"));
    editor.key("Right");
    assert_ne!(on(&editor), "src", "Right enters an open folder");
    editor.key("Left");
    editor.key("Left");
    editor.key("Left");
    assert!(!shown_in(&editor, EXPLORER).contains('▾'), "{}", shown_in(&editor, EXPLORER));
    let _ = std::fs::remove_dir_all(&root);
}

/// F1 lists every key — the chords and each bound action — where documents
/// are read.
#[test]
fn f1_lists_every_key() {
    let (root, mut editor) = opened();
    editor.chord("F1");
    assert_eq!(editor.screen.focus, DOCUMENT);
    let shown = shown_in(&editor, DOCUMENT);
    assert!(shown.contains("Ctrl+S") && shown.contains("Alt+Up"), "{shown}");
    assert!(shown.contains("Space f o"), "the keymap's own keys are missing:\n{shown}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+Tab and Ctrl+Shift+Tab go through the tabs of the pane being read.
#[test]
fn ctrl_tab_cycles_the_documents() {
    let (root, mut editor) = opened();
    for path in ["src/i.rs", "src/long.rs"] {
        editor.choose(EXPLORER, 0, "file.open", Some(path.to_string()), None);
    }
    editor.chord("C-S-Tab");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/i.rs"));
    assert_eq!(editor.screen.focus, DOCUMENT);
    editor.chord("C-Tab");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/long.rs"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Going back to a tab goes back to where the cursor was in it.
#[test]
fn a_tab_keeps_its_cursor() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    editor.key("Down");
    let was = editor.line_and_column();
    assert_eq!(was.0, 1);
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    let strip = editor.strip();
    let at = strip.text[..strip.text.find("i.rs").unwrap()].chars().count();
    editor.act_in_bar(false, "screen.show", at);
    assert_eq!(editor.line_and_column(), was);
    let _ = std::fs::remove_dir_all(&root);
}

/// A requirement opens in the document with each clause and what claims it,
/// and clicking a claim opens the claiming file at its line.
///
/// @tests REQ-SHOW.requirement_opened
#[test]
fn a_requirement_opens_and_its_claims_open_the_code() {
    let root = project();
    std::fs::create_dir_all(root.join("reqs")).unwrap();
    std::fs::write(
        root.join("reqs/REQ-X.md"),
        "---\nid: REQ-X\ntitle: Something holds\nstatus: approved\nclauses:\n  holds: It shall hold.\n  other: Nobody does this.\n---\n\n# REQ-X\n",
    )
    .unwrap();
    std::fs::write(root.join("src/x.rs"), "fn a() {}\n\n/// @implements REQ-X.holds\nfn holds() {}\n").unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 30 };
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "requirement REQ-X".into() },
    });
    assert_eq!(editor.screen.focus, DOCUMENT, "a requirement did not open where documents are read");
    let shown = shown_in(&editor, DOCUMENT);
    assert!(shown.contains("holds") && shown.contains("nothing claims it yet"), "{shown}");
    let link = shown.find("src/x.rs:").unwrap_or_else(|| panic!("no claim is linked:\n{shown}"));
    let at = shown[..link].chars().count();
    editor.choose(DOCUMENT, at, "file.open", None, None);
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/x.rs"));
    assert!(editor.line_and_column().0 >= 2, "the file did not open at the claim: {:?}", editor.line_and_column());
    // What a name says, for a pointer resting on it.
    assert_eq!(editor.requirement_text("REQ-X.holds").as_deref(), Some("REQ-X.holds: It shall hold."));
    assert_eq!(editor.requirement_text("REQ-X").as_deref(), Some("REQ-X: Something holds"));
    assert_eq!(editor.requirement_text("REQ-NONE"), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// A clause written as a block shows its text, and its narrowings indented
/// under it — read from the document, not counted as clauses.
///
/// @tests REQ-REQDOC.narrowings_nest
#[test]
fn a_requirement_shows_narrowings_under_their_clause() {
    let root = project();
    std::fs::create_dir_all(root.join("reqs")).unwrap();
    std::fs::write(
        root.join("reqs/REQ-X.md"),
        "---\nid: REQ-X\ntitle: Something holds\nstatus: approved\nclauses:\n  holds:\n    text: It shall hold.\n    empty: With nothing, it holds.\n  other: Nobody does this.\n---\n\n# REQ-X\n",
    )
    .unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 30 };
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "requirement REQ-X".into() },
    });
    let shown = shown_in(&editor, DOCUMENT);
    assert!(shown.contains("  It shall hold.\n    empty: With nothing, it holds.\n"), "{shown}");
    assert!(shown.contains("implements 0/2"), "a narrowing was counted as a clause:\n{shown}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A clause a model claims offers its judge's prompt, which opens as a buffer
/// and is copied whole.
///
/// @tests REQ-JUDGE.prompt_exported
#[test]
fn a_modelled_clause_opens_its_judge_prompt_to_copy() {
    let root = project();
    std::fs::create_dir_all(root.join("reqs")).unwrap();
    std::fs::create_dir_all(root.join("formal")).unwrap();
    std::fs::write(
        root.join("reqs/REQ-X.md"),
        "---\nid: REQ-X\ntitle: Something holds\nstatus: approved\nclauses:\n  holds: It shall hold.\n---\n\n# REQ-X\n",
    )
    .unwrap();
    std::fs::write(root.join("formal/X.lean"), "/-- @models REQ-X.holds -/\ndef holds : Nat := 1\n").unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 30 };
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "requirement REQ-X".into() },
    });
    let shown = shown_in(&editor, DOCUMENT);
    let name = shown.find("REQ-X.holds").unwrap_or_else(|| panic!("no judge link:\n{shown}"));
    editor.choose(DOCUMENT, shown[..name].chars().count(), "trace.judge", None, None);
    let prompt = shown_in(&editor, DOCUMENT);
    assert!(prompt.contains("It shall hold.") && prompt.contains("def holds"), "{prompt}");
    editor.perform(tracelean_core::surface::act::dispatch("file.copy".into(), editor.focus(), editor.workspace(), Vec::new()));
    assert_eq!(editor.clipboard.as_deref(), Some(editor.buffer().text.as_str()));
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+F finds forwards from the cursor and wraps; F3 finds the next one,
/// Shift+F3 the one before.
#[test]
fn find_moves_the_cursor_to_each_match_in_turn() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/long.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    // Each match is selected, the cursor at its end, so typing replaces it.
    editor.find("line 2", true);
    assert_eq!(editor.line_and_column(), (1, 6), "the first match is line 2");
    assert_eq!(editor.selected().as_deref(), Some("line 2"));
    editor.chord("F3");
    assert_eq!(editor.line_and_column(), (19, 6), "the next match is line 20");
    editor.chord("S-F3");
    assert_eq!(editor.line_and_column(), (1, 6), "back to line 2");
    editor.find("nowhere", true);
    assert_eq!(editor.line_and_column(), (1, 6), "a miss moved the cursor");
    // What was found stays selected in the document; a folder clicked in
    // the explorer still closes.
    editor.key("Escape");
    let at = find_row(&editor, "src").expect("src is shown");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(shown_in(&editor, EXPLORER).contains("▸ src"), "{}", shown_in(&editor, EXPLORER));
    let _ = std::fs::remove_dir_all(&root);
}

/// Another folder is opened from the explorer's menu, and becomes the tree.
#[test]
fn another_folder_is_opened_from_the_explorer() {
    let (root, mut editor) = opened();
    let other = project();
    std::fs::write(other.join("src/elsewhere.rs"), "fn e() {}\n").unwrap();
    let offered = editor.offers_at(EXPLORER, 0);
    let open = offered.iter().find(|o| o.label.starts_with("Open another folder")).expect("offered");
    assert!(open.asks.is_some());
    editor.choose(EXPLORER, 0, &open.action, open.target.clone(), Some(other.display().to_string()));
    assert_eq!(editor.root, other);
    assert!(shown_in(&editor, EXPLORER).contains("elsewhere.rs"), "{}", shown_in(&editor, EXPLORER));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}

/// A requirement is made from the requirements panel's menu, from a template,
/// and opens to be written.
#[test]
fn a_new_requirement_is_made_from_the_panel() {
    let (root, mut editor) = opened();
    let side = tracelean_core::surface::screen::SIDE;
    let offered = editor.offers_at(side, 0);
    let new = offered.iter().find(|o| o.action == "trace.new_requirement").expect("offered");
    assert!(new.asks.is_some());
    editor.choose(side, 0, "trace.new_requirement", None, Some("REQ-NEW".into()));
    let written = editor.workspace().files.get("reqs/REQ-NEW.md").cloned().expect("the document was made");
    assert!(written.starts_with("---\nid: REQ-NEW\n"), "{written}");
    editor.chord("C-s");
    let index = tracelean_core::trace::index::build(&root);
    assert!(index.requirements.contains_key("REQ-NEW"), "the template is not a requirement the index reads");
    let _ = std::fs::remove_dir_all(&root);
}

/// The history shows each change, and clicking one goes back to it.
///
/// @tests REQ-UNDO.tree_is_shown
#[test]
fn a_point_in_the_history_is_gone_back_to_by_clicking_it() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    editor.key("A");
    editor.key("B");
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "history".into() },
    });
    let pane = editor.screen.focus.clone();
    let shown = shown_in(&editor, &pane);
    assert!(shown.contains("typed \"A\" in src/i.rs"), "{shown}");
    let first = shown.lines().find(|l| l.contains("\"A\"")).unwrap();
    let name = first.split_whitespace().find(|w| w.starts_with('#')).unwrap().to_string();
    let at = shown[..shown.find(&format!("{name}  typed \"A\"")).unwrap()].chars().count();
    editor.choose(&pane, at, "history.jump", None, None);
    assert_eq!(editor.workspace().files.get("src/i.rs").map(String::as_str), Some("Aone\ntwo\nthree\n"));
    let _ = std::fs::remove_dir_all(&root);
}

/// A selection is cut as one change: typing replaces it, and one undo brings
/// it back.
#[test]
fn a_selection_is_cut_and_typed_over() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.cut(DOCUMENT, 4, 7);
    editor.key("X");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\nX\nthree\n");
    editor.chord("C-z");
    editor.chord("C-z");
    assert_eq!(shown_in(&editor, DOCUMENT), "one\ntwo\nthree\n", "the cut was not one change");
    let _ = std::fs::remove_dir_all(&root);
}

/// A tab says when its file has changes not yet saved, until it is saved.
#[test]
fn an_unsaved_file_is_marked_on_its_tab() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(!editor.strip().text.contains("i.rs ●"));
    editor.place(DOCUMENT, 0);
    editor.key("A");
    assert!(editor.strip().text.contains("i.rs ●"), "{}", editor.strip().text);
    editor.chord("C-s");
    assert!(!editor.strip().text.contains("●"), "{}", editor.strip().text);
    let _ = std::fs::remove_dir_all(&root);
}

/// Closing with unsaved work is refused once, saying which files; asking again
/// closes. With nothing unsaved it closes at once.
#[test]
fn closing_with_unsaved_work_is_refused_once() {
    let (root, mut editor) = opened();
    assert!(editor.unsaved().is_empty());
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 0);
    editor.key("A");
    assert_eq!(editor.unsaved(), vec!["src/i.rs".to_string()]);
    assert!(!editor.may_close());
    let said = plain_text(editor.status.clone()).join("\n");
    assert!(said.contains("src/i.rs") && said.contains("close again"), "{said}");
    assert!(editor.may_close());
    let _ = std::fs::remove_dir_all(&root);
}

/// Reopening a project brings back its tabs, its open folders and the file in
/// front; a file removed since is left out.
#[test]
fn reopening_brings_back_what_was_open() {
    let (root, mut editor) = opened();
    for path in ["src/i.rs", "src/long.rs"] {
        editor.quick_open(path);
        editor.choose(EXPLORER, 0, "file.open", Some(path.to_string()), None);
    }
    editor.remember();
    let kept = editor.recall();
    assert_eq!(kept.shown.as_deref(), Some("src/long.rs"));
    std::fs::remove_file(root.join("src/i.rs")).unwrap();
    let again = Editor::open(root.clone(), keymap());
    let back = again.recall();
    assert_eq!(back.files, vec!["src/long.rs".to_string()]);
    assert_eq!(back.shown.as_deref(), Some("src/long.rs"));
    assert_eq!(back.folders, kept.folders);
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+P's list is the project's files that match, and choosing one opens it.
#[test]
fn quick_open_finds_a_file_by_a_few_letters() {
    let (root, mut editor) = opened();
    let found = editor.quick_open("lng");
    assert_eq!(found.first().map(String::as_str), Some("src/long.rs"), "{found:?}");
    editor.choose(EXPLORER, 0, "file.open", Some(found[0].clone()), None);
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/long.rs"));
    // With nothing typed, the file just left comes first.
    editor.choose(EXPLORER, 0, "file.open", Some("src/i.rs".into()), None);
    assert_eq!(editor.quick_open(""), vec!["src/long.rs".to_string()]);
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+P takes a line too: `lng:5` opens the match at line 5, `:30` goes to
/// line 30 of the file in front.
#[test]
fn quick_open_goes_to_a_line() {
    let (root, mut editor) = opened();
    let found = editor.quick_open("lng:5");
    assert_eq!(found.first().map(String::as_str), Some("src/long.rs:5"), "{found:?}");
    editor.choose(EXPLORER, 0, "file.open", Some(found[0].clone()), None);
    assert_eq!(editor.line_and_column().0, 4);
    let found = editor.quick_open(":30");
    assert_eq!(found, vec!["src/long.rs:30".to_string()]);
    editor.choose(EXPLORER, 0, "file.open", Some(found[0].clone()), None);
    assert_eq!(editor.line_and_column().0, 29);
    let _ = std::fs::remove_dir_all(&root);
}

/// Ctrl+W closes the buffer the focused pane shows.
#[test]
fn ctrl_w_closes_the_focused_buffer() {
    let (root, mut editor) = opened();
    let at = row_of(&mut editor, "src/i.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    assert!(editor.strip().text.contains("i.rs"));
    editor.chord("C-w");
    assert!(!editor.strip().text.contains("i.rs"), "{}", editor.strip().text);
    let _ = std::fs::remove_dir_all(&root);
}

/// `.` lists what can be done where the cursor is — the keyboard's right-click
/// — and a key picks from it.
///
/// @tests REQ-ACT.everything_is_offered
#[test]
fn dot_offers_what_can_be_done_here_and_a_key_picks() {
    let (root, mut editor) = opened();
    editor.focus_pane(EXPLORER);
    // Down to `i.rs`, under the project's name and `src`.
    editor.key("Down");
    editor.key("Down");
    editor.key(".");
    let menu = plain_text(editor.menu.clone().expect("a list is up")).join("\n");
    assert!(menu.contains("1  Open src/i.rs"), "{menu}");
    editor.key("1");
    assert!(editor.menu.is_none(), "the list stayed up");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/i.rs"));
    let _ = std::fs::remove_dir_all(&root);
}

/// A report that answers with a command offers to copy just that command.
#[test]
fn a_command_in_a_report_is_copied_by_its_line() {
    let (root, mut editor) = opened();
    editor.perform(tracelean_core::surface::act::Intent::Display {
        what: BufferKind::Record { title: "drt run".into() },
    });
    let pane = editor.screen.focus.clone();
    let shown = shown_in(&editor, &pane);
    let at = shown[..shown.find("cargo test").expect(&shown)].chars().count();
    assert!(editor.offers_at(&pane, at).iter().any(|o| o.action == "file.copy_line"));
    editor.choose(&pane, at, "file.copy_line", None, None);
    assert_eq!(editor.clipboard.as_deref(), Some("cargo test -p tracelean-core -- --ignored"));
    let _ = std::fs::remove_dir_all(&root);
}

/// F12 on a name goes to where it is declared.
#[test]
fn f12_goes_to_the_definition() {
    let root = project();
    std::fs::write(root.join("src/def.rs"), "fn helper() {}\n").unwrap();
    std::fs::write(root.join("src/use.rs"), "fn main() {\n    helper();\n}\n").unwrap();
    let mut editor = Editor::open(root.clone(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 120, height: 20 };
    let at = row_of(&mut editor, "src/use.rs");
    editor.choose(EXPLORER, at, "file.open", None, None);
    editor.place(DOCUMENT, 18);
    editor.chord("F12");
    assert!(matches!(editor.buffer().kind, BufferKind::File { ref path } if path == "src/def.rs"), "{:?}", editor.buffer().kind);
    assert_eq!(editor.line_and_column(), (0, 0));
    let _ = std::fs::remove_dir_all(&root);
}

/// Searching every file lists each line that holds the text, each a link.
#[test]
fn a_search_lists_every_line_that_holds_the_text() {
    let (root, mut editor) = opened();
    editor.list_occurrences("LINE 1", false);
    let pane = editor.screen.focus.clone();
    let shown = shown_in(&editor, &pane);
    assert!(shown.starts_with("at: src/long.rs:1 line 1"), "{shown}");
    assert_eq!(shown.lines().count(), 11, "line 1, then 10–19: {shown}");
    let at = shown[..shown.find("src/long.rs:10").unwrap()].chars().count();
    editor.choose(&pane, at, "file.open", None, None);
    assert_eq!(editor.line_and_column(), (9, 0));
    let _ = std::fs::remove_dir_all(&root);
}
