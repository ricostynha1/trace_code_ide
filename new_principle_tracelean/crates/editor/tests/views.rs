//! The editor's views, from the shell: `tracelean-view` prints what a station
//! or report shows, on a copy of the demo, and leaves what a person has open
//! alone.

use std::path::{Path, PathBuf};
use std::process::Command;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a directory");
    for entry in std::fs::read_dir(from).expect("readable").flatten() {
        let name = entry.file_name();
        if matches!(name.to_str(), Some("target" | "sessions" | "editor.json")) {
            continue;
        }
        if entry.path().is_dir() {
            copy(&entry.path(), &to.join(&name));
        } else {
            std::fs::copy(entry.path(), to.join(&name)).expect("copied");
        }
    }
}

fn demo() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo");
    let to = std::env::temp_dir().join(format!("tracelean-views-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&to);
    copy(&here, &to);
    to
}

fn view(root: &Path, words: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tracelean-view")).arg(root).args(words).output().expect("ran");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).to_string())
}

/// Each view prints the text the editor produces for it — the same words a
/// person reads — and nothing a person keeps is written.
///
/// @tests REQ-CONTEXT.views_from_the_shell
#[test]
fn every_view_prints_what_the_editor_shows_and_keeps_nothing() {
    let root = demo();
    let expect: &[(&[&str], &str)] = &[
        (&["requirements"], "REQ-THERMO"),
        (&["design"], "REQ-TABLE"),
        (&["requirement", "REQ-THERMO"], "to_celsius"),
        (&["trace", "src/celsius.rs"], "REQ-THERMO.to_celsius"),
        (&["context", "REQ-THERMO.to_celsius"], "to_celsius"),
        (&["findings"], "has no model"),
        (&["evidence"], "REQ-THERMO"),
        (&["bindings"], ""),
        (&["keys"], "Space"),
    ];
    for (words, seen) in expect {
        let (ok, text) = view(&root, words);
        assert!(ok && text.contains(seen), "{words:?} did not show `{seen}`:\n{text}");
    }
    assert!(!root.join(".tracelean/editor.json").exists(), "a view kept the person's tabs");
    let (ok, _) = view(&root, &["lock"]);
    assert!(!ok, "a view that writes was offered");
    let _ = std::fs::remove_dir_all(root);
}
