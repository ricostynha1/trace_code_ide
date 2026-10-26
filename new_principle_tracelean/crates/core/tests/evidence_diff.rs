mod harness;

use std::path::Path;
use std::process::Command;

use tracelean_core::evidence::Mark;
use tracelean_core::observe::revision::states_at;
use tracelean_core::trace::diff::{diff, states, Change};
use tracelean_core::trace::index::build;
use tracelean_core::trace::lockfile::{read, render, write};
use tracelean_core::trace::proofcheck::ProofReport;

fn git(dir: &Path, args: &[&str]) {
    let ran = Command::new("git")
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(ran.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ran.stderr));
}

fn lock(dir: &Path) {
    write(dir, &render(&build(dir), vec![])).expect("the lock is written");
}

/// @tests REQ-DIFF.base_from_history
/// @tests REQ-DIFF.text_shown
/// @tests REQ-DIFF.losses_first
#[test]
fn a_revision_is_compared_without_being_checked_out() {
    let dir = harness::scratch("evidence-diff");
    std::fs::create_dir_all(dir.join("reqs")).unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("reqs/a.md"), "---\nid: REQ-A\nclauses:\n  one: The system shall do one.\n  two: The system shall do two.\n---\nbody").unwrap();
    std::fs::write(dir.join("src/i.rs"), "// @implements REQ-A.one\npub fn f() {}\n// @implements REQ-A.two\npub fn g() {}\n").unwrap();
    git(&dir, &["init", "-q"]);
    lock(&dir);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "base"]);

    std::fs::write(dir.join("reqs/a.md"), "---\nid: REQ-A\nclauses:\n  one: The system shall do one, twice.\n  two: The system shall do two.\n---\nbody").unwrap();
    std::fs::write(dir.join("src/i.rs"), "// @implements REQ-A.one\npub fn f() {}\npub fn g() {}\n").unwrap();
    lock(&dir);

    let index = build(&dir);
    let before = states_at(&dir, "HEAD").expect("the base is read from history");
    let after = states(&read(&dir).expect("a lock"), &index.requirements, &ProofReport::default());
    let found = diff(before, after);

    assert_eq!(found.len(), 2, "{found:#?}");
    assert_eq!(found[0].clause.as_deref(), Some("two"), "the loss comes first: {found:#?}");
    assert!(found[0].changes.contains(&Change::MarkLost { mark: Mark::Annotated }), "{found:#?}");
    assert!(
        found[0].changes.contains(&Change::LinkRemoved { role: "implements".into(), symbol: "src/i.rs::g".into() }),
        "{found:#?}"
    );
    assert_eq!(found[1].clause.as_deref(), Some("one"));
    assert_eq!(
        found[1].changes,
        vec![Change::Reworded {
            before: "The system shall do one.".into(),
            after: "The system shall do one, twice.".into()
        }]
    );
    let status = Command::new("git").args(["status", "--porcelain"]).current_dir(&dir).output().unwrap();
    assert!(!String::from_utf8_lossy(&status.stdout).is_empty(), "the working tree was left as it was");
    let _ = std::fs::remove_dir_all(&dir);
}
