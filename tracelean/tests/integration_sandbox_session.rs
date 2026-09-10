//! End-to-end test for the sandboxed-session workspace (docs plan:
//! "Sandboxed workspace: run Claude Code inside tracelean's VM"):
//!
//!   create a reflink session -> run a real command inside its bwrap
//!   namespace -> SandboxWatcher notices the work-dir change -> mirrors it
//!   into AppState (undo tree + buffer) and the real tree -> undo restores
//!   the pre-image.
//!
//! Skips (not fails) if `bwrap` isn't on PATH, matching the sandbox's own
//! probe-and-degrade convention.

use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tracelean_core::sandbox::session::{bwrap_argv, bwrap_available, create_session, destroy_session};
use tracelean_core::sandbox::{mirror::SelfWrites, SandboxWatcher};
use tracelean_core::state::AppState;
use tracelean_core::EventSink;

#[derive(Default)]
struct CapturingSink(Mutex<Vec<(String, String)>>);
impl EventSink for CapturingSink {
    fn emit(&self, event: &str, payload: &str) {
        self.0.lock().unwrap().push((event.to_string(), payload.to_string()));
    }
}

fn wait_for<F: Fn() -> bool>(cond: F, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn watcher_mirrors_a_real_sandboxed_command_and_undo_restores_it() {
    if !bwrap_available() {
        eprintln!("skipping: bwrap not on PATH");
        return;
    }

    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("b.txt"), "will be deleted").unwrap();

    let spec = create_session(project.path(), true).expect("create_session");

    let mut state = AppState::new();
    state.set_project_root(spec.project_root.clone());
    let state = Arc::new(Mutex::new(state));
    let sink: Arc<dyn EventSink> = Arc::new(CapturingSink::default());

    let _watcher = SandboxWatcher::spawn(spec.clone(), Arc::clone(&state), SelfWrites::new(), sink)
        .expect("watcher spawn");

    // Run a real command inside the session's namespace — exactly what
    // `tracelean-sandbox shell` sets up for an interactive shell.
    let mut argv = bwrap_argv(&spec);
    argv.push("sh".into());
    argv.push("-c".into());
    argv.push("echo hi > a.txt; rm -f b.txt".into());
    let out = StdCommand::new("bwrap").args(&argv).output().expect("run sandboxed command");
    assert!(out.status.success(), "sandboxed command failed: {}", String::from_utf8_lossy(&out.stderr));

    let real_a = spec.project_root.join("a.txt");
    let real_b = spec.project_root.join("b.txt");

    let mirrored = wait_for(
        || real_a.exists() && !real_b.exists(),
        Duration::from_secs(5),
    );
    assert!(mirrored, "watcher did not mirror the sandboxed command into the real tree in time");
    assert_eq!(std::fs::read_to_string(&real_a).unwrap().trim(), "hi");

    // It landed in AppState too (buffer + undo tree), not just on disk.
    {
        let s = state.lock().unwrap();
        assert_eq!(s.get_content(&PathBuf::from("a.txt")).unwrap().trim(), "hi");
        assert!(s.get_content(&PathBuf::from("b.txt")).is_none(), "b.txt should have been removed from the buffer set");
    }

    // ...and it's undoable, like any other edit — one undo() call reverts
    // the whole mirrored batch (it was applied as a single Command::Batch,
    // so it is a single undo-tree node). Disk-level restore is a separate,
    // pre-existing GUI-layer concern (`sync_file_operations_to_disk` in
    // gui_backend) that isn't exercised by this core-only test.
    {
        let mut s = state.lock().unwrap();
        let outcome = s.undo();
        assert!(outcome.changed, "undo should have something to revert");
        assert_eq!(s.get_content(&PathBuf::from("b.txt")).unwrap(), "will be deleted");
        assert!(s.get_content(&PathBuf::from("a.txt")).is_none(), "a.txt was freshly created; undo should remove it from the buffer set");
    }

    let _ = destroy_session(&spec);
}

#[test]
fn repeated_watcher_ticks_on_a_stable_file_do_not_duplicate_content() {
    // User report: after one real edit inside a sandbox (appending a line
    // to a file), a later `cat` of that same file (read from *inside* the
    // sandbox, i.e. from `work_dir` directly) showed the whole file
    // content duplicated. Nothing in the mirror path writes back into
    // `work_dir` except `service::save_file`'s mirror-out (not exercised
    // here) — so this reproduces the other candidate: does the watcher's
    // own repeated ticking over an already-stable file corrupt anything,
    // on either side (`work_dir` or the real tree)?
    if !bwrap_available() {
        eprintln!("skipping: bwrap not on PATH");
        return;
    }

    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("hello.py"), "print(\"Hello World\")\n").unwrap();

    let spec = create_session(project.path(), true).expect("create_session");
    let mut state = AppState::new();
    state.set_project_root(spec.project_root.clone());
    let state = Arc::new(Mutex::new(state));
    let sink: Arc<dyn EventSink> = Arc::new(CapturingSink::default());

    let _watcher = SandboxWatcher::spawn(spec.clone(), Arc::clone(&state), SelfWrites::new(), sink)
        .expect("watcher spawn");

    // Exactly the user's real command: append inside the sandbox.
    let mut argv = bwrap_argv(&spec);
    argv.push("sh".into());
    argv.push("-c".into());
    argv.push(r#"printf '\nprint("amen epic")\n' >> hello.py"#.into());
    let out = StdCommand::new("bwrap").args(&argv).output().expect("run sandboxed append");
    assert!(out.status.success(), "sandboxed append failed: {}", String::from_utf8_lossy(&out.stderr));

    let expected = "print(\"Hello World\")\n\nprint(\"amen epic\")\n";
    let work_file = spec.work_dir.join("hello.py");
    let real_file = spec.project_root.join("hello.py");

    let mirrored = wait_for(|| std::fs::read_to_string(&real_file).ok().as_deref() == Some(expected), Duration::from_secs(5));
    assert!(mirrored, "watcher did not mirror the append into the real tree in time");

    // The critical check: from *inside* the sandbox (work_dir), content
    // must be exactly what was written — not doubled.
    assert_eq!(std::fs::read_to_string(&work_file).unwrap(), expected, "work_dir content must not be duplicated");

    // Let several more debounce cycles pass with nothing changing — a
    // redundant tick over an already-stable file must be a no-op on both
    // sides, not re-apply or duplicate anything.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(std::fs::read_to_string(&work_file).unwrap(), expected, "work_dir content changed after idle ticks with no further edits");
    assert_eq!(std::fs::read_to_string(&real_file).unwrap(), expected, "real tree content changed after idle ticks with no further edits");
    {
        let s = state.lock().unwrap();
        assert_eq!(s.get_content(&PathBuf::from("hello.py")).unwrap(), expected, "AppState buffer content changed after idle ticks with no further edits");
    }

    let _ = destroy_session(&spec);
}
