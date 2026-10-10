//! Line coverage beside the code: which lines tests ran, which they did not,
//! and who ran each — measured on the demo, shown by the editor.

use std::path::{Path, PathBuf};

use tracelean_core::surface::keymap;
use tracelean_core::surface::screen::DOCUMENT;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

/// The demo, as a project of its own, in a scratch directory.
fn demo() -> PathBuf {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo");
    let dir = std::env::temp_dir().join(format!("tracelean-coverage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["", "reqs", "specs", "src", "tests"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for entry in std::fs::read_dir(from.join(sub)).unwrap().flatten() {
            if entry.path().is_file() {
                std::fs::copy(entry.path(), dir.join(sub).join(entry.file_name())).unwrap();
            }
        }
    }
    dir
}

fn opened(root: &Path, file: &str) -> Editor {
    let keys = keymap::load(KEYMAP, &keymap::actions()).unwrap();
    let mut editor = Editor::open(root.to_path_buf(), keys);
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 60 };
    editor.choose(DOCUMENT, 0, "file.open", Some(file.into()), None);
    editor
}

fn coverage_of(editor: &Editor) -> Vec<(usize, u64, String)> {
    let placed = editor.laid_out(editor.region);
    let found = placed.iter().find(|p| p.pane == DOCUMENT).expect("the document is placed");
    editor.coverage_shown(&found.buffer, found.top, found.at.height as usize)
}

/// Each test run alone: the branches `describe`'s one test never reaches are
/// uncovered, and a line two tests reach names both, with their counts.
///
/// @tests REQ-LINECOV.per_test
/// @tests REQ-LINECOV.uncovered_shown
/// @tests REQ-LINECOV.stale_hidden
/// @tests REQ-LINECOV.requirement_summary
/// @tests REQ-LINECOV.lines_listed
#[test]
#[ignore = "builds and runs the demo's tests under coverage; run with --ignored"]
fn uncovered_lines_and_the_tests_behind_each_line_are_shown() {
    let root = demo();
    let files = tracelean_core::observe::workspace::snapshot(&root).files;
    let (coverage, tests) = tracelean_core::drt::lines_run::measure(&root, &files).expect("measured");
    assert_eq!(tests, 3);
    tracelean_core::drt::lines_run::write(&root, &coverage).unwrap();

    let mut editor = opened(&root, "src/celsius.rs");
    let shown = coverage_of(&editor);
    let text = std::fs::read_to_string(root.join("src/celsius.rs")).unwrap();
    let line_of = |needle: &str| text.lines().position(|l| l.contains(needle)).unwrap();
    let at = |line: usize| shown.iter().find(|(l, _, _)| *l == line).cloned();

    let (_, hits, said) = at(line_of("\"steam\"")).expect("the steam branch is measured");
    assert_eq!((hits, said.as_str()), (0, "no test runs this line"));
    let (_, hits, said) = at(line_of("(degrees * 9).div_euclid(5)")).expect("measured");
    assert_eq!(hits, 7);
    assert_eq!(said, "run 7 times by converting_back_gives_what_went_in ×6, the_scales_meet_at_minus_forty ×1");

    // The requirement says how much of what implements each clause is run.
    editor.choose(DOCUMENT, 0, "trace.requirement", Some("REQ-TABLE".into()), None);
    let document = |editor: &Editor| {
        let placed = editor.laid_out(editor.region);
        let found = placed.iter().find(|p| p.pane == DOCUMENT).unwrap();
        tracelean_core::surface::view::plain_text(found.buffer.clone()).join("\n")
    };
    let requirement = document(&editor);
    assert!(requirement.contains("covered     REQ-TABLE.every_sample  4/7 lines run (57%), by 1 test"), "{requirement}");
    // And over the whole requirement, which a click opens line by line: the
    // tests behind each item, and each line none ran as a link to it.
    assert!(requirement.contains("covered    REQ-TABLE  "), "{requirement}");
    editor.choose(DOCUMENT, 0, "trace.coverage", Some("REQ-TABLE".into()), None);
    let lines = document(&editor);
    assert!(lines.starts_with("Coverage of REQ-TABLE"), "{lines}");
    assert!(lines.contains("ran by  below_zero_is_ice"), "{lines}");
    assert!(lines.contains(&format!("src/celsius.rs:{}  \"steam\"", line_of("\"steam\"") + 1)), "{lines}");

    // Typed into, the file is no longer the text that was measured.
    editor.choose(DOCUMENT, 0, "file.open", Some("src/celsius.rs".into()), None);
    editor.paste("// a line\n");
    assert!(coverage_of(&editor).is_empty(), "coverage of other text is shown");
    let _ = std::fs::remove_dir_all(&root);
}
