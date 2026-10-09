//! The sandbox, end to end on a real tree: make one, let an "agent" write into
//! its copy, see the change and what it said, accept or reject it, end it.
//!
//! The agent here is this test writing files and a Claude Code transcript —
//! the editor cannot tell the difference, which is the point: it only ever
//! reads what was left behind.

use std::path::{Path, PathBuf};

use tracelean_core::surface::act::Intent;
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::{actions_at, plain_text, BufferKind};
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-sandbox-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(dir.join("src/b.rs"), "fn b() {}\n").unwrap();
    dir
}

fn act(editor: &mut Editor, action: &str) {
    let intent = tracelean_core::surface::act::dispatch(action.into(), editor.focus(), editor.workspace(), editor.waiting());
    editor.perform(intent);
}

fn panel(editor: &Editor) -> String {
    let held = editor.screen.opened.iter().find(|b| b.id == "record:sandbox").expect("the panel is open");
    plain_text(held.clone()).join("\n")
}

fn work(editor: &Editor) -> PathBuf {
    PathBuf::from(&editor.session.as_ref().expect("a session").work)
}

/// @tests REQ-OBS.workspace_is_a_copy
/// @tests REQ-OBS.visible_while_running
/// @tests REQ-SHOW.sandbox_session_shown
#[test]
fn an_agents_change_is_seen_reviewed_and_accepted_into_the_project() {
    let root = project("accept");
    let mut editor = Editor::open(root.clone(), keymap());
    act(&mut editor, "sandbox.new");
    assert!(panel(&editor).contains("enter.sh"), "no command to copy:\n{}", panel(&editor));

    act(&mut editor, "sandbox.copy");
    let copied = editor.clipboard.clone().unwrap_or_default();
    assert!(copied.starts_with("sh ") && copied.ends_with("enter.sh"), "nothing was copied: {copied}");
    let launcher = std::fs::read_to_string(copied.trim_start_matches("sh ")).expect("the launcher");
    assert!(launcher.contains("exec bwrap "), "{launcher}");

    // The agent edits one file and makes another, in the copy.
    std::fs::write(work(&editor).join("src/a.rs"), "fn a() { 1 }\n").unwrap();
    std::fs::write(work(&editor).join("src/c.rs"), "fn c() {}\n").unwrap();
    assert!(editor.tick(), "the panel did not notice the change");
    let shown = panel(&editor);
    assert!(shown.contains("modified") && shown.contains("src/a.rs"), "{shown}");
    assert!(shown.contains("created") && shown.contains("src/c.rs"), "{shown}");
    assert!(shown.contains("src/a.rs +1 −1") && shown.contains("src/c.rs +1 −0"), "no sizes:\n{shown}");
    assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn a() {}\n", "the real tree changed early");

    // Clicking the path opens the diff against what the agent wrote.
    let held = editor.screen.opened.iter().find(|b| b.id == "record:sandbox").unwrap().clone();
    let at = held.text[..held.text.find("src/a.rs").unwrap()].chars().count();
    assert_eq!(actions_at(held, at), vec!["observe.diff".to_string()]);
    editor.perform(Intent::Display { what: BufferKind::Review { target: "src/a.rs".into() } });
    let review = plain_text(editor.buffer()).join("\n");
    assert!(review.contains("fn a() { 1 }"), "the review is not of the agent's version:\n{review}");

    act(&mut editor, "observe.accept");
    assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn a() { 1 }\n");
    assert!(root.join("src/c.rs").exists(), "the created file did not reach the project");
    assert!(!editor.tick(), "accepted changes are still waiting");

    act(&mut editor, "sandbox.end");
    assert!(editor.session.is_none());
    assert!(panel(&editor).contains("[ New sandbox ]"));
    let _ = std::fs::remove_dir_all(&root);
}

/// Rejecting takes the change out of the copy and never touches the project.
#[test]
fn a_rejected_change_leaves_the_project_and_the_copy_as_they_were() {
    let root = project("reject");
    let mut editor = Editor::open(root.clone(), keymap());
    act(&mut editor, "sandbox.new");
    std::fs::remove_file(work(&editor).join("src/b.rs")).unwrap();
    editor.tick();
    assert!(panel(&editor).contains("deleted"), "{}", panel(&editor));
    act(&mut editor, "observe.reject");
    assert!(root.join("src/b.rs").exists());
    assert!(work(&editor).join("src/b.rs").exists(), "the copy still lacks the file");
    assert!(!editor.tick());
    let _ = std::fs::remove_dir_all(&root);
}

/// One file is accepted from its row and another rejected from its row; each
/// leaves the other waiting.
#[test]
fn changes_are_accepted_and_rejected_one_file_at_a_time() {
    let root = project("one");
    let mut editor = Editor::open(root.clone(), keymap());
    act(&mut editor, "sandbox.new");
    std::fs::write(work(&editor).join("src/a.rs"), "fn a() { 1 }\n").unwrap();
    std::fs::write(work(&editor).join("src/b.rs"), "fn b() { 2 }\n").unwrap();
    assert!(editor.tick());

    let press = |editor: &mut Editor, path: &str, button: &str, action: &str| {
        let held = editor.screen.opened.iter().find(|b| b.id == "record:sandbox").unwrap().clone();
        let row = held.text[..held.text.find(path).expect(path)].rfind('\n').map_or(0, |n| n + 1);
        let at = row + held.text[row..].find(button).expect(button);
        let at = held.text[..at].chars().count();
        assert_eq!(actions_at(held.clone(), at), vec![action.to_string()]);
        // The panel is in the side pane; the offset is the buffer's own.
        let pane = editor.laid_out(editor.region).into_iter().find(|p| p.buffer.id == "record:sandbox").unwrap();
        editor.choose(&pane.pane, at - text_offset_of_top(&held, pane.top), action, None, None);
    };
    press(&mut editor, "src/a.rs", "[✓]", "observe.accept_file");
    assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "fn a() { 1 }\n", "a.rs was not taken in");
    assert_eq!(std::fs::read_to_string(root.join("src/b.rs")).unwrap(), "fn b() {}\n", "b.rs was taken in too");
    let shown = panel(&editor);
    assert!(shown.contains("src/b.rs") && !shown.contains("src/a.rs"), "{shown}");

    press(&mut editor, "src/b.rs", "[✗]", "observe.reject_file");
    assert_eq!(std::fs::read_to_string(work(&editor).join("src/b.rs")).unwrap(), "fn b() {}\n", "the copy kept b.rs");
    assert!(panel(&editor).contains("none yet"), "{}", panel(&editor));
    let _ = std::fs::remove_dir_all(&root);
}

/// Where the window of a pane starts, in characters of its buffer.
fn text_offset_of_top(buffer: &tracelean_core::surface::view::Buffer, top: usize) -> usize {
    buffer.text.split('\n').take(top).map(|line| line.chars().count() + 1).sum()
}

/// What a Claude Code agent said appears in the panel, newest first.
///
/// @tests REQ-TRANSCRIPT.tool_format_read
#[test]
fn the_agents_conversation_appears_in_the_panel() {
    let root = project("chat");
    let home = std::env::temp_dir().join(format!("tracelean-claude-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    // The agent's state directory is where `CLAUDE_CONFIG_DIR` points.
    std::env::set_var("CLAUDE_CONFIG_DIR", &home);
    let mut editor = Editor::open(root.clone(), keymap());
    act(&mut editor, "sandbox.new");

    let canonical = root.canonicalize().unwrap();
    let slug = tracelean_core::observe::sandbox::claude_slug(&canonical.to_string_lossy());
    let dir = home.join("projects").join(slug);
    std::fs::create_dir_all(&dir).unwrap();
    write(&dir.join("t.jsonl"), &[
        r#"{"type":"user","message":{"role":"user","content":"make it faster"}}"#,
        r#"{"type":"assistant","message":{"model":"m","usage":{"input_tokens":5,"output_tokens":7},"content":[{"type":"text","text":"Looking at src/a.rs"},{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}"#,
    ]);
    editor.tick();
    let shown = panel(&editor);
    assert!(shown.contains("you: make it faster"), "{shown}");
    assert!(shown.contains("tool: Read(src/a.rs)"), "{shown}");
    assert!(shown.find("tool: Read").unwrap() < shown.find("you: make it faster").unwrap(), "not newest first");
    std::env::remove_var("CLAUDE_CONFIG_DIR");
    let _ = std::fs::remove_dir_all(&home);
    let _ = std::fs::remove_dir_all(&root);
}

fn write(path: &Path, lines: &[&str]) {
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

/// A sandbox starts from what the editor holds, so an edit not yet saved is
/// not mistaken for something the agent did.
#[test]
fn an_unsaved_edit_is_not_an_agents_change() {
    let root = project("unsaved");
    let mut editor = Editor::open(root.clone(), keymap());
    editor.perform(Intent::Display { what: BufferKind::File { path: "src/a.rs".into() } });
    editor.paste("// mine\n");
    act(&mut editor, "sandbox.new");
    editor.tick();
    assert!(panel(&editor).contains("Changes waiting (0)"), "{}", panel(&editor));
    assert!(std::fs::read_to_string(work(&editor).join("src/a.rs")).unwrap().starts_with("// mine"));
    let _ = std::fs::remove_dir_all(&root);
}
