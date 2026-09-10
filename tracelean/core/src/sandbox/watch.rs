//! Live watcher tying a session's work dir to `AppState`: on a quiescent
//! burst of writes, diff the work dir against the real tree and mirror the
//! result in — buffers, undo tree, file tree — the same way
//! `ai::tool_executor::materialize_sandbox_run` does for the agent's own
//! sandboxed shell commands.

use super::diff::collect_tree_mutations;
use super::mirror::{apply_mutations, SelfWrites};
use super::session::SessionSpec;
use crate::state::AppState;
use crate::EventSink;
use notify::{RecursiveMode, Watcher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long to wait for the burst to go quiet before acting — one
/// `Command::Batch` per burst, not one per file write.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Owns the background watcher thread for one session. Dropping it stops
/// the thread; the underlying `notify` watcher stops with it.
pub struct SandboxWatcher {
    stop: Arc<AtomicBool>,
    _watcher: notify::RecommendedWatcher,
}

impl SandboxWatcher {
    /// Start watching `spec.work_dir`. Bursts of changes are diffed against
    /// `spec.project_root` and mirrored into `state` + the real tree.
    /// `self_writes` is shared with the IDE-save mirror
    /// (`service::save_file`) so echoes of tracelean's own writes are
    /// dropped instead of bouncing back into the undo tree.
    pub fn spawn(
        spec: SessionSpec,
        state: Arc<Mutex<AppState>>,
        self_writes: SelfWrites,
        event_sink: Arc<dyn EventSink>,
    ) -> Result<Self, String> {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(tx).map_err(|e| e.to_string())?;
        watcher
            .watch(&spec.work_dir, RecursiveMode::Recursive)
            .map_err(|e| e.to_string())?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let session_id = spec.id.clone();

        std::thread::spawn(move || loop {
            if stop_thread.load(Ordering::SeqCst) {
                return;
            }
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(_) => {
                    // Drain the rest of the burst before acting.
                    while rx.recv_timeout(DEBOUNCE).is_ok() {}
                    if stop_thread.load(Ordering::SeqCst) {
                        return;
                    }
                    tick(&spec, &state, &self_writes, &event_sink, &session_id);
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        });

        Ok(Self { stop, _watcher: watcher })
    }
}

fn tick(
    spec: &SessionSpec,
    state: &Arc<Mutex<AppState>>,
    self_writes: &SelfWrites,
    event_sink: &Arc<dyn EventSink>,
    session_id: &str,
) {
    let mutations: Vec<_> = collect_tree_mutations(&spec.work_dir, &spec.project_root)
        .into_iter()
        .filter(|m| !self_writes.is_echo(&m.path, m.post.as_deref().unwrap_or("")))
        .collect();
    if mutations.is_empty() {
        return;
    }
    let changed: Vec<String> = mutations.iter().map(|m| m.path.to_string_lossy().to_string()).collect();

    let result = match state.lock() {
        Ok(mut s) => apply_mutations(&mutations, &spec.project_root, &mut s),
        Err(_) => return,
    };

    match result {
        Ok(_) => {
            event_sink.emit("undo-tree-changed", "");
            event_sink.emit("files-changed", "");
            event_sink.emit(
                "sandbox-changed",
                &serde_json::json!({ "session_id": session_id, "paths": changed }).to_string(),
            );
        }
        Err(e) => {
            event_sink.emit(
                "sandbox-changed",
                &serde_json::json!({ "session_id": session_id, "error": e }).to_string(),
            );
        }
    }
}

impl Drop for SandboxWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
