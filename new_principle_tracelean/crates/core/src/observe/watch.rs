use std::path::Path;

use crate::observe::cost::Price;
use crate::observe::transcript::{self, Read, Usage};

pub const TRANSCRIPT: &str = ".tracelean/agent/transcript.jsonl";

pub const PRICES: &str = ".tracelean/prices.json";

/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn transcript_of(root: &Path) -> Read {
    transcript::read(std::fs::read_to_string(root.join(TRANSCRIPT)).unwrap_or_default())
}

/// Where the agent of this project's sandbox keeps its conversations: the configuration
/// the sandbox was started with, so the panel reads what that agent wrote.
fn claude_home(root: &Path) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok();
    crate::observe::workcopy::claude_config_named(root, home.as_deref())
        .map(std::path::PathBuf::from)
        .or_else(|| home.map(|home| Path::new(&home).join(".claude")))
}

/// @implements REQ-TRANSCRIPT.read_only
/// @implements REQ-TRANSCRIPT.absent_is_fine
pub fn claude_transcript(root: &Path, since: std::time::SystemTime) -> String {
    let Some(home) = claude_home(root) else { return String::new() };
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

const DEFAULT_PRICES: &str = include_str!("../../../../assets/prices.json");

/// The project's price file, then the shipped table for models it does not list.
/// @implements REQ-COST.price_is_per_model
pub fn table_of(root: &Path) -> Vec<Price> {
    let text = std::fs::read_to_string(root.join(PRICES)).unwrap_or_default();
    let mut table: Vec<Price> = serde_json::from_str(&text).unwrap_or_default();
    let shipped: Vec<Price> = serde_json::from_str(DEFAULT_PRICES).unwrap_or_default();
    for price in shipped {
        if !table.iter().any(|held| held.model == price.model) {
            table.push(price);
        }
    }
    table
}

/// A model the table does not list is priced as the newest listed model of the same
/// family (fable, opus, sonnet, haiku); a family with none stays unlisted, so it is named as
/// unpriced rather than counted as free.
pub fn priced_as(table: &[Price], usage: Vec<Usage>) -> Vec<Usage> {
    usage
        .into_iter()
        .map(|mut one| {
            if !table.iter().any(|price| price.model == one.model) {
                if let Some(family) = ["fable", "opus", "sonnet", "haiku"].iter().find(|f| one.model.contains(**f)) {
                    if let Some(price) = table.iter().rev().find(|price| price.model.contains(family)) {
                        one.model = price.model.clone();
                    }
                }
            }
            one
        })
        .collect()
}

/// "12.3k in · 40.1k cached · 2.0k out" over all usage, so a cost of 0 is never unexplained.
pub fn tokens_line(usage: &[Usage]) -> String {
    let sum = |f: fn(&Usage) -> u64| usage.iter().map(f).sum::<u64>();
    let k = |n: u64| if n >= 1000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() };
    format!(
        "tokens: {} in · {} cached read · {} cached written · {} out",
        k(sum(|u| u.input)),
        k(sum(|u| u.cached)),
        k(sum(|u| u.cache_write)),
        k(sum(|u| u.output))
    )
}
