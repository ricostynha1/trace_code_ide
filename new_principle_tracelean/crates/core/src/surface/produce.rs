//! Producing buffers from state.
//!
//! [`crate::surface::view`] says what a buffer *is*; this says where one comes
//! from. For every kind of thing the editor shows there is one function here
//! from the state to a buffer, and a frontend calls it rather than building a
//! buffer of its own — which is the whole of `REQ-SHOW.core_produces`, and the
//! reason two frontends cannot drift apart again.
//!
//! Nothing here reads a disk, a terminal or a clock. The state arrives as an
//! argument; reading it is the shell's job.
//!
//! @implements REQ-SHOW.core_produces

use serde::{Deserialize, Serialize};

use crate::observe::transcript::Event;
use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// A region something marked, before it is a span: a parser's output, or a
/// producer's own idea of where a line sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub start: usize,
    pub stop: usize,
    pub role: Role,
}

/// What can be done at a region when its role is all that is known.
///
/// One place, so a path in a listing and a path in a message offer the same
/// thing. A producer that knows more than the role — a menu row, which
/// dispatches one particular action — gives its spans actions directly instead.
///
/// @implements REQ-SHOW.core_produces
/// @drt REQ-SHOW.core_produces
pub fn actions_for(role: Role) -> Vec<String> {
    let names: &[&str] = match role {
        Role::Plain => &[],
        Role::Path => &["file.open"],
        Role::Entry => &["file.open"],
        Role::Heading => &["trace.evidence", "trace.check"],
        Role::Requirement => &["trace.requirement", "trace.context", "trace.evidence", "trace.findings"],
        Role::Level { .. } => &["trace.rollup"],
        Role::Added => &["observe.accept", "observe.reject"],
        Role::Removed => &["observe.accept", "observe.reject"],
        // A token is something to read; what can be done with code is offered
        // by the buffer, not by each keyword in it.
        Role::Token { .. } => &[],
        // The link beside a claim opens it; the word says what kind it is.
        Role::Claim { .. } => &[],
    };
    names.iter().map(|name| name.to_string()).collect()
}

/// Turn marks into spans, taking each one's affordances from its role.
pub fn spans_of_marks(marks: Vec<Mark>) -> Vec<Span> {
    marks
        .into_iter()
        .map(|mark| Span {
            start: mark.start,
            stop: mark.stop,
            role: mark.role,
            actions: actions_for(mark.role),
        })
        .collect()
}

/// Spans that describe the text they are over: in range, in order, not
/// overlapping.
///
/// Every producer ends with this, which is what makes
/// `well_formed_by_construction` a property of all of them rather than of each.
/// An empty or backwards span marks nothing and one starting inside its
/// predecessor would overlap; both are dropped rather than moved, because
/// moving a span hides which producer was wrong.
///
/// @implements REQ-SHOW.well_formed_by_construction
/// @implements REQ-SHOW.producers_are_total
/// @drt REQ-SHOW.well_formed_by_construction
/// @drt REQ-SHOW.producers_are_total
pub fn tidy(size: usize, spans: Vec<Span>) -> Vec<Span> {
    let mut clamped: Vec<Span> = spans
        .into_iter()
        .map(|span| Span { start: span.start.min(size), stop: span.stop.min(size), ..span })
        .collect();
    // Stable, so two spans starting at the same place keep the order the
    // producer put them in and the one it wrote first is the one that survives.
    clamped.sort_by_key(|span| span.start);

    let mut out: Vec<Span> = Vec::new();
    let mut previous = 0usize;
    for span in clamped {
        if span.stop <= span.start || span.start < previous {
            continue;
        }
        previous = span.stop;
        out.push(span);
    }
    out
}

/// A file as a buffer: its own text, and whatever a parser marked in it.
///
/// The marks come from outside because the model does not parse Rust. What is
/// modelled is everything that happens to them afterwards, which is where the
/// bugs are.
///
/// @implements REQ-SHOW.file_from_text
/// @implements REQ-SHOW.producer_is_pure
/// @drt REQ-SHOW.file_from_text
pub fn file_buffer(path: String, text: String, marks: Vec<Mark>) -> Buffer {
    let size = text.chars().count();
    Buffer {
        id: format!("file:{path}"),
        kind: BufferKind::File { path },
        text,
        spans: tidy(size, spans_of_marks(marks)),
    }
}

/// One line of a review, and what that line is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub role: Role,
    pub text: String,
}

/// How many lines two sides share at the front.
fn common_prefix(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn marked(marker: &str, role: Role, line: &str) -> DiffLine {
    DiffLine { role, text: format!("{marker}{line}") }
}

/// What changed, line by line: context, then what went, then what came.
///
/// No heuristics: the lines both sides share at the front, the lines both sides
/// share at the back, and everything between removed and then added. The same
/// answer every time, which a heuristic diff is not.
///
/// @implements REQ-SHOW.review_from_change
/// @drt REQ-SHOW.review_from_change
pub fn diff_lines(before: Vec<String>, after: Vec<String>) -> Vec<DiffLine> {
    let front = common_prefix(&before, &after);
    let before_rest: Vec<String> = before.iter().skip(front).cloned().collect();
    let after_rest: Vec<String> = after.iter().skip(front).cloned().collect();

    let before_back: Vec<String> = before_rest.iter().rev().cloned().collect();
    let after_back: Vec<String> = after_rest.iter().rev().cloned().collect();
    let back = common_prefix(&before_back, &after_back);

    let removed = before_rest.len() - back;
    let added = after_rest.len() - back;

    let mut out: Vec<DiffLine> = Vec::new();
    for line in before.iter().take(front) {
        out.push(marked(" ", Role::Plain, line));
    }
    for line in before_rest.iter().take(removed) {
        out.push(marked("-", Role::Removed, line));
    }
    for line in after_rest.iter().take(added) {
        out.push(marked("+", Role::Added, line));
    }
    for line in before_rest.iter().skip(removed) {
        out.push(marked(" ", Role::Plain, line));
    }
    out
}

/// A span over every line that is not context, at its offset in the joined text.
fn line_spans(lines: &[DiffLine]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for line in lines {
        let length = line.text.chars().count();
        if line.role != Role::Plain {
            out.push(Span {
                start: at,
                stop: at + length,
                role: line.role,
                actions: actions_for(line.role),
            });
        }
        at += length + 1;
    }
    out
}

/// A change between two states of a file, as a buffer a person can accept or
/// reject.
///
/// @implements REQ-SHOW.review_from_change
/// @implements REQ-SHOW.producer_is_pure
/// @drt REQ-SHOW.producer_is_pure
pub fn review_buffer(target: String, before: String, after: String) -> Buffer {
    let lines = diff_lines(split_lines(&before), split_lines(&after));
    let text: String =
        lines.iter().map(|line| line.text.clone()).collect::<Vec<_>>().join("\n");
    let size = text.chars().count();
    Buffer {
        id: format!("review:{target}"),
        kind: BufferKind::Review { target },
        text,
        spans: tidy(size, line_spans(&lines)),
    }
}

/// Split as the model's `String.splitOn "\n"` does: an empty text is one empty
/// line, not no lines.
fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

/// One row of a menu: the key, what it is for, and what it dispatches.
///
/// A row that *enters a mode* dispatches nothing, and its span carries no
/// action: `REQ-VIEW` is explicit that a span's affordances are names the
/// keymap dispatches, and mode entry is not one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuEntry {
    pub key: String,
    pub description: String,
    pub action: Option<String>,
}

fn menu_line(entry: &MenuEntry) -> String {
    format!("{}  {}", entry.key, entry.description)
}

/// A keymap mode as a buffer: one row a key, carrying what that key reaches.
///
/// @implements REQ-SHOW.menu_from_keymap
/// @implements REQ-SHOW.producer_is_pure
/// @drt REQ-SHOW.menu_from_keymap
pub fn menu_buffer(title: String, entries: Vec<MenuEntry>) -> Buffer {
    let mut spans = Vec::new();
    let mut at = 0usize;
    for entry in &entries {
        let length = menu_line(entry).chars().count();
        spans.push(Span {
            start: at,
            stop: at + length,
            role: Role::Entry,
            actions: entry.action.clone().into_iter().collect(),
        });
        at += length + 1;
    }
    let text: String = entries.iter().map(menu_line).collect::<Vec<_>>().join("\n");
    let size = text.chars().count();
    Buffer {
        id: format!("menu:{title}"),
        kind: BufferKind::Menu { title },
        text,
        spans: tidy(size, spans),
    }
}

fn event_line(event: &Event) -> String {
    format!("{}: {}", event.kind, event.text)
}

/// What was observed, as a buffer: one row an event, each leading to its diff.
///
/// @implements REQ-SHOW.record_from_events
/// @implements REQ-SHOW.producer_is_pure
/// @drt REQ-SHOW.record_from_events
pub fn record_buffer(title: String, events: Vec<Event>) -> Buffer {
    let mut spans = Vec::new();
    let mut at = 0usize;
    for event in &events {
        let length = event_line(event).chars().count();
        spans.push(Span {
            start: at,
            stop: at + length,
            role: Role::Entry,
            actions: vec!["observe.diff".to_string()],
        });
        at += length + 1;
    }
    let text: String = events.iter().map(event_line).collect::<Vec<_>>().join("\n");
    let size = text.chars().count();
    Buffer {
        id: format!("record:{title}"),
        kind: BufferKind::Record { title },
        text,
        spans: tidy(size, spans),
    }
}

/// What a sandboxed agent changed, said and spent, as the rows of one buffer.
///
/// Three sources, one order. The changes come first because they are the truth:
/// the transcript is the tool's account of what it did, and
/// `REQ-TRANSCRIPT.no_interpretation` is the rule that the account never
/// decides anything. The estimate comes last because it is a reading of that
/// account against a table, which is one step further from the workspace again.
///
/// @implements REQ-SHOW.sandbox_from_observation
/// @implements REQ-SHOW.producer_is_pure
/// @drt REQ-SHOW.sandbox_from_observation
pub fn sandbox_events(
    changed: Vec<String>,
    said: Vec<Event>,
    spend: crate::observe::cost::Spend,
) -> Vec<Event> {
    let mut out: Vec<Event> =
        changed.into_iter().map(|path| Event { kind: "changed".into(), text: path }).collect();
    out.extend(said);
    out.push(Event {
        kind: "cost".into(),
        text: crate::observe::cost::estimate_line(spend),
    });
    out
}

/// The sandbox station's buffer.
///
/// A record like any other record, so the rows a frontend draws and the actions
/// they carry are the ones `record_buffer` already gives — there is no second
/// kind of buffer here and no second producer.
///
/// @implements REQ-SHOW.sandbox_from_observation
pub fn sandbox_buffer(
    changed: Vec<String>,
    said: Vec<Event>,
    spend: crate::observe::cost::Spend,
) -> Buffer {
    record_buffer("sandbox".to_string(), sandbox_events(changed, said, spend))
}

/// A requirement, as a station shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub title: String,
    pub refines: Vec<String>,
    pub level: crate::evidence::Level,
    /// How many of its clauses something implements, of how many.
    pub implemented: usize,
    pub clauses: usize,
}

/// A node at a depth in the refinement graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub indent: usize,
    pub node: Node,
}

fn grade_text(grade: crate::evidence::Level) -> &'static str {
    use crate::evidence::Level;
    match grade {
        Level::L1 => "L1",
        Level::L2 => "L2",
        Level::L3 => "L3",
        Level::L4 => "L4",
    }
}

/// The two spans every row of either station carries: the grade, and the id.
///
/// Both take their actions from their role, so a level in this buffer offers
/// what a level offers anywhere. The grade is two characters wide, which is why
/// the id starts four along.
fn row_spans(at: usize, indent: usize, node: &Node) -> Vec<Span> {
    let lead = at + indent * 2;
    vec![
        Span {
            start: lead,
            stop: lead + 2,
            role: Role::Level { grade: node.level },
            actions: actions_for(Role::Level { grade: node.level }),
        },
        Span {
            start: lead + 4,
            stop: lead + 4 + node.id.chars().count(),
            role: Role::Requirement,
            actions: actions_for(Role::Requirement),
        },
    ]
}

/// Cells of five a node's implemented clauses fill, rounded, never past five.
fn filled_of(node: &Node) -> usize {
    ((node.implemented * 5 + node.clauses / 2) / node.clauses.max(1)).min(5)
}

/// The coverage bar, read at a glance down the list: filled cells, then empty.
fn bar_of(node: &Node) -> String {
    "█".repeat(filled_of(node)) + &"░".repeat(5 - filled_of(node))
}

fn index_line(node: &Node) -> String {
    format!(
        "{}  {}  {} {}/{}  {}",
        grade_text(node.level),
        node.id,
        bar_of(node),
        node.implemented,
        node.clauses,
        node.title
    )
}

/// The bar's filled cells, in the colour of what fills them.
fn bar_spans(at: usize, node: &Node) -> Vec<Span> {
    let start = at + 4 + node.id.chars().count() + 2;
    let role = Role::Claim { role: crate::trace::annotation::Role::Implements };
    match filled_of(node) {
        0 => Vec::new(),
        filled => vec![Span { start, stop: start + filled, role, actions: actions_for(role) }],
    }
}

/// The requirement set as a buffer: one row a requirement, carrying what its
/// evidence reached and what it is called.
///
/// @implements REQ-SHOW.index_from_requirements
/// @implements REQ-SHOW.producer_is_pure
/// @implements REQ-SHOW.well_formed_by_construction
/// @drt REQ-SHOW.index_from_requirements
pub fn requirements_buffer(nodes: Vec<Node>) -> Buffer {
    let mut spans = Vec::new();
    let mut at = 0usize;
    for node in &nodes {
        spans.extend(row_spans(at, 0, node));
        spans.extend(bar_spans(at, node));
        at += index_line(node).chars().count() + 1;
    }
    let text: String = nodes.iter().map(index_line).collect::<Vec<_>>().join("\n");
    let size = text.chars().count();
    Buffer {
        id: "menu:requirements".to_string(),
        kind: BufferKind::Menu { title: "requirements".to_string() },
        text,
        spans: tidy(size, spans),
    }
}

fn children_of<'a>(nodes: &'a [Node], parent: &str) -> Vec<&'a Node> {
    nodes.iter().filter(|node| node.refines.iter().any(|name| name == parent)).collect()
}

/// Expand one node and, under it, everything that refines it.
///
/// `fuel` bounds the depth, and it has to: `refines` is data, so a cycle in it
/// is representable and a graph walk without a bound would not be a function. A
/// node reached at the bound is drawn without its children rather than dropped
/// — a row missing is a lie about what exists, a row without its children only
/// a view cut short.
fn expand(fuel: usize, nodes: &[Node], indent: usize, node: &Node) -> Vec<Row> {
    let here = Row { indent, node: node.clone() };
    if fuel == 0 {
        return vec![here];
    }
    let mut out = vec![here];
    out.extend(expand_all(fuel - 1, nodes, indent + 1, &children_of(nodes, &node.id)));
    out
}

fn expand_all(fuel: usize, nodes: &[Node], indent: usize, todo: &[&Node]) -> Vec<Row> {
    todo.iter().flat_map(|node| expand(fuel, nodes, indent, node)).collect()
}

/// The refinement graph, depth first, roots first.
///
/// A root is a requirement that refines nothing — the architecture documents.
/// Four levels are drawn, one more than the deepest chain this project has.
pub fn design_rows(nodes: Vec<Node>) -> Vec<Row> {
    let roots: Vec<&Node> = nodes.iter().filter(|node| node.refines.is_empty()).collect();
    expand_all(3, &nodes, 0, &roots)
}

/// A row of the graph is a row of the index, indented under what it refines.
fn design_line(row: &Row) -> String {
    " ".repeat(row.indent * 2) + &index_line(&row.node)
}

/// The refinement graph as a buffer: one row a requirement, indented under what
/// it refines.
///
/// @implements REQ-SHOW.graph_from_refinement
/// @implements REQ-SHOW.producer_is_pure
/// @implements REQ-SHOW.well_formed_by_construction
/// @drt REQ-SHOW.graph_from_refinement
pub fn design_buffer(nodes: Vec<Node>) -> Buffer {
    let rows = design_rows(nodes);
    let mut spans = Vec::new();
    let mut at = 0usize;
    for row in &rows {
        spans.extend(row_spans(at, row.indent, &row.node));
        spans.extend(bar_spans(at + row.indent * 2, &row.node));
        at += design_line(row).chars().count() + 1;
    }
    let text: String = rows.iter().map(design_line).collect::<Vec<_>>().join("\n");
    let size = text.chars().count();
    Buffer {
        id: "menu:design".to_string(),
        kind: BufferKind::Menu { title: "design".to_string() },
        text,
        spans: tidy(size, spans),
    }
}

/// The character offset the line at `from` starts at.
fn line_start(lines: &[String], start: usize) -> usize {
    lines.iter().take(start).map(|line| line.chars().count() + 1).sum()
}

/// One span, moved into the window's coordinates and cut to fit.
///
/// A span ending before the window or starting after it marks nothing here and
/// is dropped. One straddling an edge is cut — saturating subtraction is exactly
/// the clip wanted at the near edge.
fn clip_span(shift: usize, size: usize, span: &Span) -> Option<Span> {
    if span.stop <= shift || shift + size <= span.start {
        return None;
    }
    Some(Span {
        start: span.start.saturating_sub(shift),
        stop: span.stop.saturating_sub(shift).min(size),
        role: span.role,
        actions: span.actions.clone(),
    })
}

/// The part of a buffer starting at line `from`, at most `count` lines of it.
///
/// This is how a frontend scrolls without lying. A terminal showing forty lines
/// of a long file draws a fraction of it, and a frontend draws everything it is
/// given — so what it is given is the fraction. The identity is unchanged, so a
/// delta carries a scroll the same way it carries an edit.
///
/// @implements REQ-SHOW.window_is_a_buffer
/// @implements REQ-SHOW.producers_are_total
/// @drt REQ-SHOW.window_is_a_buffer
pub fn window(buffer: Buffer, start: usize, count: usize) -> Buffer {
    let lines = crate::surface::view::plain_text(buffer.clone());
    let taken: Vec<String> = lines.iter().skip(start).take(count).cloned().collect();
    let text = taken.join("\n");
    let size = text.chars().count();
    let shift = line_start(&lines, start);
    let clipped: Vec<Span> =
        buffer.spans.iter().filter_map(|span| clip_span(shift, size, span)).collect();
    Buffer { id: buffer.id, kind: buffer.kind, text, spans: tidy(size, clipped) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{directory_buffer, faults, plain_text};

    /// Three of four clauses implemented fill four cells of five, and those
    /// cells are drawn in the colour of what fills them.
    #[test]
    fn the_index_draws_each_requirement_s_coverage_as_a_bar() {
        let node = Node {
            id: "REQ-A".into(),
            title: "A thing".into(),
            refines: vec![],
            level: crate::evidence::Level::L1,
            implemented: 3,
            clauses: 4,
        };
        let shown = requirements_buffer(vec![node]);
        assert_eq!(plain_text(shown.clone()), vec!["L1  REQ-A  ████░ 3/4  A thing"]);
        let bar = shown.spans.iter().find(|s| matches!(s.role, Role::Claim { .. })).expect("a bar span");
        assert_eq!((bar.start, bar.stop), (11, 15));
        assert!(faults(shown).is_empty());
    }

    fn mark(start: usize, stop: usize, role: Role) -> Mark {
        Mark { start, stop, role }
    }

    /// Marks of all three wrong kinds at once — one overlapping its neighbour,
    /// one running backwards, one reaching past the end — still produce a
    /// buffer that describes its own text.
    ///
    /// @tests REQ-SHOW.well_formed_by_construction
    #[test]
    fn a_producer_cannot_emit_a_span_the_text_does_not_carry() {
        let buffer = file_buffer(
            "a.rs".into(),
            "let x = 1".into(),
            vec![
                mark(3, 7, Role::Heading),
                mark(5, 9, Role::Plain),
                mark(9, 4, Role::Path),
                mark(0, 400, Role::Requirement),
            ],
        );
        assert_eq!(faults(buffer.clone()), vec![]);
        // The mark that reached past the end is clamped to the whole text, and
        // sorting by start puts it first — so it is the one that survives and
        // the three it covers are dropped. Dropping rather than moving is what
        // makes that legible: the buffer says one region was marked, not four
        // shuffled into place.
        assert_eq!(buffer.spans.len(), 1);
        assert_eq!((buffer.spans[0].start, buffer.spans[0].stop), (0, 9));
        assert_eq!(buffer.spans[0].role, Role::Requirement);
    }

    /// @tests REQ-SHOW.review_from_change
    #[test]
    fn a_review_shows_what_went_and_what_came_between_what_stayed() {
        let lines = diff_lines(
            vec!["one".into(), "two".into(), "three".into()],
            vec!["one".into(), "TWO".into(), "three".into()],
        );
        assert_eq!(
            lines,
            vec![
                DiffLine { role: Role::Plain, text: " one".into() },
                DiffLine { role: Role::Removed, text: "-two".into() },
                DiffLine { role: Role::Added, text: "+TWO".into() },
                DiffLine { role: Role::Plain, text: " three".into() },
            ]
        );

        let buffer = review_buffer("a.rs".into(), "one\ntwo\nthree".into(), "one\nTWO\nthree".into());
        assert_eq!(faults(buffer.clone()), vec![]);
        assert_eq!(plain_text(buffer.clone()), vec![" one", "-two", "+TWO", " three"]);
        // Only the changed lines can be accepted or rejected; context cannot.
        assert_eq!(buffer.spans.len(), 2);
        for span in &buffer.spans {
            assert_eq!(span.actions, vec!["observe.accept", "observe.reject"]);
        }
    }

    /// A file that did not change produces a review with nothing to act on,
    /// rather than a review claiming everything changed.
    ///
    /// @tests REQ-SHOW.review_from_change
    #[test]
    fn an_unchanged_file_reviews_as_no_change() {
        let buffer = review_buffer("a.rs".into(), "one\ntwo".into(), "one\ntwo".into());
        assert!(buffer.spans.is_empty());
        assert_eq!(plain_text(buffer), vec![" one", " two"]);
    }

    /// @tests REQ-SHOW.menu_from_keymap
    #[test]
    fn a_menu_row_offers_what_its_key_dispatches_and_a_mode_row_offers_nothing() {
        let buffer = menu_buffer(
            "leader".into(),
            vec![
                MenuEntry { key: "t".into(), description: "trace".into(), action: None },
                MenuEntry {
                    key: "u".into(),
                    description: "undo".into(),
                    action: Some("history.undo".into()),
                },
            ],
        );
        assert_eq!(faults(buffer.clone()), vec![]);
        let offered: Vec<Vec<String>> =
            buffer.spans.iter().map(|span| span.actions.clone()).collect();
        assert_eq!(offered, vec![Vec::<String>::new(), vec!["history.undo".to_string()]]);
    }

    /// @tests REQ-SHOW.record_from_events
    #[test]
    fn a_record_leads_from_each_event_to_its_diff() {
        let buffer = record_buffer(
            "agent".into(),
            vec![
                Event { kind: "wrote".into(), text: "a.rs".into() },
                Event { kind: "ran".into(), text: "cargo test".into() },
            ],
        );
        assert_eq!(faults(buffer.clone()), vec![]);
        assert_eq!(plain_text(buffer.clone()), vec!["wrote: a.rs", "ran: cargo test"]);
        assert!(buffer.spans.iter().all(|span| span.actions == vec!["observe.diff"]));
    }

    /// Every kind has a producer, and no producer fails on the empty state.
    ///
    /// @tests REQ-SHOW.producers_are_total
    #[test]
    fn every_kind_has_a_producer_and_none_of_them_refuses() {
        let produced = vec![
            file_buffer("a.rs".into(), String::new(), vec![]),
            directory_buffer("src".into(), vec![]),
            review_buffer("a.rs".into(), String::new(), String::new()),
            menu_buffer("empty".into(), vec![]),
            record_buffer("quiet".into(), vec![]),
        ];
        for buffer in produced {
            assert_eq!(faults(buffer.clone()), vec![], "{:?} was not well formed", buffer.kind);
        }
    }

    /// A window is a buffer: it describes its own text, carries the spans that
    /// fall in it, and is empty rather than wrong when it falls off the end.
    ///
    /// @tests REQ-SHOW.window_is_a_buffer
    #[test]
    fn a_window_is_a_buffer_of_the_part_it_shows() {
        let whole = file_buffer(
            "a.rs".into(),
            "one\ntwo\nthree".into(),
            vec![mark(4, 7, Role::Heading)],
        );

        let middle = window(whole.clone(), 1, 1);
        assert_eq!(plain_text(middle.clone()), vec!["two"]);
        assert_eq!(faults(middle.clone()), vec![]);
        // The span that was on the second line is on the window's first.
        assert_eq!(middle.spans.len(), 1);
        assert_eq!((middle.spans[0].start, middle.spans[0].stop), (0, 3));
        // And the identity is unchanged, so a scroll is a delta like any other.
        assert_eq!(middle.id, whole.id);

        // A window of everything is what it started with.
        assert_eq!(window(whole.clone(), 0, 9).text, whole.text);
        // A window past the end is empty, not an error.
        assert_eq!(window(whole.clone(), 9, 4).text, "");
        assert_eq!(window(whole.clone(), 0, 0).text, "");

        // A span straddling the near edge is cut rather than dropped.
        let straddling = file_buffer(
            "a.rs".into(),
            "one\ntwo\nthree".into(),
            vec![mark(0, 12, Role::Heading)],
        );
        let cut = window(straddling, 1, 1);
        assert_eq!((cut.spans[0].start, cut.spans[0].stop), (0, 3));
        assert_eq!(faults(cut), vec![]);
    }

    /// Non-ASCII text is measured in characters, or a span lands mid-character
    /// and two frontends disagree about where it is.
    ///
    /// @tests REQ-SHOW.well_formed_by_construction
    #[test]
    fn a_span_is_measured_in_characters_not_bytes() {
        let buffer = record_buffer(
            "wide".into(),
            vec![Event { kind: "é".into(), text: "ü".into() }],
        );
        assert_eq!(faults(buffer.clone()), vec![]);
        // "é: ü" is four characters and six bytes.
        assert_eq!(buffer.spans[0].stop, 4);
        assert_eq!(buffer.text.len(), 6);
    }
}
