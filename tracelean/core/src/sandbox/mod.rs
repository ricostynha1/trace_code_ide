//! Sandboxed session workspace — lets the user run an *external* tool (most
//! notably Claude Code) inside a bubblewrap namespace bound to a reflink
//! copy of the project, while tracelean watches the copy and mirrors its
//! effects into `AppState` (buffers, file tree, undo tree) the same way
//! `ai::shell_sandbox` mirrors its own agent's shell commands.
//!
//! tracelean never launches, prompts, or otherwise drives the external
//! tool — the user runs it themselves in a plain shell (`tracelean-sandbox
//! shell ...`, see `core/src/bin/tracelean-sandbox.rs`). This module only
//! creates the workspace and observes it.
//!
//! Submodules:
//! - `session`   — `SessionSpec` lifecycle: create/destroy/list, `bwrap` argv.
//! - `diff`      — two-way tree diff between the work copy and the real tree.
//! - `mirror`    — turn a diff into `Command`s, apply to `AppState` + disk.
//! - `watch`     — filesystem watcher tying diff + mirror together live.
//! - `transcript`— tails a Claude Code session's own JSONL transcript.

pub mod cost;
pub mod diff;
pub mod mirror;
pub mod session;
pub mod transcript;
pub mod watch;

pub use diff::collect_tree_mutations;
pub use mirror::{apply_mutations, SandboxLink, SelfWrites};
pub use session::{SandboxCapabilities, SessionSpec};
pub use watch::SandboxWatcher;
pub use transcript::{TranscriptEvent, TranscriptTail};
