//! Git Patch Edit — apply unified diff patches to source files.
//!
//! Produces shadow buffer with patch applied, then diffs → Insert/Delete only.
//! Fails atomically if patch doesn't apply cleanly.

use crate::surgical_edit::error::SurgicalEditError;
use crate::surgical_edit::shadow_diff::diff_to_commands;
use crate::surgical_edit::EditResult;
use std::path::PathBuf;

/// A parsed unified diff hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchHunk {
    pub orig_start: Option<usize>,
    pub lines: Vec<PatchLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchLine {
    Context(String),
    Add(String),
    Remove(String),
}

/// A complete patch (one or more hunks) for a single file.
#[derive(Debug, Clone)]
pub struct PatchEdit {
    pub hunks: Vec<PatchHunk>,
}

impl PatchEdit {
    /// Parse a unified diff string into a PatchEdit.
    pub fn parse(patch_text: &str) -> Result<Self, SurgicalEditError> {
        let mut hunks = Vec::new();
        let mut current_lines: Vec<PatchLine> = Vec::new();
        let mut current_start: Option<usize> = None;

        for line in patch_text.lines() {
            if line.starts_with("@@") {
                if !current_lines.is_empty() {
                    hunks.push(PatchHunk {
                        orig_start: current_start,
                        lines: std::mem::take(&mut current_lines),
                    });
                }
                current_start = parse_hunk_header(line);
            } else if let Some(rest) = line.strip_prefix('+') {
                current_lines.push(PatchLine::Add(rest.to_string()));
            } else if let Some(rest) = line.strip_prefix('-') {
                current_lines.push(PatchLine::Remove(rest.to_string()));
            } else if let Some(rest) = line.strip_prefix(' ') {
                current_lines.push(PatchLine::Context(rest.to_string()));
            } else if line.is_empty() {
                current_lines.push(PatchLine::Context(String::new()));
            } else if !line.starts_with("---") && !line.starts_with("+++") && !line.starts_with("diff") {
                current_lines.push(PatchLine::Context(line.to_string()));
            }
        }

        if !current_lines.is_empty() {
            hunks.push(PatchHunk {
                orig_start: current_start,
                lines: current_lines,
            });
        }

        if hunks.is_empty() {
            return Err(SurgicalEditError::InvalidPatch("No hunks found in patch".into()));
        }

        Ok(Self { hunks })
    }

    /// Apply this patch to source → shadow buffer → diff → Insert/Delete commands.
    pub fn apply(
        &self,
        file: &PathBuf,
        source: &str,
    ) -> Result<EditResult, SurgicalEditError> {
        let source_lines: Vec<&str> = source.lines().collect();
        let mut result_lines: Vec<String> = source_lines.iter().map(|s| s.to_string()).collect();

        // Locate and sort hunks by position (descending for bottom-up application)
        let mut sorted_hunks: Vec<(usize, &PatchHunk)> = Vec::new();
        for hunk in &self.hunks {
            let start = if let Some(s) = hunk.orig_start {
                s.saturating_sub(1)
            } else {
                find_hunk_position(&source_lines, hunk)?
            };
            sorted_hunks.push((start, hunk));
        }
        sorted_hunks.sort_by(|a, b| b.0.cmp(&a.0));

        for (start, hunk) in &sorted_hunks {
            result_lines = apply_single_hunk(&result_lines, *start, hunk)?;
        }

        // Build shadow buffer
        let shadow = if source.ends_with('\n') {
            result_lines.join("\n") + "\n"
        } else {
            result_lines.join("\n")
        };

        // Diff → Insert/Delete commands
        let commands = diff_to_commands(file, source, &shadow);

        Ok(EditResult {
            file: file.clone(),
            commands,
            new_content: shadow,
        })
    }
}

fn parse_hunk_header(line: &str) -> Option<usize> {
    let after_at = line.trim_start_matches('@').trim();
    if let Some(dash_part) = after_at.strip_prefix('-') {
        let num_str = dash_part.split(&[',', ' ', '+'][..]).next()?;
        num_str.parse::<usize>().ok()
    } else {
        None
    }
}

fn find_hunk_position(source_lines: &[&str], hunk: &PatchHunk) -> Result<usize, SurgicalEditError> {
    let context_and_remove: Vec<&str> = hunk
        .lines
        .iter()
        .filter_map(|l| match l {
            PatchLine::Context(s) => Some(s.as_str()),
            PatchLine::Remove(s) => Some(s.as_str()),
            PatchLine::Add(_) => None,
        })
        .collect();

    if context_and_remove.is_empty() {
        return Ok(0);
    }

    let pattern_len = context_and_remove.len();
    let mut found_at: Option<usize> = None;

    for start in 0..=source_lines.len().saturating_sub(pattern_len) {
        let matches = context_and_remove
            .iter()
            .enumerate()
            .all(|(i, ctx)| start + i < source_lines.len() && source_lines[start + i] == *ctx);

        if matches {
            if found_at.is_some() {
                return Err(SurgicalEditError::PatchFailed(
                    "Context matches at multiple positions — ambiguous patch".into(),
                ));
            }
            found_at = Some(start);
        }
    }

    found_at.ok_or_else(|| {
        SurgicalEditError::PatchFailed("Context lines do not match any position in source".into())
    })
}

fn apply_single_hunk(
    lines: &[String],
    start: usize,
    hunk: &PatchHunk,
) -> Result<Vec<String>, SurgicalEditError> {
    let mut result = Vec::new();
    let mut src_idx = 0;

    while src_idx < start {
        result.push(lines[src_idx].clone());
        src_idx += 1;
    }

    for patch_line in &hunk.lines {
        match patch_line {
            PatchLine::Context(expected) => {
                if src_idx >= lines.len() {
                    return Err(SurgicalEditError::PatchFailed(format!(
                        "Context line past end of file: '{}'", expected
                    )));
                }
                if lines[src_idx] != *expected {
                    return Err(SurgicalEditError::PatchFailed(format!(
                        "Context mismatch at line {}: expected '{}', got '{}'",
                        src_idx + 1, expected, lines[src_idx]
                    )));
                }
                result.push(lines[src_idx].clone());
                src_idx += 1;
            }
            PatchLine::Remove(expected) => {
                if src_idx >= lines.len() {
                    return Err(SurgicalEditError::PatchFailed(format!(
                        "Remove line past end of file: '{}'", expected
                    )));
                }
                if lines[src_idx] != *expected {
                    return Err(SurgicalEditError::PatchFailed(format!(
                        "Remove mismatch at line {}: expected '{}', got '{}'",
                        src_idx + 1, expected, lines[src_idx]
                    )));
                }
                src_idx += 1;
            }
            PatchLine::Add(text) => {
                result.push(text.clone());
            }
        }
    }

    while src_idx < lines.len() {
        result.push(lines[src_idx].clone());
        src_idx += 1;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;

    fn assert_only_replace(commands: &[Command]) {
        for cmd in commands {
            match cmd {
                Command::Replace { .. } => {}
                other => panic!("Expected only Replace, got: {:?}", other),
            }
        }
    }

    #[test]
    fn test_simple_add_line() {
        let source = "pub enum UploadError {\n    EmptyFilename,\n    TooLarge,\n    IoError(String),\n}\n";
        let patch = "@@\n pub enum UploadError {\n     EmptyFilename,\n     TooLarge,\n     IoError(String),\n }\n+\n+impl UploadError {}\n";
        let edit = PatchEdit::parse(patch).unwrap();
        let result = edit.apply(&PathBuf::from("test.rs"), source).unwrap();
        assert_only_replace(&result.commands);
        assert!(result.new_content.contains("impl UploadError {}"));
    }

    #[test]
    fn test_remove_and_add() {
        let source = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n";
        let patch = "@@\n fn main() {\n-    let x = 1;\n+    let x = 42;\n     let y = 2;\n }\n";
        let edit = PatchEdit::parse(patch).unwrap();
        let result = edit.apply(&PathBuf::from("test.rs"), source).unwrap();
        assert_only_replace(&result.commands);
        assert!(result.new_content.contains("let x = 42;"));
        assert!(!result.new_content.contains("let x = 1;"));
    }

    #[test]
    fn test_context_mismatch_fails() {
        let source = "fn main() {\n    let x = 1;\n}\n";
        let patch = "@@\n fn main() {\n     let x = 999;\n }\n";
        let edit = PatchEdit::parse(patch).unwrap();
        let result = edit.apply(&PathBuf::from("test.rs"), source);
        assert!(matches!(result, Err(SurgicalEditError::PatchFailed(_))));
    }

    #[test]
    fn test_empty_patch_fails() {
        let result = PatchEdit::parse("");
        assert!(matches!(result, Err(SurgicalEditError::InvalidPatch(_))));
    }

    #[test]
    fn test_auto_detect_position() {
        let source = "line1\nline2\nline3\nline4\n";
        let patch = " line2\n line3\n+inserted\n line4\n";
        let edit = PatchEdit::parse(patch).unwrap();
        let result = edit.apply(&PathBuf::from("test.rs"), source).unwrap();
        assert_only_replace(&result.commands);
        assert!(result.new_content.contains("line3\ninserted\nline4"));
    }
}
