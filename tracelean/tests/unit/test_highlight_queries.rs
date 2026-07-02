//! Snapshot tests: known source → expected capture list, per language.
//! Verifies .scm query execution + theme map color resolution.

use std::path::Path;
use tracelean_lib::parser::{get_highlights_query, get_highlight_captures, resolve_capture_color, HighlightCapture};

// --- Helpers ---

/// Run captures on source with given extension, return sorted captures.
fn captures_for(ext: &str, source: &str) -> Vec<HighlightCapture> {
    let path = Path::new("test").with_extension(ext);
    get_highlight_captures(&path, source)
}

/// Run full highlight (captures + color resolution) on source.
fn highlights_for(ext: &str, source: &str) -> Vec<tracelean_lib::parser::HighlightSpan> {
    let path = Path::new("test").with_extension(ext);
    get_highlights_query(&path, source)
}

// --- Theme map tests ---

#[test]
fn theme_map_exact_match() {
    assert_eq!(resolve_capture_color("keyword"), Some("#c678dd"));
    assert_eq!(resolve_capture_color("string"), Some("#98c379"));
    assert_eq!(resolve_capture_color("comment"), Some("#5c6370"));
}

#[test]
fn theme_map_prefix_fallback() {
    // "function.call" matches exactly
    assert_eq!(resolve_capture_color("function.call"), Some("#61afef"));
    // "function.unknown" falls back to "function"
    assert_eq!(resolve_capture_color("function.unknown"), Some("#61afef"));
    // "string.escape" matches exactly
    assert_eq!(resolve_capture_color("string.escape"), Some("#56b6c2"));
}

#[test]
fn theme_map_no_match() {
    assert_eq!(resolve_capture_color("nonexistent.thing"), None);
}

// --- Rust snapshot ---

#[test]
fn rust_highlights_basic() {
    let src = r#"fn main() {
    let x = 42;
}"#;
    let caps = captures_for("rs", src);
    assert!(!caps.is_empty(), "should produce captures for Rust");

    // Must contain keyword captures for "fn" and "let"
    let keywords: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "keyword")
        .collect();
    assert!(keywords.len() >= 2, "expected at least 2 keywords (fn, let), got {}", keywords.len());

    // Must contain function capture for "main"
    let fns: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "function")
        .collect();
    assert!(!fns.is_empty(), "expected function capture for 'main'");

    // Must contain number capture for "42"
    let nums: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "number")
        .collect();
    assert!(!nums.is_empty(), "expected number capture for '42'");
}

#[test]
fn rust_highlights_colors_resolved() {
    let src = "fn hello() {}";
    let spans = highlights_for("rs", src);
    assert!(!spans.is_empty());
    // "fn" keyword should be purple
    let kw = spans.iter().find(|s| s.from == 0 && s.to == 2).unwrap();
    assert_eq!(kw.color, "#c678dd");
}

// --- Python snapshot ---

#[test]
fn python_highlights_basic() {
    let src = r#"def greet(name):
    return "Hello " + name
"#;
    let caps = captures_for("py", src);
    assert!(!caps.is_empty());

    let keywords: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "keyword")
        .collect();
    assert!(keywords.len() >= 2, "expected def + return keywords");

    let fns: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "function")
        .collect();
    assert!(!fns.is_empty(), "expected function capture for 'greet'");

    let strings: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "string")
        .collect();
    assert!(!strings.is_empty(), "expected string capture");
}

// --- C++ snapshot ---

#[test]
fn cpp_highlights_basic() {
    let src = r#"int main() {
    int x = 10;
    return x;
}"#;
    let caps = captures_for("cpp", src);
    assert!(!caps.is_empty());

    let types: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "type.builtin")
        .collect();
    assert!(!types.is_empty(), "expected type.builtin for 'int'");

    let keywords: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "keyword")
        .collect();
    assert!(!keywords.is_empty(), "expected keyword capture for 'return'");
}

// --- Lean 4 snapshot ---

#[test]
fn lean4_highlights_basic() {
    let src = r#"def hello : Nat := 42"#;
    let caps = captures_for("lean", src);
    assert!(!caps.is_empty());

    let keywords: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "keyword")
        .collect();
    assert!(!keywords.is_empty(), "expected keyword 'def'");

    let nums: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "number")
        .collect();
    assert!(!nums.is_empty(), "expected number '42'");
}

// --- Markdown snapshot (heading propagation test) ---

#[test]
fn markdown_heading_full_span() {
    let src = "# Hello World\n\nSome text.";
    let caps = captures_for("md", src);
    assert!(!caps.is_empty());

    // The full heading node should be captured with @markup.heading
    let headings: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "markup.heading")
        .collect();
    assert!(!headings.is_empty(), "expected @markup.heading capture");

    // The heading capture should span the entire "# Hello World" line
    let h = &headings[0];
    assert_eq!(h.from, 0);
    // Should cover at least "# Hello World" (14 bytes)
    assert!(h.to >= 13, "heading span should cover full heading text, got to={}", h.to);

    // Marker sub-capture
    let markers: Vec<_> = caps.iter()
        .filter(|c| c.capture_name == "markup.heading.marker")
        .collect();
    assert!(!markers.is_empty(), "expected @markup.heading.marker for '#'");
}

#[test]
fn markdown_heading_color_resolution() {
    let src = "## Title\n";
    let spans = highlights_for("md", src);
    assert!(!spans.is_empty());

    // heading color = #e06c75
    let heading_spans: Vec<_> = spans.iter()
        .filter(|s| s.color == "#e06c75")
        .collect();
    assert!(!heading_spans.is_empty(), "heading should resolve to #e06c75");
}

// --- Edge cases ---

#[test]
fn empty_file_no_crash() {
    let spans = highlights_for("rs", "");
    assert!(spans.is_empty());
}

#[test]
fn unknown_extension_no_crash() {
    let spans = highlights_for("xyz", "some content");
    assert!(spans.is_empty());
}

#[test]
fn unicode_content_char_offsets() {
    // Verify char offsets are correct with multi-byte chars
    let src = "fn héllo() {}";
    let spans = highlights_for("rs", src);
    assert!(!spans.is_empty());
    // "fn" is chars 0..2
    let kw = spans.iter().find(|s| s.from == 0 && s.to == 2);
    assert!(kw.is_some(), "keyword 'fn' should be at char offsets 0..2");
}
