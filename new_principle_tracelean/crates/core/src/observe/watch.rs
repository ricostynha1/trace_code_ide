//! Reading what a sandboxed agent left behind.
//!
//! A shell, and a thin one: two reads and no decisions. What the transcript
//! means is `transcript::read`'s answer and what the usage cost is `cost`'s,
//! both of them functions of the bytes this hands over
//! ([ARCH-CORE-SHELL](../../../reqs/arch/ARCH-CORE-SHELL.md)).
//!
//! Neither file being there is the ordinary case rather than an error. A tool
//! that writes no transcript is fully supported — the workspace diff is the
//! truth about what happened either way
//! (`REQ-TRANSCRIPT.absent_is_fine`) — and a missing price table leaves every
//! model unpriced, which the estimate then says.

use std::path::Path;

use crate::observe::cost::Price;
use crate::observe::transcript::{self, Read};

/// Where a tool leaves its own session record, if it leaves one.
pub const TRANSCRIPT: &str = ".tracelean/agent/transcript.jsonl";

/// What a token of each kind costs, per model, as this project records it.
pub const PRICES: &str = ".tracelean/prices.json";

/// The tool's own account of what it did.
///
/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn transcript_of(root: &Path) -> Read {
    transcript::read(std::fs::read_to_string(root.join(TRANSCRIPT)).unwrap_or_default())
}

/// Where Claude Code keeps its state: `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
fn claude_home() -> Option<std::path::PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".claude")))
}

/// The newest Claude Code transcript of this project written since `since`,
/// as text — empty when there is none.
///
/// Claude Code files a session under the directory it was started in, and a
/// sandbox mounts the copy at the project's own path, so the project's name is
/// where to look. Only transcripts touched since the session began are the
/// session's: an older one is a conversation from before it.
///
/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn claude_transcript(root: &Path, since: std::time::SystemTime) -> String {
    let Some(home) = claude_home() else { return String::new() };
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let slug = crate::observe::sandbox::claude_slug(&canonical.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(home.join("projects").join(slug)) else {
        return String::new();
    };
    let newest = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(modified, _)| *modified >= since)
        .max_by_key(|(modified, _)| *modified);
    match newest {
        Some((_, path)) => std::fs::read_to_string(path).unwrap_or_default(),
        None => String::new(),
    }
}

/// The price table.
///
/// A file rather than a constant, because it is a fact about somebody else's
/// pricing and it goes out of date.
///
/// @implements REQ-COST.price_is_per_model
pub fn table_of(root: &Path) -> Vec<Price> {
    let text = std::fs::read_to_string(root.join(PRICES)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}
