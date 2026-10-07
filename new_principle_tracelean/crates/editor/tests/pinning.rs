//! A requirement shows whether each clause's specification pins its model, as
//! Lean last said — and stops saying so the moment a declaration changes.

use std::path::{Path, PathBuf};

use tracelean_core::surface::keymap;
use tracelean_core::surface::screen::DOCUMENT;
use tracelean_core::surface::view::plain_text;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

/// The demo's requirements and Lean, in a scratch directory.
fn thermo() -> PathBuf {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo");
    let dir = std::env::temp_dir().join(format!("tracelean-pinning-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["reqs", "specs"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for entry in std::fs::read_dir(demo.join(sub)).unwrap().flatten() {
            std::fs::copy(entry.path(), dir.join(sub).join(entry.file_name())).unwrap();
        }
    }
    dir
}

fn requirement(root: &Path) -> String {
    let keys = keymap::load(KEYMAP, &keymap::actions()).unwrap();
    let mut editor = Editor::open(root.to_path_buf(), keys);
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 60 };
    editor.choose(DOCUMENT, 0, "trace.requirement", Some("REQ-THERMO".into()), None);
    let placed = editor.laid_out(editor.region);
    let found = placed.iter().find(|p| p.pane == DOCUMENT).expect("the document is placed");
    plain_text(found.buffer.clone()).join("\n")
}

/// @tests REQ-STRENGTH.kernel_decides
/// @tests REQ-STRENGTH.verdict_kept
/// @tests REQ-STRENGTH.qualifies_proof
#[test]
#[ignore = "runs Lean; run with --ignored"]
fn a_clause_is_pinned_once_lean_accepts_its_theorem_and_until_it_changes() {
    let root = thermo();
    let shown = requirement(&root);
    assert!(shown.contains("attempted\n    Thermo.to_fahrenheit_pinned  not yet checked"), "{shown}");

    let index = tracelean_core::trace::index::build(&root);
    let files = tracelean_core::observe::workspace::snapshot(&root).files;
    for plan in tracelean_core::trace::pinning::plans(&index, &files) {
        tracelean_core::drt::pins::check(&root, &plan);
    }
    let shown = requirement(&root);
    assert!(shown.contains("pinned\n    Thermo.to_fahrenheit_pinned"), "{shown}");
    assert!(shown.contains("pinned\n    Thermo.to_celsius_pinned"), "{shown}");

    // The specification loosened: the verdict was about the old one. And with
    // no `@pins` theorem, the clause says what it owes.
    let spec = root.join("specs/Thermo.lean");
    let text = std::fs::read_to_string(&spec)
        .unwrap()
        .replace("9 * c < 5 * (f - 32) + 5", "9 * c < 5 * (f - 32) + 10")
        .replace("@pins REQ-THERMO.to_celsius", "");
    std::fs::write(&spec, text).unwrap();
    let shown = requirement(&root);
    assert!(shown.contains("attempted\n    Thermo.to_fahrenheit_pinned"), "{shown}");
    assert!(shown.contains("open\n    owes a theorem annotated @pins: (∀ x1, Thermo.ToCelsius x1"), "{shown}");
    let _ = std::fs::remove_dir_all(&root);
}
