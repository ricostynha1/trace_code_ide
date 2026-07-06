//! AI → shadow buffer → diff pipeline.
//! AI output lands in temp buffer. User sees diff view with accept/reject per-hunk.
//! Accepted hunks become Commands (undoable, attributed to AI agent).

use crate::commands::Command;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single diff hunk between original and proposed content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub id: String,
    /// Start line in original file (0-indexed)
    pub original_start: usize,
    /// Number of lines from original
    pub original_count: usize,
    /// Start line in proposed file (0-indexed)
    pub proposed_start: usize,
    /// Number of lines from proposed
    pub proposed_count: usize,
    /// Original lines
    pub original_lines: Vec<String>,
    /// Proposed lines
    pub proposed_lines: Vec<String>,
    /// Whether user has accepted this hunk
    pub accepted: bool,
}

/// A pending AI diff for a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDiff {
    pub id: String,
    pub file: String,
    /// Original content
    pub original: String,
    /// AI-proposed content
    pub proposed: String,
    /// Computed hunks
    pub hunks: Vec<DiffHunk>,
    /// Agent that produced this diff
    pub agent: String,
    /// Timestamp
    pub timestamp: String,
}

/// Compute diff hunks between original and proposed content.
pub fn compute_diff(original: &str, proposed: &str) -> Vec<DiffHunk> {
    let orig_lines: Vec<&str> = original.lines().collect();
    let prop_lines: Vec<&str> = proposed.lines().collect();

    let mut hunks = Vec::new();
    let lcs = longest_common_subsequence(&orig_lines, &prop_lines);

    let mut oi = 0usize;
    let mut pi = 0usize;
    let mut li = 0usize; // index into lcs

    while oi < orig_lines.len() || pi < prop_lines.len() {
        // Skip matching lines
        if li < lcs.len() && oi < orig_lines.len() && pi < prop_lines.len()
            && orig_lines[oi] == lcs[li] && prop_lines[pi] == lcs[li]
        {
            oi += 1;
            pi += 1;
            li += 1;
            continue;
        }

        // Collect a hunk of differing lines
        let hunk_orig_start = oi;
        let hunk_prop_start = pi;
        let mut orig_hunk: Vec<String> = Vec::new();
        let mut prop_hunk: Vec<String> = Vec::new();

        // Collect original lines not in LCS
        while oi < orig_lines.len() && (li >= lcs.len() || orig_lines[oi] != lcs[li]) {
            orig_hunk.push(orig_lines[oi].to_string());
            oi += 1;
        }

        // Collect proposed lines not in LCS
        while pi < prop_lines.len() && (li >= lcs.len() || prop_lines[pi] != lcs[li]) {
            prop_hunk.push(prop_lines[pi].to_string());
            pi += 1;
        }

        if !orig_hunk.is_empty() || !prop_hunk.is_empty() {
            hunks.push(DiffHunk {
                id: uuid::Uuid::new_v4().to_string(),
                original_start: hunk_orig_start,
                original_count: orig_hunk.len(),
                proposed_start: hunk_prop_start,
                proposed_count: prop_hunk.len(),
                original_lines: orig_hunk,
                proposed_lines: prop_hunk,
                accepted: false,
            });
        }
    }

    hunks
}

/// Create a PendingDiff from original content and AI-proposed content.
pub fn create_pending_diff(
    file: &str,
    original: &str,
    proposed: &str,
    agent: &str,
) -> PendingDiff {
    let hunks = compute_diff(original, proposed);
    PendingDiff {
        id: uuid::Uuid::new_v4().to_string(),
        file: file.to_string(),
        original: original.to_string(),
        proposed: proposed.to_string(),
        hunks,
        agent: agent.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    }
}

/// Apply accepted hunks as a batch of Commands.
/// Returns the Commands to execute (caller applies them to state).
pub fn accepted_hunks_to_commands(diff: &PendingDiff) -> Vec<Command> {
    let file_path = PathBuf::from(&diff.file);
    let orig_lines: Vec<&str> = diff.original.lines().collect();

    // Rebuild content with accepted hunks applied
    let mut result_lines: Vec<String> = Vec::new();
    let mut orig_idx = 0usize;

    for hunk in &diff.hunks {
        if !hunk.accepted {
            // Keep original lines for this range
            while orig_idx < hunk.original_start + hunk.original_count && orig_idx < orig_lines.len() {
                result_lines.push(orig_lines[orig_idx].to_string());
                orig_idx += 1;
            }
        } else {
            // Copy lines before hunk
            while orig_idx < hunk.original_start && orig_idx < orig_lines.len() {
                result_lines.push(orig_lines[orig_idx].to_string());
                orig_idx += 1;
            }
            // Replace with proposed lines
            result_lines.extend(hunk.proposed_lines.clone());
            orig_idx = hunk.original_start + hunk.original_count;
        }
    }

    // Remaining original lines after last hunk
    while orig_idx < orig_lines.len() {
        result_lines.push(orig_lines[orig_idx].to_string());
        orig_idx += 1;
    }

    let new_content = result_lines.join("\n");

    // Emit as a Delete+Insert batch (attributed to AI agent)
    vec![Command::replace(file_path, 0, diff.original.clone(), new_content)]
}

/// Simple LCS for line-based diff.
fn longest_common_subsequence<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<&'a str> {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0u32; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack
    let mut result = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            result.push(a[i - 1]);
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] > dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    result.reverse();
    result
}
