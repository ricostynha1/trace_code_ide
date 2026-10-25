use std::path::{Path, PathBuf};

use tracelean_core::history::command::Command;
use tracelean_core::surface::act::{Intent, Watch};
use tracelean_core::surface::keymap::{self, Keymap};
use tracelean_core::surface::view::{plain_text, BufferKind};
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn keymap() -> Keymap {
    keymap::load(KEYMAP, &keymap::actions()).expect("the shipped keymap loads")
}

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-reports-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let at = dir.join(path);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("a directory");
        std::fs::write(at, text).expect("written");
    }
    dir
}

fn opened(root: &Path) -> Editor {
    let mut editor = Editor::open(root.to_path_buf(), keymap());
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 120, height: 20 };
    editor
}

fn record(editor: &mut Editor, title: &str) -> String {
    plain_text(editor.view(BufferKind::Record { title: title.into() })).join("\n")
}

fn said(editor: &Editor) -> String {
    plain_text(editor.status.clone()).join("\n")
}

const REQ: &str = "---\nid: REQ-A\ntitle: A\nclauses:\n  one: First.\n---\nbody\n";

/// @tests REQ-DOCLINK.review_is_not_error
#[test]
fn every_document_is_reported_current_in_review_or_dangling() {
    let root = project("stale", &[("reqs/a.md", REQ), ("src/a.rs", "// @implements REQ-A.one\npub fn a() {}\n")]);
    let hash = tracelean_core::trace::doclink::current_hashes(&tracelean_core::trace::index::build(&root))
        .get("REQ-A")
        .cloned()
        .expect("the requirement hashes");
    let doc = |target: &str, hash: &str| format!("---\ndescribes: [{target}]\ndescribed_hash:\n  {target}: {hash}\n---\n\n# Doc\n");
    std::fs::write(root.join("current.md"), doc("REQ-A", &hash)).unwrap();
    std::fs::write(root.join("review.md"), doc("REQ-A", "0000000000000000")).unwrap();
    std::fs::write(root.join("gone.md"), doc("REQ-GONE", "0000000000000000")).unwrap();
    let mut editor = opened(&root);
    let text = record(&mut editor, "stale");
    assert!(text.contains("current.md describes REQ-A"), "{text}");
    assert!(text.contains("review.md describes REQ-A, which has changed since"), "{text}");
    assert!(text.contains("gone.md describes REQ-GONE, which is gone"), "{text}");
    let _ = std::fs::remove_dir_all(&root);

    let bare = project("stale-none", &[("reqs/a.md", REQ)]);
    let mut editor = opened(&bare);
    assert!(record(&mut editor, "stale").contains("no document records what it describes"));
    let _ = std::fs::remove_dir_all(&bare);
}

/// @tests REQ-LINECOV.requirement_summary
/// @tests REQ-LINECOV.clause_summary
/// @tests REQ-LINECOV.uncovered_shown
#[test]
fn coverage_opens_as_a_report_and_marks_the_lines_beside_a_file() {
    use tracelean_core::trace::lines::{Coverage, LineHits};
    let text = "// @implements REQ-A.one\npub fn a() -> u8 {\n    1\n}\n";
    let root = project("coverage", &[("reqs/a.md", REQ), ("src/a.rs", text)]);
    let mut coverage = Coverage::default();
    let hits = |line: u32, hits: u64| LineHits {
        line,
        hits,
        tests: if hits > 0 { vec![("t".to_string(), hits)] } else { Vec::new() },
    };
    coverage.files.insert(
        "src/a.rs".into(),
        (tracelean_core::trace::hash::text(text), vec![hits(2, 3), hits(3, 0)]),
    );
    tracelean_core::drt::lines_run::write(&root, &coverage).expect("written");
    let mut editor = opened(&root);
    assert!(record(&mut editor, "coverage REQ-A").starts_with("Coverage of REQ-A"));
    assert!(record(&mut editor, "coverage REQ-A.one").starts_with("Coverage of REQ-A.one"));
    let file = editor.view(BufferKind::File { path: "src/a.rs".into() });
    let marked = editor.coverage_shown(&file, 0, 10);
    assert_eq!(marked.iter().map(|(row, hits, _)| (*row, *hits)).collect::<Vec<_>>(), vec![(1, 3), (2, 0)], "{marked:?}");
    let report = editor.view(BufferKind::Record { title: "findings".into() });
    assert!(editor.coverage_shown(&report, 0, 10).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-LOCK.evidence_preserved
/// @tests REQ-ROLLUP.open_is_lower_bound
#[test]
fn every_report_says_what_it_holds_or_why_it_holds_nothing() {
    let empty = project("reports-empty", &[("src/a.rs", "a\n")]);
    let mut editor = opened(&empty);
    assert!(record(&mut editor, "rollup").contains("no requirement stands at the top"));
    assert!(record(&mut editor, "nonsense").contains("nothing produces a `nonsense` report"));
    for (title, said) in [
        ("drt run", "cargo test"),
        ("drt shrink", "differential_shrink"),
        ("drt coverage", "--ignored"),
        ("drt judge", "a judgement is a person's"),
    ] {
        assert!(record(&mut editor, title).contains(said), "{title}");
    }
    let _ = std::fs::remove_dir_all(&empty);

    let root = project("reports", &[("reqs/a.md", REQ), ("src/a.rs", "// @implements REQ-A.one\npub fn a() {}\n")]);
    let mut editor = opened(&root);
    let lock = record(&mut editor, "lock");
    assert!(lock.contains("1 requirements, 1 links, 0 evidence records"), "{lock}");
    assert!(root.join(".tracelean").exists() || lock.contains("written"), "{lock}");
    assert!(record(&mut editor, "rollup").contains("REQ-A"));
    assert!(record(&mut editor, "judge REQ-A.one").contains("nothing models REQ-A.one"));
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.unknown_is_refused
/// @tests REQ-NAV.uses_found
#[test]
fn what_is_not_there_is_said_by_name() {
    use tracelean_core::surface::act::Blocked;
    let root = project("unknown", &[("reqs/a.md", REQ), ("src/a.rs", "pub fn alpha() {}\nfn b() { alpha() }\n")]);
    let mut editor = opened(&root);
    assert!(record(&mut editor, "context REQ-NOPE").contains("no requirement is called REQ-NOPE"));
    assert!(record(&mut editor, "judge REQ-A").contains("nothing models REQ-A,"));
    editor.perform(Intent::Refuse { why: Blocked::UnknownAction { action: "fly".into() } });
    assert!(said(&editor).contains("`fly` is not an action"), "{}", said(&editor));
    editor.perform(Intent::Display { what: BufferKind::Record { title: "references alpha".into() } });
    let shown = plain_text(editor.buffer().clone()).join("\n") + &said(&editor);
    assert!(shown.contains("src/a.rs"), "{shown}");
    assert!(!plain_text(editor.stations()).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-SHOW.listing_from_entries
#[test]
fn a_folders_listing_holds_only_what_is_under_it() {
    let root = project("listing", &[("src/a.rs", "a\n"), ("src/b.rs", "b\n"), ("docs/c.md", "c\n")]);
    let mut editor = opened(&root);
    let listing = plain_text(editor.view(BufferKind::Directory { path: "src/".into() })).join("\n");
    assert!(listing.contains("src/a.rs") && listing.contains("src/b.rs"), "{listing}");
    assert!(!listing.contains("docs/c.md"), "{listing}");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-ACT.edits_are_commands
#[test]
fn accepting_nothing_or_a_refused_change_says_why() {
    let root = project("accept", &[("src/a.rs", "a\n")]);
    let mut editor = opened(&root);
    let accept = |commands: Vec<Command>| Intent::Observe { watch: Watch::Accept { command: Command::Batch { commands } } };
    editor.perform(accept(Vec::new()));
    assert!(said(&editor).contains("nothing has been observed"), "{}", said(&editor));
    let refused = Command::Insert { file: "missing.rs".into(), offset: 0, text: "x".into() };
    editor.perform(accept(vec![refused.clone()]));
    assert!(said(&editor).contains("refused"), "{}", said(&editor));

    let one = |command: Command| Intent::Observe {
        watch: Watch::AcceptFile { path: "src/a.rs".into(), command },
    };
    editor.perform(one(Command::Batch { commands: Vec::new() }));
    assert!(said(&editor).contains("src/a.rs has no change waiting"), "{}", said(&editor));
    editor.perform(one(refused));
    assert!(said(&editor).contains("refused"), "{}", said(&editor));
    assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "a\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-MYTH.modes_defined
#[test]
fn escape_goes_back_one_mode_and_typing_needs_a_file() {
    let root = project("keys", &[("src/a.rs", "a\n")]);
    let mut editor = opened(&root);
    let root_mode = editor.mode.clone();
    editor.key("Space");
    let leader = editor.mode.clone();
    editor.key("f");
    assert_ne!(editor.mode, leader, "`f` opened no menu");
    editor.key("Escape");
    assert_eq!(editor.mode, leader, "Escape did not go back one mode");
    editor.key("Escape");
    assert_eq!(editor.mode, root_mode, "Escape did not leave the menu");

    editor.perform(Intent::Display { what: BufferKind::Record { title: "findings".into() } });
    editor.key("i");
    assert!(said(&editor).contains("nothing here to type into"), "{}", said(&editor));
    let _ = std::fs::remove_dir_all(&root);
}
