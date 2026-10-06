//! What the editor answers when it is asked for a report, and what it does
//! when it is told to watch.
//!
//! The thing being pinned down here is not the wording. It is that every
//! report an action can ask for is either computed from the tree or names the
//! command that would compute it — never a blank page, and never a number
//! nobody produced.

use std::path::{Path, PathBuf};

use tracelean_core::surface::act::dispatch;
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::plain_text;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

/// A working tree of our own, so a test never reads the project it is in.
fn project(files: &[(&str, &str)]) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tracelean-editor-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, content).expect("a file");
    }
    dir
}

const TREE: &[(&str, &str)] = &[
    ("reqs/a.md", "---\nid: REQ-A\ndecomposition: complete\nclauses:\n  one: It does the thing.\n---\nbody\n"),
    ("src/i.rs", "// @implements REQ-A.one\npub fn f() {}\n"),
];

/// Run an action and read what the editor put on screen.
fn ask(editor: &mut Editor, action: &str) -> String {
    let intent = dispatch(action.to_string(), editor.focus(), editor.workspace());
    editor.perform(intent);
    plain_text(editor.buffer()).join("\n")
}

fn said(editor: &Editor) -> String {
    plain_text(editor.status.clone()).join("\n")
}

/// Every report an action can reach says something about this tree.
///
/// The failure this guards against is a report that silently shows nothing:
/// a person reading a blank page cannot tell an empty project from an
/// unimplemented feature.
#[test]
fn every_report_an_action_can_ask_for_answers() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    let reports = [
        "trace.check",
        "trace.evidence",
        "trace.findings",
        "trace.lock",
        "trace.rollup",
        "trace.stale",
        "drt.bindings",
        "drt.coverage",
        "drt.judge",
        "drt.run",
        "drt.shrink",
        "history.tree",
    ];
    for action in reports {
        let shown = ask(&mut editor, action);
        assert!(!shown.trim().is_empty(), "`{action}` showed nothing");
        assert!(
            !shown.contains("not computed") && !shown.contains("not wired"),
            "`{action}` still says it is not wired:\n{shown}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// A report that would mean running something names the command instead.
///
/// The editor starts no processes, so the alternative to naming the command is
/// a result nobody computed.
#[test]
fn a_report_that_would_run_something_names_the_command() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    let shown = ask(&mut editor, "drt.run");
    assert!(shown.contains("cargo test"), "the run report does not name a command:\n{shown}");
    assert!(shown.contains("starts none"), "the run report does not say why:\n{shown}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Locking writes a lock file that reads back as one.
#[test]
fn locking_writes_a_lock_that_parses() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    let shown = ask(&mut editor, "trace.lock");
    assert!(shown.contains("trace.lock"), "the lock report does not name the file:\n{shown}");

    let written = std::fs::read_to_string(root.join(".tracelean/trace.lock")).expect("a lock");
    let parsed = tracelean_core::trace::lockfile::parse(&written).expect("a readable lock");
    assert!(parsed.requirements.contains_key("REQ-A"), "the lock lost the requirement");
    assert!(!parsed.links.is_empty(), "the lock lost the link");

    // Writing it again over the same tree writes the same bytes.
    ask(&mut editor, "trace.lock");
    let again = std::fs::read_to_string(root.join(".tracelean/trace.lock")).expect("a lock");
    assert_eq!(written, again, "two locks of one tree differ");
    let _ = std::fs::remove_dir_all(&root);
}

/// A clause nothing has earned evidence for reads L1, not blank and not higher.
#[test]
fn a_clause_with_no_recorded_evidence_reads_l1() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    let shown = ask(&mut editor, "trace.evidence");
    assert!(shown.contains("REQ-A.one"), "the clause is missing:\n{shown}");
    assert!(shown.contains("l1"), "the clause does not read L1:\n{shown}");
    assert!(shown.contains("implements"), "the role it is claimed by is missing:\n{shown}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Coverage of an unclaimed decomposition is written as a lower bound.
#[test]
fn a_roll_up_says_what_it_is_a_bound_on() {
    let root = project(&[
        ("reqs/a.md", "---\nid: REQ-A\nclauses:\n  one: It does the thing.\n---\nbody\n"),
        ("src/i.rs", "// @implements REQ-A.one\npub fn f() {}\n"),
    ]);
    let mut editor = Editor::open(root.clone(), keymap());
    let shown = ask(&mut editor, "trace.rollup");
    assert!(shown.contains("REQ-A"), "the requirement is missing:\n{shown}");
    assert!(shown.contains('≥'), "an open decomposition read as exact:\n{shown}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Watching is reading the tree somebody else wrote, and nothing else.
#[test]
fn watching_sees_a_file_written_outside_the_editor() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());

    // Somebody else — an agent, a compiler, a person in another window.
    std::fs::write(root.join("src/j.rs"), "pub fn g() {}\n").expect("a file");

    let shown = ask(&mut editor, "observe.start");
    assert!(shown.contains("src/j.rs"), "the new file was not observed:\n{shown}");
    assert!(said(&editor).contains("observed"), "the editor did not say it observed");

    ask(&mut editor, "observe.accept");
    assert!(
        editor.workspace().files.contains_key("src/j.rs"),
        "accepting did not take the file in"
    );
    // And it is in the history like any other change.
    assert!(editor.tree.undo().is_some(), "accepting left nothing to undo");
    let _ = std::fs::remove_dir_all(&root);
}

/// Rejecting drops the change and leaves the working tree where it is.
#[test]
fn rejecting_does_not_delete_what_somebody_else_wrote() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    std::fs::write(root.join("src/j.rs"), "pub fn g() {}\n").expect("a file");

    ask(&mut editor, "observe.start");
    ask(&mut editor, "observe.reject");
    assert!(said(&editor).contains("rejected"), "the editor did not say it rejected");
    assert!(
        !editor.workspace().files.contains_key("src/j.rs"),
        "rejecting took the file in anyway"
    );
    assert!(Path::new(&root.join("src/j.rs")).exists(), "rejecting deleted somebody else's file");

    // And there is nothing left pending.
    ask(&mut editor, "observe.reject");
    assert!(said(&editor).contains("nothing to reject"), "the change was still pending");
    let _ = std::fs::remove_dir_all(&root);
}

/// A tree nobody has touched has nothing to observe.
#[test]
fn an_untouched_tree_observes_nothing() {
    let root = project(TREE);
    let mut editor = Editor::open(root.clone(), keymap());
    ask(&mut editor, "observe.start");
    assert!(said(&editor).contains("nothing to observe"), "a clean tree reported changes");
    let _ = std::fs::remove_dir_all(&root);
}
