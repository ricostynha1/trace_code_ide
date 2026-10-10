//! Everything that must be done again is listed by one command whose exit
//! status says whether anything is (`REQ-STALE.listed_for_a_script`).

use std::path::PathBuf;
use std::process::Command;

fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tracelean-stale-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("reqs")).unwrap();
    std::fs::write(
        dir.join("reqs/REQ-T.md"),
        "---\nid: REQ-T\ntitle: T\nstatus: approved\ndecomposition: open\nclauses:\n  c: A thing shall hold.\n---\n\n# T\n",
    )
    .unwrap();
    dir
}

fn stale(dir: &PathBuf) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_tracelean-trace")).arg(dir).arg("--stale").output().unwrap();
    (out.status.code().unwrap(), String::from_utf8_lossy(&out.stdout).to_string())
}

/// A record whose requirement hash no longer matches is listed, with what to
/// do about it, and the exit status is non-zero; with nothing recorded it is
/// zero.
///
/// @tests REQ-STALE.listed_for_a_script
#[test]
fn the_exit_status_says_whether_anything_must_be_redone() {
    let dir = project("one");
    assert_eq!(stale(&dir), (0, "0 to redo\n".to_string()));

    std::fs::create_dir_all(dir.join(".tracelean/evidence")).unwrap();
    std::fs::write(
        dir.join(".tracelean/evidence/REQ-T.c.ModelImpl.json"),
        r#"{"key":{"reqId":"REQ-T","clause":"c","bond":"modelImpl"},"level":"L3",
            "detail":{"drt":{"seed":1,"cases":10,"op":"REQ-T.c"}},"linkHash":"0000000000000000",
            "inputs":[["implementation","0000000000000000"],["model","0000000000000000"],["requirement","0000000000000000"]]}"#,
    )
    .unwrap();
    let (code, text) = stale(&dir);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("differential test") && text.contains("REQ-T.c"), "{text}");
    assert!(text.contains("redo:") && text.ends_with("1 to redo\n"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
