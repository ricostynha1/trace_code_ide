//! The representation every frontend renders.
//!
//! Implements `REQ-VIEW`. A buffer is an identity, a kind and text; structure is
//! spans over that text saying what a region *is* and what can be done there. A
//! file is a buffer, and so is a directory listing, a diff under review and a
//! menu.
//!
//! The core answers "what is on screen" once. Two frontends computing their own
//! view give two answers, and the unmaintained answer rots with nothing failing
//! — which is what happened to the TUI this port came from.

use serde::{Deserialize, Serialize};

/// What a buffer holds. Every shown thing is one of these.
///
/// @implements REQ-VIEW.everything_is_a_buffer
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BufferKind {
    /// The contents of a file on disk.
    File { path: String },
    /// A directory, listed.
    Directory { path: String },
    /// A change an external agent made, waiting for a person.
    Review { target: String },
    /// A list of choices: the which-key bar, a command list.
    Menu { title: String },
    /// Anything recorded rather than chosen: a transcript, a log.
    Record { title: String },
}

/// What a region of text *is*. Never how it looks.
///
/// A role a frontend does not recognise is still rendered as text; one it does
/// recognise it may render as richly as its medium allows.
///
/// @implements REQ-VIEW.structure_over_text
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Plain,
    /// A path, in a listing or in a message.
    Path,
    /// One item of a listing or menu.
    Entry,
    /// A heading: a section, a file name at the top of a diff.
    Heading,
    /// A requirement or clause identifier.
    Requirement,
    /// An evidence level, and which one.
    ///
    /// The grade is carried rather than left in the text. A frontend that had
    /// to read `L3` out of the characters to colour it would be parsing the
    /// buffer, which is the one thing a frontend may not do; and one that could
    /// not tell L1 from L4 would have to paint every level the same, which is
    /// the whole of what a colour is for here.
    Level { grade: crate::evidence::Level },
    /// A line a change added.
    Added,
    /// A line a change removed.
    Removed,
    /// A token of source code, and what kind: what syntax highlighting colours.
    ///
    /// The kind comes from parsing, in the core, for the same reason a level
    /// carries its grade: a frontend deciding what was a keyword would be
    /// parsing the buffer itself.
    Token { kind: TokenKind },
    /// What a claim on a requirement is — implements, tests, models, proves —
    /// so each is coloured as its gutter chip is, and a requirement's claims
    /// are told apart at a glance.
    Claim { role: crate::trace::annotation::Role },
}

/// What a token of source code is.
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
    /// Markdown prose: a heading line, `**bold**`, `*italic*`, `[a link](…)`.
    Heading,
    Bold,
    Italic,
    Link,
}

/// A region of a buffer's text, what it is, and what can be done there.
///
/// `actions` names what is available, using the names the keymap dispatches. A
/// frontend reads them; it does not invent them.
///
/// Offsets are in characters, matching the model: a byte offset would mean two
/// frontends disagreeing about where a span is the moment anything is not ASCII.
///
/// @implements REQ-VIEW.affordances_named
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub start: usize,
    pub stop: usize,
    pub role: Role,
    pub actions: Vec<String>,
}

/// What is on screen, or could be.
///
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

/// What is wrong with a buffer's structure.
///
/// Reported rather than clamped: a frontend cannot render what is not there,
/// and moving the span silently would hide which producer was wrong.
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

/// Everything wrong with a buffer's spans, in the order the spans are given.
///
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

/// What can be done at a position: the actions of every span covering it, in
/// span order, without repeats.
///
/// An action a frontend offers and this does not return is an affordance nobody
/// declared, which is the thing that cannot be checked.
///
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

/// Every action any span of a buffer names, sorted and without repeats.
///
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

/// The rendering every frontend has to agree with: the buffer's text, as lines.
///
/// A frontend with a richer medium draws more than this. What it may not do is
/// show something else.
///
/// @implements REQ-VIEW.rendering_is_total
/// @implements REQ-VIEW.text_is_the_content
/// @drt REQ-VIEW.rendering_is_total
pub fn plain_text(buffer: Buffer) -> Vec<String> {
    buffer.text.split('\n').map(str::to_string).collect()
}

fn listing_line(entry: &(usize, String)) -> String {
    format!("{}{}", " ".repeat(2 * entry.0), entry.1)
}

/// A directory listing as a buffer: indentation, then the name, one entry a line.
///
/// It has no grammar and needs none — the core produces its spans directly, and
/// they are the same spans a parser would have produced for a file. One type,
/// two producers.
///
/// @implements REQ-VIEW.structure_has_one_type
/// @implements REQ-VIEW.everything_is_a_buffer
/// @implements REQ-SHOW.listing_from_entries
/// @drt REQ-VIEW.structure_has_one_type
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

/// A change to what is shown.
///
/// A frontend that re-reads everything on every keystroke stutters, so a change
/// is a delta — and the delta is enough: applying it gives back exactly the
/// buffer it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    /// The new text, when it changed.
    pub text: Option<String>,
    /// The new spans, when they changed.
    pub spans: Option<Vec<Span>>,
    /// The new kind, when it changed.
    pub kind: Option<BufferKind>,
}

/// What changed between two states of one buffer.
///
/// @implements REQ-VIEW.changes_are_deltas
/// @drt REQ-VIEW.changes_are_deltas
pub fn delta(before: Buffer, after: Buffer) -> Delta {
    Delta {
        text: if before.text == after.text { None } else { Some(after.text) },
        spans: if before.spans == after.spans { None } else { Some(after.spans) },
        kind: if before.kind == after.kind { None } else { Some(after.kind) },
    }
}

/// Apply a change.
///
/// @implements REQ-VIEW.changes_are_deltas
pub fn apply_delta(buffer: Buffer, d: Delta) -> Buffer {
    Buffer {
        id: buffer.id,
        kind: d.kind.unwrap_or(buffer.kind),
        text: d.text.unwrap_or(buffer.text),
        spans: d.spans.unwrap_or(buffer.spans),
    }
}

/// What a frontend says it drew.
///
/// A frontend is a process that, given a buffer, answers with this: the lines it
/// put on screen and, for each position, the actions it offered there. That
/// answer is a value, so it crosses the same line protocol every other side of
/// this system crosses, and the same check runs against a terminal, a web view,
/// or anything else somebody writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rendering {
    pub lines: Vec<String>,
    /// Every action offered, with the position it was offered at.
    pub offered: Vec<(usize, String)>,
}

/// A way a rendering fails to be the buffer it came from.
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

/// Everything a frontend got wrong about a buffer.
///
/// Not how it looks — that is the frontend's business. That the text is the
/// buffer's text, and that the actions offered are exactly the actions
/// declared: a frontend that invents one is offering something nobody can
/// trace, and one that drops the last action on a line has quietly removed a
/// feature.
///
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

/// A region as a frontend put it on the screen: the glyphs it painted, and the
/// name those glyphs carry.
///
/// The name is what a screen reader announces and what the capture harness
/// reads. A frontend with glyphs to spare paints one; what it may not do is
/// give the region a name the buffer does not contain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presented {
    pub painted: String,
    pub name: String,
}

/// What a row of presented regions reads as: the names, in order.
///
/// The glyphs are not consulted. That is the whole content of the clause.
///
/// @implements REQ-VIEW.presentation_may_be_symbolic
/// @drt REQ-VIEW.presentation_may_be_symbolic
pub fn accessible(row: Vec<Presented>) -> String {
    row.into_iter().map(|piece| piece.name).collect()
}

/// The regions a frontend chose to present as something other than their name.
///
/// Reported so that a run can say it exercised the symbolic case rather than
/// assuming it did.
///
/// @implements REQ-VIEW.presentation_may_be_symbolic
pub fn symbolic(row: Vec<Presented>) -> Vec<Presented> {
    row.into_iter().filter(|piece| piece.painted != piece.name).collect()
}

/// A frontend that presented symbolically, checked against the buffer it was
/// given.
///
/// Its rows are read by their names and then compared exactly as any other
/// rendering is: there is no second notion of conformance, only a second way of
/// arriving at the lines.
///
/// Not itself differentially bound: it is `conformance` with its lines read out
/// of names, and `conformance` is the bound one. What is claimed here beyond
/// that — that the painting is not consulted — is a theorem
/// (`a_symbol_is_read_as_its_name`), which is a stronger answer than a run.
///
/// @implements REQ-VIEW.presentation_may_be_symbolic
/// @implements REQ-VIEW.screen_is_readable
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
        // Each span covers the name, not the indentation before it.
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
        // The indentation of the nested entry belongs to no span, so nothing is
        // offered there — an empty answer rather than a guess.
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

        // An action nobody declared, at a position that has none.
        let mut invented = drawn.clone();
        invented.offered.push((8, "file.delete".into()));
        assert!(matches!(
            conformance(buffer.clone(), invented)[0],
            Breach::ActionInvented { .. }
        ));

        // A frontend that draws its own text instead of the buffer's.
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

        // The first row painted as an emblem. What is read is still the name.
        let rows = vec![
            vec![presented("📁", "main.rs")],
            vec![presented("  deep.rs", "  deep.rs")],
            vec![presented("lib.rs", "lib.rs")],
        ];
        assert_eq!(accessible(rows[0].clone()), "main.rs");
        assert_eq!(symbolic(rows[0].clone()).len(), 1);
        assert_eq!(symbolic(rows[1].clone()), vec![]);
        assert_eq!(presented_conformance(buffer.clone(), rows.clone(), offered.clone()), vec![]);

        // A frontend that names a region after the glyph it painted has put
        // something on the screen the buffer does not contain, and it is the
        // name that gives it away — the painting never could.
        let mut mislabelled = rows;
        mislabelled[0] = vec![presented("📁", "📁")];
        assert!(matches!(
            presented_conformance(buffer, mislabelled, offered)[0],
            Breach::LineDiffers { .. }
        ));
    }
}
