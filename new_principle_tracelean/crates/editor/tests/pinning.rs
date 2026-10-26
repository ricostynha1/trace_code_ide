use std::path::{Path, PathBuf};

use tracelean_core::surface::keymap;
use tracelean_core::surface::screen::DOCUMENT;
use tracelean_core::surface::view::plain_text;
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

fn copy_of(name: &str, parts: &[&str]) -> PathBuf {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo");
    let dir = std::env::temp_dir().join(format!("tracelean-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in parts {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for entry in std::fs::read_dir(demo.join(sub)).unwrap().flatten() {
            if entry.path().is_file() {
                std::fs::copy(entry.path(), dir.join(sub).join(entry.file_name())).unwrap();
            }
        }
    }
    dir
}

fn thermo() -> PathBuf {
    copy_of("pinning", &["reqs", "specs"])
}

/// @tests REQ-DRT-SCHEMA.derived_from_both
#[test]
fn a_derived_test_counts_as_differential_testing() {
    let root = copy_of("derived", &["reqs", "specs", "src", "tests", ".tracelean", ".tracelean/evidence"]);
    let shown = requirement(&root);
    assert!(shown.contains("drt 2/4"), "{shown}");
    let _ = std::fs::remove_dir_all(&root);
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
/// @tests REQ-STRENGTH.pinned_by_theorem
/// @tests REQ-STRENGTH.strength_per_model
#[test]
#[ignore = "runs Lean; run with --ignored"]
fn a_model_is_pinned_once_lean_accepts_its_obligation_and_until_it_changes() {
    let root = thermo();
    let shown = requirement(&root);
    assert!(shown.contains("attempted\n    Thermo::to_fahrenheit_pinned: changed since it was checked"), "{shown}");

    let check = |root: &Path| {
        let index = tracelean_core::trace::index::build(root);
        let report = tracelean_core::drt::proofs::check(root, &index).expect("Lean runs");
        tracelean_core::drt::proofs::save(root, &report).unwrap();
    };
    check(&root);
    let shown = requirement(&root);
    assert!(shown.contains("pinned\n    Thermo::to_fahrenheit_pinned"), "{shown}");
    assert!(shown.contains("pinned\n    Thermo::to_celsius_pinned"), "{shown}");

    let spec = root.join("specs/Thermo.lean");
    let text = std::fs::read_to_string(&spec)
        .unwrap()
        .replace("9 * c < 5 * (f - 32) + 5", "9 * c < 5 * (f - 32) + 10")
        .replace("@pins REQ-THERMO.to_celsius", "");
    std::fs::write(&spec, text).unwrap();
    let shown = requirement(&root);
    assert!(shown.contains("attempted\n    Thermo::to_fahrenheit_pinned"), "{shown}");
    assert!(shown.contains("open\n    owes a theorem annotated @pins for Thermo::toCelsius"), "{shown}");
    let _ = std::fs::remove_dir_all(&root);
}
