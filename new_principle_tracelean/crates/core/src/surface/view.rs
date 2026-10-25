use serde::{Deserialize, Serialize};

/// @implements REQ-VIEW.everything_is_a_buffer
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BufferKind {
    File { path: String },
    Directory { path: String },
    Review { target: String },
    Menu { title: String },
    Record { title: String },
}

/// @implements REQ-VIEW.structure_over_text
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Plain,
    Path,
    Entry,
    Heading,
    Requirement,
    Mark { mark: crate::evidence::Mark, held: bool },
    Added,
    Removed,
    Token { kind: TokenKind },
    Claim { role: crate::trace::annotation::Role },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenKind {
    Keyword,
    String,
    Escape,
    Number,
    Constant,
    Comment,
    Type,
    Function,
    MacroCall,
    Operator,
    Property,
    Heading,
    Bold,
    Italic,
    Link,
}

/// @implements REQ-VIEW.affordances_named
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start: usize,
    pub stop: usize,
    pub role: Role,
    pub actions: Vec<String>,
}

pub fn action_name(action: &str) -> &str {
    action.split_once(' ').map_or(action, |(name, _)| name)
}

pub fn action_target(action: &str) -> Option<&str> {
    action.split_once(' ').map(|(_, target)| target)
}

/// @implements REQ-VIEW.one_representation
/// @implements REQ-VIEW.everything_is_a_buffer
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Buffer {
    pub id: String,
    pub kind: BufferKind,
    pub text: String,
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Fault {
    PastTheEnd { start: usize, stop: usize, length: usize },
    Backwards { start: usize, stop: usize },
    Overlap { first: usize, second: usize },
    Unordered { first: usize, second: usize },
}

fn faults_between(spans: &[Span]) -> Vec<Fault> {
    let mut out = Vec::new();
    for pair in spans.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if b.start < a.start {
            out.push(Fault::Unordered { first: a.start, second: b.start });
        } else if b.start < a.stop {
            out.push(Fault::Overlap { first: a.start, second: b.start });
        }
    }
    out
}

/// @implements REQ-VIEW.text_is_the_content
/// @implements REQ-VIEW.structure_over_text
/// @drt REQ-VIEW.text_is_the_content
/// @drt REQ-VIEW.structure_over_text
pub fn faults(buffer: Buffer) -> Vec<Fault> {
    let size = buffer.text.chars().count();
    let mut out = Vec::new();
    for span in &buffer.spans {
        if span.stop < span.start {
            out.push(Fault::Backwards { start: span.start, stop: span.stop });
        } else if size < span.stop {
            out.push(Fault::PastTheEnd { start: span.start, stop: span.stop, length: size });
        }
    }
    out.extend(faults_between(&buffer.spans));
    out
}

/// @implements REQ-VIEW.affordances_named
/// @implements REQ-VIEW.frontend_adds_nothing
/// @drt REQ-VIEW.affordances_named
pub fn actions_at(buffer: Buffer, offset: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for span in &buffer.spans {
        if span.start <= offset && offset < span.stop {
            for action in &span.actions {
                if !out.contains(action) {
                    out.push(action.clone());
                }
            }
        }
    }
    out
}

/// @implements REQ-VIEW.frontend_adds_nothing
pub fn declared_actions(buffer: &Buffer) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for span in &buffer.spans {
        for action in &span.actions {
            if !out.contains(action) {
                out.push(action.clone());
            }
        }
    }
    out.sort();
    out
}

/// @implements REQ-VIEW.rendering_is_total
/// @implements REQ-VIEW.text_is_the_content
/// @drt REQ-VIEW.rendering_is_total
pub fn plain_text(buffer: Buffer) -> Vec<String> {
    buffer.text.split('\n').map(str::to_string).collect()
}

fn listing_line(entry: &(usize, String)) -> String {
    format!("{}{}", " ".repeat(2 * entry.0), entry.1)
}

/// @implements REQ-VIEW.structure_has_one_type
/// @implements REQ-VIEW.everything_is_a_buffer
/// @implements REQ-SHOW.listing_from_entries
/// @drt REQ-VIEW.structure_has_one_type
/// @drt REQ-VIEW.everything_is_a_buffer
/// @drt REQ-SHOW.listing_from_entries
pub fn directory_buffer(path: String, entries: Vec<(usize, String)>) -> Buffer {
    let mut spans = Vec::new();
    let mut at = 0;
    for entry in &entries {
        let line = listing_line(entry);
        let length = line.chars().count();
        spans.push(Span {
            start: at + 2 * entry.0,
            stop: at + length,
            role: Role::Path,
            actions: vec!["file.open".to_string()],
        });
        at += length + 1;
    }
    Buffer {
        id: format!("dir:{path}"),
        kind: BufferKind::Directory { path },
        text: entries.iter().map(listing_line).collect::<Vec<_>>().join("\n"),
        spans,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub text: Option<String>,
    pub spans: Option<Vec<Span>>,
    pub kind: Option<BufferKind>,
}

/// @implements REQ-VIEW.changes_are_deltas
/// @drt REQ-VIEW.changes_are_deltas
pub fn delta(before: Buffer, after: Buffer) -> Delta {
    Delta {
        text: if before.text == after.text { None } else { Some(after.text) },
        spans: if before.spans == after.spans { None } else { Some(after.spans) },
        kind: if before.kind == after.kind { None } else { Some(after.kind) },
    }
}

/// @implements REQ-VIEW.changes_are_deltas
pub fn apply_delta(buffer: Buffer, d: Delta) -> Buffer {
    Buffer {
        id: buffer.id,
        kind: d.kind.unwrap_or(buffer.kind),
        text: d.text.unwrap_or(buffer.text),
        spans: d.spans.unwrap_or(buffer.spans),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rendering {
    pub lines: Vec<String>,
    pub offered: Vec<(usize, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Breach {
    LineDiffers { line: usize, shown: String, expected: String },
    LineMissing { line: usize, expected: String },
    LineExtra { line: usize, shown: String },
    ActionInvented { offset: usize, action: String },
    ActionDropped { offset: usize, action: String },
}

fn line_breaches(shown: &[String], expected: &[String]) -> Vec<Breach> {
    let mut out = Vec::new();
    for line in 0..shown.len().max(expected.len()) {
        match (shown.get(line), expected.get(line)) {
            (Some(a), Some(b)) if a != b => out.push(Breach::LineDiffers {
                line,
                shown: a.clone(),
                expected: b.clone(),
            }),
            (Some(_), Some(_)) => {}
            (Some(a), None) => out.push(Breach::LineExtra { line, shown: a.clone() }),
            (None, Some(b)) => out.push(Breach::LineMissing { line, expected: b.clone() }),
            (None, None) => {}
        }
    }
    out
}

/// @implements REQ-VIEW.rendering_is_total
/// @implements REQ-VIEW.frontend_adds_nothing
/// @implements REQ-VIEW.one_representation
/// @drt REQ-VIEW.frontend_adds_nothing
/// @drt REQ-VIEW.one_representation
pub fn conformance(buffer: Buffer, rendering: Rendering) -> Vec<Breach> {
    let mut out = line_breaches(&rendering.lines, &plain_text(buffer.clone()));
    for (offset, action) in &rendering.offered {
        if !actions_at(buffer.clone(), *offset).contains(action) {
            out.push(Breach::ActionInvented { offset: *offset, action: action.clone() });
        }
    }
    let mut positions: Vec<usize> = Vec::new();
    for (offset, _) in &rendering.offered {
        if !positions.contains(offset) {
            positions.push(*offset);
        }
    }
    for offset in positions {
        for action in actions_at(buffer.clone(), offset) {
            if !rendering.offered.iter().any(|(o, a)| *o == offset && *a == action) {
                out.push(Breach::ActionDropped { offset, action });
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presented {
    pub painted: String,
    pub name: String,
}

/// @implements REQ-VIEW.presentation_may_be_symbolic
/// @drt REQ-VIEW.presentation_may_be_symbolic
pub fn accessible(row: Vec<Presented>) -> String {
    row.into_iter().map(|piece| piece.name).collect()
}

/// @implements REQ-VIEW.presentation_may_be_symbolic
pub fn symbolic(row: Vec<Presented>) -> Vec<Presented> {
    row.into_iter().filter(|piece| piece.painted != piece.name).collect()
}

/// @implements REQ-VIEW.presentation_may_be_symbolic
/// @implements REQ-VIEW.screen_is_readable
/// @drt REQ-VIEW.screen_is_readable
pub fn presented_conformance(
    buffer: Buffer,
    rows: Vec<Vec<Presented>>,
    offered: Vec<(usize, String)>,
) -> Vec<Breach> {
    let lines = rows.into_iter().map(accessible).collect();
    conformance(buffer, Rendering { lines, offered })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing() -> Buffer {
        directory_buffer(
            "src".into(),
            vec![(0, "main.rs".into()), (1, "deep.rs".into()), (0, "lib.rs".into())],
        )
    }

    /// @tests REQ-VIEW.structure_has_one_type
    /// @tests REQ-VIEW.text_is_the_content
    #[test]
    fn a_listing_describes_its_own_text() {
        let buffer = listing();
        assert_eq!(buffer.text, "main.rs\n  deep.rs\nlib.rs");
        assert_eq!(faults(buffer.clone()), vec![]);
        let chars: Vec<char> = buffer.text.chars().collect();
        for span in &buffer.spans {
            let named: String = chars[span.start..span.stop].iter().collect();
            assert!(named.ends_with(".rs"), "a span covers `{named}`");
        }
    }

    /// @tests REQ-VIEW.affordances_named
    #[test]
    fn what_can_be_done_comes_from_the_span_under_the_cursor() {
        let buffer = listing();
        assert_eq!(actions_at(buffer.clone(), 0), vec!["file.open".to_string()]);
        assert_eq!(actions_at(buffer, 8), Vec::<String>::new());
    }

    /// @tests REQ-VIEW.text_is_the_content
    #[test]
    fn a_span_that_reaches_past_the_text_is_reported() {
        let mut buffer = listing();
        buffer.spans.push(Span {
            start: 0,
            stop: 9_000,
            role: Role::Plain,
            actions: vec![],
        });
        assert!(matches!(faults(buffer)[0], Fault::PastTheEnd { .. }));
    }

    /// @tests REQ-VIEW.changes_are_deltas
    #[test]
    fn a_delta_rebuilds_what_it_came_from() {
        let before = listing();
        let mut after = before.clone();
        after.text.push_str("\nextra.rs");
        after.spans.push(Span {
            start: 24,
            stop: 32,
            role: Role::Path,
            actions: vec!["file.open".into()],
        });
        assert_eq!(apply_delta(before.clone(), delta(before, after.clone())), after);
    }

    /// @tests REQ-VIEW.rendering_is_total
    #[test]
    fn the_reference_rendering_is_the_text() {
        assert_eq!(plain_text(listing()), vec!["main.rs", "  deep.rs", "lib.rs"]);
    }
    /// @tests REQ-VIEW.frontend_adds_nothing
    /// @tests REQ-VIEW.rendering_is_total
    #[test]
    fn a_frontend_that_draws_the_buffer_is_conformant_and_one_that_embroiders_is_not() {
        let buffer = listing();
        let drawn = Rendering {
            lines: vec!["main.rs".into(), "  deep.rs".into(), "lib.rs".into()],
            offered: vec![(0, "file.open".into()), (10, "file.open".into())],
        };
        assert_eq!(conformance(buffer.clone(), drawn.clone()), vec![]);

        let mut invented = drawn.clone();
        invented.offered.push((8, "file.delete".into()));
        assert!(matches!(
            conformance(buffer.clone(), invented)[0],
            Breach::ActionInvented { .. }
        ));

        let mut embroidered = drawn;
        embroidered.lines[0] = "📁 main.rs".into();
        assert!(matches!(
            conformance(buffer, embroidered)[0],
            Breach::LineDiffers { .. }
        ));
    }

    fn presented(painted: &str, name: &str) -> Presented {
        Presented { painted: painted.into(), name: name.into() }
    }

    /// @tests REQ-VIEW.presentation_may_be_symbolic
    /// @tests REQ-VIEW.screen_is_readable
    #[test]
    fn a_symbol_is_read_as_its_name_and_a_mislabelled_one_is_caught() {
        let buffer = listing();
        let offered = vec![(0, "file.open".to_string()), (10, "file.open".to_string())];

        let rows = vec![
            vec![presented("📁", "main.rs")],
            vec![presented("  deep.rs", "  deep.rs")],
            vec![presented("lib.rs", "lib.rs")],
        ];
        assert_eq!(accessible(rows[0].clone()), "main.rs");
        assert_eq!(symbolic(rows[0].clone()).len(), 1);
        assert_eq!(symbolic(rows[1].clone()), vec![]);
        assert_eq!(presented_conformance(buffer.clone(), rows.clone(), offered.clone()), vec![]);

        let mut mislabelled = rows;
        mislabelled[0] = vec![presented("📁", "📁")];
        assert!(matches!(
            presented_conformance(buffer, mislabelled, offered)[0],
            Breach::LineDiffers { .. }
        ));
    }
}
