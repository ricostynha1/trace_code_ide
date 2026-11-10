mod harness;

use harness::{name, strukt};
use tracelean_core::drt::schema::Schema;
use tracelean_core::launch::start;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// @tests REQ-HANDOFF.started_not_driven
#[test]
fn a_handed_off_program_is_not_waited_for() {
    let marker = std::env::temp_dir().join(format!("tracelean-handoff-wait-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    start(&argv(&["sh", "-c", &format!("sleep 1; echo > {}", marker.display())])).expect("sh starts");
    assert!(!marker.exists(), "start returned only after the program had finished");
    let _ = std::fs::remove_file(&marker);
}

/// @tests REQ-HANDOFF.started_not_driven
#[test]
fn a_handed_off_program_is_given_no_input() {
    let out = std::env::temp_dir().join(format!("tracelean-handoff-{}", std::process::id()));
    let _ = std::fs::remove_file(&out);
    start(&argv(&["sh", "-c", &format!("cat > {0}.in; echo ended > {0}", out.display())])).expect("sh starts");
    for _ in 0..150 {
        if out.is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let read = std::fs::read_to_string(format!("{}.in", out.display())).unwrap_or_default();
    assert!(out.is_file(), "the program's input never ended");
    assert_eq!(read, "", "the program was given input");
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(format!("{}.in", out.display()));
}

/// @tests REQ-HANDOFF.failure_named
#[test]
fn a_program_that_cannot_start_is_named() {
    let why = start(&argv(&["tracelean-no-such-program"])).unwrap_err();
    assert!(why.contains("tracelean-no-such-program"), "{why}");
    assert!(start(&[]).is_err());
}

fn os() -> Schema {
    name(&["linux", "macos", "windows", "freebsd"])
}

/// @drt REQ-HANDOFF.file_to_vscode
/// @tests REQ-HANDOFF.file_to_vscode
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_vscode_is_started() {
    let schema = strukt(&[
        ("os", os()),
        ("file", name(&["/p/a.lean", "C:/a b.lean", "a:b"])),
        ("line", Schema::Nat { max: Some(200), edges: vec![0, 9, 99] }),
        ("column", Schema::Nat { max: Some(200), edges: vec![0, 9, 99] }),
    ]);
    harness::agree("REQ-HANDOFF.file_to_vscode", &schema, 113, 2_000);
}

/// @drt REQ-HANDOFF.report_to_browser
/// @tests REQ-HANDOFF.report_to_browser
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_the_browser_is_started() {
    let schema = strukt(&[("os", os()), ("page", name(&["/r/index.html", "C:/r/index.html", ""]))]);
    harness::agree("REQ-HANDOFF.report_to_browser", &schema, 127, 2_000);
}

/// @tests REQ-HANDOFF.started_not_driven
/// @tests REQ-HANDOFF.choice_opened
#[test]
fn a_question_is_answered_away_from_the_keys() {
    let answer = tracelean_core::launch::ask(&argv(&["sh", "-c", "sleep 1; echo /home/u/proj"]));
    assert!(answer.try_recv().is_err(), "ask waited for the answer");
    assert_eq!(answer.recv().unwrap().unwrap(), "/home/u/proj\n");
    let closed = tracelean_core::launch::ask(&argv(&["sh", "-c", "echo /x; exit 1"]));
    assert_eq!(closed.recv().unwrap().unwrap(), "", "a picker closed without a choice still chose");
    let missing = tracelean_core::launch::ask(&argv(&["tracelean-no-such-picker"]));
    assert!(missing.recv().unwrap().unwrap_err().contains("tracelean-no-such-picker"));
}

/// @drt REQ-HANDOFF.folder_picked
/// @tests REQ-HANDOFF.folder_picked
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_folder_picker() {
    let schema = strukt(&[
        ("os", os()),
        ("desktop", name(&["KDE", "GNOME", "ubuntu:GNOME", "", "X-KDE-Plasma"])),
        ("start", name(&["/p", "/home/u/a b", ""])),
    ]);
    harness::agree("REQ-HANDOFF.folder_picked", &schema, 131, 2_000);
}

/// @drt REQ-HANDOFF.choice_opened
/// @tests REQ-HANDOFF.choice_opened
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_the_picker_chose() {
    let schema = strukt(&[("said", name(&["/home/u/p\n", "C:\\p\r\n", "", "\n", "/a\n/b\n", "/no-newline", "\r\n"]))]);
    harness::agree("REQ-HANDOFF.choice_opened", &schema, 137, 2_000);
}
