//! `tracelean-trace --drt` on a tree of its own: a clause that agreed is not
//! run again while what it ran on is unchanged, and is the moment it is not.

use std::path::{Path, PathBuf};
use std::process::Command;

const TREE: &[(&str, &str)] = &[
    (
        "reqs/a.md",
        "---\nid: REQ-A\ntitle: Doubling\nstatus: approved\ndecomposition: complete\nclauses:\n  doubles: Doubling shall give twice what went in.\n---\n",
    ),
    ("specs/M.lean", "namespace M\n\n/-- @models REQ-A.doubles -/\ndef double (n : Int) : Int := 2 * n\n\nend M\n"),
    ("src/double.rs", "/// @implements REQ-A.doubles\npub fn double(n: i64) -> i64 {\n    2 * n\n}\n"),
];

fn tree() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-drt-cached-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, text) in TREE {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(path, text).expect("written");
    }
    dir
}

fn drt(root: &Path, again: bool) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tracelean-trace"));
    command.arg(root).arg("--drt");
    if again {
        command.arg("--again");
    }
    String::from_utf8_lossy(&command.output().expect("ran").stdout).to_string()
}

/// @tests REQ-STALE.current_not_rerun
#[test]
#[ignore = "builds a Lean and a Rust runner; run with --ignored"]
fn an_agreed_clause_is_run_again_only_when_what_it_ran_on_changed() {
    let root = tree();
    assert!(drt(&root, false).contains("agreed     REQ-A.doubles"), "the first run runs");
    assert!(drt(&root, false).contains("cached     REQ-A.doubles"), "an unchanged clause is not run again");
    assert!(drt(&root, true).contains("agreed     REQ-A.doubles"), "--again runs it");
    std::fs::write(root.join("src/double.rs"), "/// @implements REQ-A.doubles\npub fn double(n: i64) -> i64 {\n    n + n\n}\n").unwrap();
    assert!(drt(&root, false).contains("agreed     REQ-A.doubles"), "a changed implementation is run again");
    let _ = std::fs::remove_dir_all(root);
}
