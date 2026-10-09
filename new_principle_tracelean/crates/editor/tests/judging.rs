//! A requirement shows each clause's judgement: a drift as a drift, with its
//! note, and a delegated agreement with the person who delegated it.

use std::path::{Path, PathBuf};

use tracelean_core::evidence::{Bond, Level};
use tracelean_core::surface::keymap;
use tracelean_core::surface::screen::DOCUMENT;
use tracelean_core::surface::view::plain_text;
use tracelean_core::trace::lockfile;
use tracelean_core::trace::record::{Detail, Evidence, Key};
use tracelean_editor::Editor;

const KEYMAP: &str = include_str!("../../../assets/keymap.json");

/// The demo's requirements, Lean and lock, in a scratch directory.
fn demo(name: &str) -> PathBuf {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo");
    let dir = std::env::temp_dir().join(format!("tracelean-judging-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["reqs", "specs", "src", ".tracelean"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
        for entry in std::fs::read_dir(demo.join(sub)).unwrap().flatten() {
            if entry.path().is_file() {
                std::fs::copy(entry.path(), dir.join(sub).join(entry.file_name())).unwrap();
            }
        }
    }
    dir
}

/// Put one judgement in the lock, as `--verdict` leaves it.
fn judged(root: &Path, clause: &str, verdict: &str, delegated_by: Option<&str>, note: Option<&str>) {
    let mut lock = lockfile::read(root).expect("the demo has a lock");
    lock.evidence.retain(|r| !(r.key.clause.as_deref() == Some(clause) && r.key.bond == Bond::RequirementModel));
    lock.evidence.push(Evidence {
        key: Key { req_id: "REQ-THERMO".into(), clause: Some(clause.into()), bond: Bond::RequirementModel },
        level: if verdict == "agrees" { Level::L2 } else { Level::L1 },
        detail: Detail::Judge {
            verdict: verdict.into(),
            judged_by: "claude-review".into(),
            delegated_by: delegated_by.map(String::from),
            prompt_version: "1".into(),
            note: note.map(String::from),
        },
        link_hash: "l".into(),
        inputs: vec![("requirement".into(), "r".into()), ("model".into(), "m".into())],
    });
    lockfile::write(root, &lock).unwrap();
}

fn requirement(root: &Path) -> String {
    let keys = keymap::load(KEYMAP, &keymap::actions()).unwrap();
    let mut editor = Editor::open(root.to_path_buf(), keys);
    editor.region = tracelean_core::surface::screen::Rect { left: 0, top: 0, width: 160, height: 80 };
    editor.choose(DOCUMENT, 0, "trace.requirement", Some("REQ-THERMO".into()), None);
    let placed = editor.laid_out(editor.region);
    let found = placed.iter().find(|p| p.pane == DOCUMENT).expect("the document is placed");
    plain_text(found.buffer.clone()).join("\n")
}

/// @tests REQ-JUDGE.judgement_shown
/// @tests REQ-JUDGE.drift_recorded
#[test]
fn a_judged_drift_is_shown_with_its_note() {
    let root = demo("drift");
    judged(&root, "to_celsius", "drift", Some("ricostynha"), Some("the model truncates"));
    let shown = requirement(&root);
    // The line wraps at the pane's width; the words are what matter.
    let words: Vec<&str> = shown.split_whitespace().collect();
    let said = words.join(" ");
    assert!(
        said.contains("judged: drift — the model truncates by claude-review (delegated by ricostynha)"),
        "{shown}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// @tests REQ-JUDGE.judgement_shown
/// @tests REQ-JUDGE.human_decides
#[test]
fn a_delegated_agreement_names_who_delegated_it() {
    let root = demo("delegated");
    judged(&root, "to_fahrenheit", "agrees", Some("ricostynha"), None);
    let shown = requirement(&root);
    assert!(shown.contains("agrees by claude-review — L2 (delegated by ricostynha)"), "{shown}");
    let _ = std::fs::remove_dir_all(&root);
}
