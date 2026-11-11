//! What a markdown text mentions that can be opened: the files it names in code spans, and
//! the requirements it links to. Found in the text, never guessed: a span only where the
//! path is a file of the project, so prose that happens to look like a path stays prose.

use std::collections::BTreeSet;

use crate::surface::view::{Buffer, Role, Span};

/// The directory a document lives in, `""` for the project root.
fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `target` as a path from the project root, resolved from the directory of the document.
fn resolved(document: &str, target: &str) -> String {
    let mut parts: Vec<&str> = directory(document).split('/').filter(|p| !p.is_empty()).collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The spans a markdown `text` of the file `document` holds that open something:
/// a code span naming a file of `files` opens it, and `[REQ-X](REQ-X.md)`, a link whose
/// words are the name of the document it points at, opens that requirement.
///
/// @implements REQ-VIEW.markdown_mentions_act
pub fn spans(document: &str, text: &str, files: &BTreeSet<String>) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut fenced = false;
    let mut line_start = 0usize;
    let mut at = 0usize;
    while at <= chars.len() {
        let end = chars[at..].iter().position(|c| *c == '\n').map_or(chars.len(), |n| at + n);
        let line: String = chars[at..end].iter().collect();
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced {
            line_spans(document, &line, at, files, &mut out);
        }
        line_start = end + 1;
        at = line_start;
    }
    let _ = line_start;
    out
}

fn line_spans(document: &str, line: &str, base: usize, files: &BTreeSet<String>, out: &mut Vec<Span>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '`' => {
                let Some(close) = chars[i + 1..].iter().position(|c| *c == '`') else { return };
                let inner: String = chars[i + 1..i + 1 + close].iter().collect();
                let named = inner.rsplit_once(':').filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit())).map_or(inner.as_str(), |(p, _)| p);
                if files.contains(named) && !inner.is_empty() {
                    out.push(Span {
                        start: base + i + 1,
                        stop: base + i + 1 + inner.chars().count(),
                        role: Role::Path,
                        actions: vec!["file.open".into()],
                    });
                }
                i += close + 2;
            }
            '[' => {
                let rest: String = chars[i..].iter().collect();
                let label_end = rest.find("](");
                let target_end = label_end.and_then(|l| rest[l + 2..].find(')').map(|t| (l, l + 2 + t)));
                if let Some((l, t)) = target_end {
                    let label = &rest[1..l];
                    let target = &rest[l + 2..t];
                    let path = resolved(document, target);
                    let stem = path.rsplit_once('/').map_or(path.as_str(), |(_, f)| f).trim_end_matches(".md");
                    if files.contains(&path) && path.ends_with(".md") && stem == label {
                        let label_start = base + i + 1;
                        out.push(Span {
                            start: label_start,
                            stop: label_start + label.chars().count(),
                            role: Role::Requirement,
                            actions: vec!["trace.requirement".into()],
                        });
                    }
                    i += rest[..t + 1].chars().count();
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
}

/// `buffer` with the mentions its text holds made to act. Where one lies over a span the
/// text already had (the highlight of a code span), it replaces that span there.
///
/// @implements REQ-VIEW.markdown_mentions_act
pub fn linked(buffer: Buffer, document: &str, files: &BTreeSet<String>) -> Buffer {
    let extra = spans(document, &buffer.text, files);
    if extra.is_empty() {
        return buffer;
    }
    let mut kept: Vec<Span> = buffer
        .spans
        .iter()
        .filter(|span| !extra.iter().any(|e| span.start < e.stop && e.start < span.stop))
        .cloned()
        .collect();
    kept.extend(extra);
    let size = buffer.text.chars().count();
    Buffer { spans: crate::surface::produce::tidy(size, kept), ..buffer }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> BTreeSet<String> {
        ["src/celsius.rs", "reqs/REQ-THERMO.md", "reqs/REQ-TABLE.md", "tests/round_trip.rs"].iter().map(|s| s.to_string()).collect()
    }

    fn words(text: &str, spans: &[Span]) -> Vec<(String, String)> {
        let chars: Vec<char> = text.chars().collect();
        spans.iter().map(|s| (chars[s.start..s.stop].iter().collect(), s.actions[0].clone())).collect()
    }

    /// @tests REQ-VIEW.markdown_mentions_act
    #[test]
    fn a_code_span_naming_a_project_file_opens_it_and_other_code_does_not() {
        let text = "Done in `src/celsius.rs`, with `tests/round_trip.rs:12`, not `src/nowhere.rs` or `x + 1`.\n```\n`src/celsius.rs`\n```\n";
        let found = words(text, &spans("reqs/REQ-X.md", text, &files()));
        assert_eq!(
            found,
            vec![
                ("src/celsius.rs".to_string(), "file.open".to_string()),
                ("tests/round_trip.rs:12".to_string(), "file.open".to_string()),
            ],
            "a fenced block or an unknown path acted"
        );
    }

    /// @tests REQ-VIEW.markdown_mentions_act
    #[test]
    fn a_link_to_another_requirement_by_its_own_name_opens_that_requirement() {
        let text = "See [REQ-THERMO](REQ-THERMO.md) and [the table](REQ-TABLE.md) and [REQ-NO](REQ-NO.md).";
        let found = words(text, &spans("reqs/REQ-X.md", text, &files()));
        assert_eq!(found, vec![("REQ-THERMO".to_string(), "trace.requirement".to_string())]);
    }

    /// @tests REQ-VIEW.markdown_mentions_act
    #[test]
    fn a_mention_replaces_the_highlight_it_lies_over_and_leaves_a_well_formed_buffer() {
        let text = "Run `src/celsius.rs` now.\n".to_string();
        let marked = Buffer {
            id: "file:reqs/a.md".into(),
            kind: crate::surface::view::BufferKind::File { path: "reqs/a.md".into() },
            spans: vec![Span { start: 4, stop: 22, role: Role::Plain, actions: vec![] }],
            text,
        };
        let shown = linked(marked, "reqs/a.md", &files());
        assert!(crate::surface::view::faults(shown.clone()).is_empty());
        assert!(shown.spans.iter().any(|s| s.actions == vec!["file.open".to_string()]));
        assert!(!shown.spans.iter().any(|s| s.role == Role::Plain && s.start == 4), "the highlight stayed over it");
    }
}
