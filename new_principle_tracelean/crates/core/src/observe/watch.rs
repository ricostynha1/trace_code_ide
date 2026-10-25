use std::path::Path;

use crate::observe::cost::Price;
use crate::observe::transcript::{self, Read};

pub const TRANSCRIPT: &str = ".tracelean/agent/transcript.jsonl";

pub const PRICES: &str = ".tracelean/prices.json";

/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn transcript_of(root: &Path) -> Read {
    transcript::read(std::fs::read_to_string(root.join(TRANSCRIPT)).unwrap_or_default())
}

fn claude_home() -> Option<std::path::PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".claude")))
}

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

/// @implements REQ-COST.price_is_per_model
pub fn table_of(root: &Path) -> Vec<Price> {
    let text = std::fs::read_to_string(root.join(PRICES)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}
