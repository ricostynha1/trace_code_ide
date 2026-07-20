//! CancelToken — hard-stop signal for an in-flight agent run (T12).
//!
//! A cooperative pause (PauseHandler) only takes effect at tool-call
//! boundaries every N calls; this token is checked at the top of every loop
//! iteration, raced against the in-flight LLM request, and polled by
//! `run_shell` while a child process runs — so "Stop" actually stops.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Cheaply clonable cancellation token. All clones share the same flag.
///
/// The token is reused across runs: the caller starting a new run must
/// `reset()` it first, otherwise a stale Stop from the previous run would
/// abort the new one immediately.
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Signal cancellation. Idempotent.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Clear the flag before starting a new run.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    /// Resolve once the token is cancelled. Polling-based (100ms) — used to
    /// race the in-flight provider request in a `tokio::select!`, where a
    /// tenth-of-a-second abort latency is more than fast enough.
    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Shared handle to the raw flag, for synchronous consumers that can't
    /// hold the token type (e.g. the shell executor's child-process poll loop).
    pub fn flag_handle(&self) -> Arc<AtomicBool> {
        self.flag.clone()
    }
}
