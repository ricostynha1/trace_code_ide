//! Response parser — extract code blocks, JSON, handle truncation/continuation.

use super::templates::CodeBlock;
use serde::{Deserialize, Serialize};

/// Parsed AI response with structured content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedResponse {
    /// Raw response text
    pub raw: String,
    /// Extracted code blocks by language
    pub code_blocks: Vec<CodeBlock>,
    /// Extracted JSON objects (if any)
    pub json_blocks: Vec<serde_json::Value>,
    /// Whether the response appears truncated
    pub appears_truncated: bool,
    /// Plain text sections (outside code blocks)
    pub text_sections: Vec<String>,
}

/// Parse an AI response, extracting structured content.
pub fn parse_response(content: &str) -> ParsedResponse {
    let code_blocks = super::templates::extract_code_blocks(content);
    let json_blocks = extract_json_blocks(content);
    let appears_truncated = detect_truncation(content);
    let text_sections = extract_text_sections(content);

    ParsedResponse {
        raw: content.to_string(),
        code_blocks,
        json_blocks,
        appears_truncated,
        text_sections,
    }
}

/// Extract JSON blocks from response (fenced ```json or bare { } at top level).
fn extract_json_blocks(content: &str) -> Vec<serde_json::Value> {
    let mut blocks = Vec::new();

    // From fenced code blocks tagged "json"
    let mut in_block = false;
    let mut is_json = false;
    let mut current = String::new();

    for line in content.lines() {
        if line.starts_with("```") && !in_block {
            in_block = true;
            is_json = line.trim_start_matches('`').trim().eq_ignore_ascii_case("json");
            current.clear();
        } else if line == "```" && in_block {
            in_block = false;
            if is_json {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&current) {
                    blocks.push(val);
                }
            }
        } else if in_block && is_json {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    // Also try to parse bare JSON objects that span multiple lines
    // (simple heuristic: lines starting with { and ending with } at same nesting)
    if blocks.is_empty() {
        if let Some(start) = content.find('{') {
            if let Some(json_str) = extract_balanced_braces(&content[start..]) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    blocks.push(val);
                }
            }
        }
    }

    blocks
}

/// Extract a balanced {} string from the start of input.
fn extract_balanced_braces(input: &str) -> Option<&str> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape = false;

    for (i, ch) in input.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&input[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Detect if response looks truncated (cut off mid-sentence/code).
fn detect_truncation(content: &str) -> bool {
    let trimmed = content.trim_end();
    if trimmed.is_empty() {
        return false;
    }

    // Check for unclosed code blocks
    let backtick_count = content.matches("```").count();
    if backtick_count % 2 != 0 {
        return true;
    }

    // Check if ends mid-sentence (no terminal punctuation)
    let last_char = trimmed.chars().last().unwrap_or('.');
    if !matches!(last_char, '.' | '!' | '?' | '}' | ']' | ')' | '`' | '"' | '\'' | ':' | ';') {
        // Could be truncated, but also could just be code
        // More confident if it ends mid-word
        if last_char.is_alphanumeric() {
            let last_line = trimmed.lines().last().unwrap_or("");
            // If last line looks like mid-sentence
            if !last_line.trim().is_empty() && last_line.len() > 10 {
                return true;
            }
        }
    }

    false
}

/// Build a continuation prompt for truncated responses.
pub fn continuation_prompt() -> &'static str {
    "Your previous response was cut off. Please continue from where you left off."
}

/// Extract text sections (content outside code blocks).
fn extract_text_sections(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current = String::new();
    let mut in_block = false;

    for line in content.lines() {
        if line.starts_with("```") {
            if !in_block {
                if !current.trim().is_empty() {
                    sections.push(current.trim().to_string());
                }
                current.clear();
                in_block = true;
            } else {
                in_block = false;
            }
        } else if !in_block {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.trim().is_empty() {
        sections.push(current.trim().to_string());
    }

    sections
}
