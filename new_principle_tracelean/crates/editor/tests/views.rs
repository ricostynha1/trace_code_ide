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

fn prints_only_what_the_editor_makes(source: &str) -> bool {
    source.contains("editor.view(")
        && !source.contains("Buffer {")
        && !source.contains("produce::")
        && !source.contains("make::")
}

/// @tests REQ-CONTEXT.views_from_the_shell
/// @structural REQ-CONTEXT.views_from_the_shell reason="the shell and the window call the one producer, which is a fact about the call graph; what each producer makes is modelled under REQ-SHOW"
#[test]
fn the_shell_prints_what_the_editor_produces_and_builds_nothing() {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/tracelean-view.rs"))
        .expect("the shell's source");
    assert!(prints_only_what_the_editor_makes(&source));
    assert!(!prints_only_what_the_editor_makes("let b = Buffer { text };"));
    assert!(!prints_only_what_the_editor_makes("editor.view(x); produce::requirements_buffer(n, false)"));
}

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
        (&["findings"], "missing a model"),
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
