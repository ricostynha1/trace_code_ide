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
/// @implements REQ-SHOW.actions_by_role
/// @drt REQ-SHOW.actions_by_role
pub fn actions_for(role: Role) -> Vec<String> {
    let names: &[&str] = match role {
        Role::Plain => &[],
        Role::Path => &["file.open"],
        Role::Entry => &["file.open"],
        Role::Heading => &["trace.evidence", "trace.check"],
        Role::Requirement => &["trace.requirement", "trace.context", "trace.evidence", "trace.findings"],
        Role::Mark { .. } => &["trace.rollup"],
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

/// A menu laid out on one row of `width` characters, two spaces between
/// entries: each entry's key and description when they all fit, and only the
/// keys when they do not, so a description never takes the room of a later
/// entry. Each entry still carries what it reaches.
///
/// For a frontend with one row to spare for a list, as a terminal has for the
/// stations: joining the menu's rows itself, it cut the last three stations
/// off at 100 columns.
///
/// @implements REQ-LOOK.row_fits
/// @drt REQ-LOOK.row_fits
pub fn menu_row(title: String, entries: Vec<MenuEntry>, width: u64) -> Buffer {
    let full: Vec<String> = entries.iter().map(menu_line).collect();
    let fits = full.join("  ").chars().count() as u64 <= width;
    let pieces: Vec<String> = if fits { full } else { entries.iter().map(|e| e.key.clone()).collect() };
    let mut spans = Vec::new();
    let mut at = 0usize;
    for (entry, piece) in entries.iter().zip(&pieces) {
        let length = piece.chars().count();
        spans.push(Span { start: at, stop: at + length, role: Role::Entry, actions: entry.action.clone().into_iter().collect() });
        at += length + 2;
    }
    let text = pieces.join("  ");
    let size = text.chars().count();
    Buffer { id: format!("row:{title}"), kind: BufferKind::Menu { title }, text, spans: tidy(size, spans) }
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

/// How many of a requirement's clauses hold each thing a row counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub implemented: usize,
    pub annotated: usize,
    pub judged: usize,
    pub tested: usize,
    pub proved: usize,
}

/// A requirement, as a station shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub title: String,
    pub refines: Vec<String>,
    /// The marks every one of its clauses holds.
    pub marks: crate::evidence::Marks,
    pub clauses: usize,
    /// Clauses a model can answer: all but the structural ones, which need no
    /// implementation and cannot be tested or proved.
    pub modelled: usize,
    /// How many clauses hold each: implementing, testing and proving out of
    /// `modelled`, annotating and judging out of `clauses`.
    pub held: Tally,
    /// Lines of its implementing code tests run, of how many — when that code
    /// was measured as it is now.
    #[serde(default)]
    pub covered: Option<(u64, u64)>,
}

/// A piece of a row: its text, and the role and actions of the span it is,
/// if it is one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Piece {
    text: String,
    role: Option<Role>,
    actions: Vec<String>,
}

fn plain(text: impl Into<String>) -> Piece {
    Piece { text: text.into(), role: None, actions: Vec::new() }
}

fn spanned(text: impl Into<String>, role: Role) -> Piece {
    Piece { text: text.into(), role: Some(role), actions: actions_for(role) }
}

fn pieces_text(pieces: &[Piece]) -> String {
    pieces.iter().map(|p| p.text.as_str()).collect()
}

/// The spans of a row's pieces, the row starting at `at`.
fn pieces_spans(at: usize, pieces: &[Piece]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = at;
    for piece in pieces {
        let stop = start + piece.text.chars().count();
        if let Some(role) = piece.role {
            out.push(Span { start, stop, role, actions: piece.actions.clone() });
        }
        start = stop;
    }
    out
}

/// Lines run, of how many, over a requirement and everything refining it.
///
/// Each child's figure is added in whole, so a requirement two paths reach
/// counts twice: this says how much under a row was run, not an exact count
/// of lines. `fuel` bounds it, because `refines` may hold a cycle.
fn lines_under(fuel: usize, nodes: &[Node], node: &Node) -> (u64, u64) {
    let own = node.covered.unwrap_or((0, 0));
    match fuel {
        0 => own,
        _ => children_of(nodes, &node.id).into_iter().fold(own, |(run, all), child| {
            let (r, a) = lines_under(fuel - 1, nodes, child);
            (run + r, all + a)
        }),
    }
}

/// What the requirements refining a node left unrun, if anything: the figure
/// over the node and all of them, `+children 85%`.
fn children_short(nodes: &[Node], node: &Node) -> Option<String> {
    let (run, all) = children_of(nodes, &node.id).into_iter().fold((0, 0), |(run, all), child| {
        let (r, a) = lines_under(nodes.len(), nodes, child);
        (run + r, all + a)
    });
    let (own_run, own_all) = node.covered.unwrap_or((0, 0));
    (run < all).then(|| format!("+children {}%", (own_run + run) * 100 / (own_all + all)))
}

/// One count: `J2/4` held of applicable, `T·` where none can be.
fn count_piece(letter: char, held: usize, of: usize, role: Role) -> Piece {
    match of {
        0 => plain(format!("{letter}·")),
        _ => spanned(format!("{letter}{held}/{of}"), role),
    }
}

/// A requirement as a station shows it, from each clause's marks and whether
/// something implements it: the marks every clause holds, and how many hold
/// each. A structural clause is counted only where a mark can apply to it.
///
/// @implements REQ-SHOW.index_from_requirements
/// @implements REQ-EVID.structural_not_applicable
pub fn node_of(
    id: String,
    title: String,
    refines: Vec<String>,
    clauses: Vec<(crate::evidence::Marks, bool)>,
    covered: Option<(u64, u64)>,
) -> Node {
    let modelled: Vec<&(crate::evidence::Marks, bool)> = clauses.iter().filter(|(m, _)| !m.structural).collect();
    let count = |f: &dyn Fn(&crate::evidence::Marks) -> bool| clauses.iter().filter(|(m, _)| f(m)).count();
    Node {
        id,
        title,
        refines,
        marks: crate::trace::rollup::combine(clauses.iter().map(|(m, _)| *m).collect()),
        clauses: clauses.len(),
        modelled: modelled.len(),
        held: Tally {
            implemented: modelled.iter().filter(|(_, implemented)| *implemented).count(),
            annotated: count(&|m| m.annotated),
            judged: count(&|m| m.judged),
            tested: count(&|m| m.tested),
            proved: count(&|m| m.proved),
        },
        covered,
    }
}

/// Whether anything about a requirement is still to do: a clause short of a
/// mark it can hold, or a line its tests never ran.
pub fn is_open(node: &Node) -> bool {
    let t = node.held;
    t.implemented < node.modelled
        || t.tested < node.modelled
        || t.proved < node.modelled
        || t.annotated < node.clauses
        || t.judged < node.clauses
        || node.covered.is_some_and(|(run, all)| run < all)
}

/// A row of either station, after its lead: the marks, the name, how many
/// clauses hold each mark in that mark's colour, the lines measured and what
/// the requirements under it left unrun, and the title.
fn row_pieces(nodes: &[Node], node: &Node) -> Vec<Piece> {
    use crate::evidence::Mark as M;
    let mut out: Vec<Piece> = crate::surface::requirement_view::mark_pieces(node.marks)
        .into_iter()
        .map(|(text, role)| match role {
            Role::Plain => plain(text),
            role => spanned(text, role),
        })
        .collect();
    out.push(plain("  "));
    out.push(spanned(node.id.clone(), Role::Requirement));
    out.push(plain("  "));
    let t = node.held;
    let held = |mark| Role::Mark { mark, held: true };
    let counts = [
        count_piece('I', t.implemented, node.modelled, Role::Claim { role: crate::trace::annotation::Role::Implements }),
        count_piece('A', t.annotated, node.clauses, held(M::Annotated)),
        count_piece('J', t.judged, node.clauses, held(M::Judged)),
        count_piece('T', t.tested, node.modelled, held(M::Tested)),
        count_piece('P', t.proved, node.modelled, held(M::Proved)),
    ];
    for (i, count) in counts.into_iter().enumerate() {
        if i > 0 {
            out.push(plain(" "));
        }
        out.push(count);
    }
    out.push(plain("  "));
    if let Some((run, all)) = node.covered {
        match all {
            0 => out.push(plain("lines none")),
            // Coloured as a diff's lines are, but it is no diff: it opens what
            // was measured rather than the actions of a change under review.
            _ => out.push(Piece {
                text: format!("lines {}%", run * 100 / all),
                role: Some(if run == all { Role::Added } else { Role::Removed }),
                actions: vec![format!("trace.coverage {}", node.id)],
            }),
        }
        out.push(plain("  "));
    }
    if let Some(said) = children_short(nodes, node) {
        out.push(Piece { text: said, role: Some(Role::Removed), actions: Vec::new() });
        out.push(plain("  "));
    }
    out.push(plain(node.title.clone()));
    out
}

/// A node at a depth in the refinement graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub indent: usize,
    pub node: Node,
}

/// A node with something still to do in it or anywhere under it.
fn open_under(fuel: usize, nodes: &[Node], node: &Node) -> bool {
    is_open(node)
        || (fuel > 0 && children_of(nodes, &node.id).into_iter().any(|child| open_under(fuel - 1, nodes, child)))
}

/// The requirements a station shows: all of them, or with `open_only` those
/// with something to do in them or under them, so the graph keeps the path to
/// each.
fn shown(nodes: &[Node], open_only: bool) -> Vec<Node> {
    nodes.iter().filter(|node| !open_only || open_under(nodes.len(), nodes, node)).cloned().collect()
}

/// The requirement set as a buffer: one row a requirement, carrying what its
/// evidence reached and what it is called; with `open_only`, only those with
/// something still to do.
///
/// @implements REQ-SHOW.index_from_requirements
/// @implements REQ-SHOW.producer_is_pure
/// @implements REQ-SHOW.well_formed_by_construction
/// @implements REQ-SHOW.open_filtered
/// @implements REQ-SHOW.children_lines
/// @drt REQ-SHOW.index_from_requirements
/// @drt REQ-SHOW.children_lines
pub fn requirements_buffer(nodes: Vec<Node>, open_only: bool) -> Buffer {
    // A flat list has no path to keep, so only what is open itself.
    let rows: Vec<Vec<Piece>> = nodes
        .iter()
        .filter(|node| !open_only || is_open(node))
        .map(|node| row_pieces(&nodes, node))
        .collect();
    let mut spans = Vec::new();
    let mut at = 0usize;
    for row in &rows {
        spans.extend(pieces_spans(at, row));
        at += pieces_text(row).chars().count() + 1;
    }
    let text: String = rows.iter().map(|row| pieces_text(row)).collect::<Vec<_>>().join("\n");
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

/// One node and, when it is open, everything that refines it.
///
/// `fuel` bounds the depth, and it has to: `refines` is data, so a cycle in it
/// is representable and a graph walk without a bound would not be a function. A
/// node reached at the bound is drawn without its children rather than dropped
/// — a row missing is a lie about what exists, a row without its children only
/// a view cut short.
fn expand(fuel: usize, nodes: &[Node], open: &[String], indent: usize, node: &Node) -> Vec<Row> {
    let mut out = vec![Row { indent, node: node.clone() }];
    if fuel > 0 && open.contains(&node.id) {
        out.extend(expand_all(fuel - 1, nodes, open, indent + 1, &children_of(nodes, &node.id)));
    }
    out
}

fn expand_all(fuel: usize, nodes: &[Node], open: &[String], indent: usize, todo: &[&Node]) -> Vec<Row> {
    todo.iter().flat_map(|node| expand(fuel, nodes, open, indent, node)).collect()
}

/// The refinement graph, depth first, roots first, unfolded where `open` says.
///
/// A root is a requirement that refines nothing — the architecture documents.
/// Folded, only they show, so hundreds of requirements open as a handful. No
/// chain is longer than the requirements there are, so that is the fuel.
pub fn design_rows(nodes: Vec<Node>, open: Vec<String>) -> Vec<Row> {
    let roots: Vec<&Node> = nodes.iter().filter(|node| node.refines.is_empty()).collect();
    expand_all(nodes.len(), &nodes, &open, 0, &roots)
}

/// The fold mark: `▸` folded, `▾` open, `·` nothing refines it.
fn fold_mark(nodes: &[Node], open: &[String], node: &Node) -> char {
    match (children_of(nodes, &node.id).is_empty(), open.contains(&node.id)) {
        (true, _) => '·',
        (false, true) => '▾',
        (false, false) => '▸',
    }
}

/// A row of the graph is its fold mark and a row of the index, indented under
/// what it refines. The fold mark of a node with something under it unfolds
/// it one level, and offers to unfold everything under it.
///
/// `graph` is what is drawn, `all` every requirement: what the requirements
/// under a row left unrun is counted over all of them, shown or not.
fn design_pieces(graph: &[Node], all: &[Node], open: &[String], row: &Row) -> Vec<Piece> {
    let mut out = vec![plain(" ".repeat(row.indent * 2))];
    let mark = fold_mark(graph, open, &row.node).to_string();
    out.push(match children_of(graph, &row.node.id).is_empty() {
        true => plain(mark),
        false => Piece {
            text: mark,
            role: Some(Role::Entry),
            actions: vec![format!("design.toggle {}", row.node.id), format!("design.expand_all {}", row.node.id)],
        },
    });
    out.push(plain(" "));
    out.extend(row_pieces(all, &row.node));
    out
}

/// The buttons above the graph: unfold all of it, fold it to its roots, and
/// show only what is still to do or everything again.
pub fn design_header(open_only: bool) -> Vec<(String, String)> {
    vec![
        ("[ expand all ]".into(), "design.expand_everything".into()),
        ("[ fold all ]".into(), "design.fold_all".into()),
        ((if open_only { "[ show all ]" } else { "[ open only ]" }).into(), "design.open_only".into()),
    ]
}

/// The refinement graph as a buffer: the buttons, then one row a requirement,
/// indented under what it refines; with `open_only`, only the requirements
/// with something to do in them or under them.
///
/// @implements REQ-SHOW.graph_from_refinement
/// @implements REQ-SHOW.producer_is_pure
/// @implements REQ-SHOW.well_formed_by_construction
/// @implements REQ-SHOW.open_filtered
/// @drt REQ-SHOW.graph_from_refinement
/// @drt REQ-SHOW.open_filtered
pub fn design_buffer(nodes: Vec<Node>, opened: Vec<String>, open_only: bool) -> Buffer {
    let graph = shown(&nodes, open_only);
    let rows = design_rows(graph.clone(), opened.clone());
    let mut header = Vec::new();
    for (i, (label, action)) in design_header(open_only).into_iter().enumerate() {
        if i > 0 {
            header.push(plain("  "));
        }
        header.push(Piece { text: label, role: Some(Role::Entry), actions: vec![action] });
    }
    let lines: Vec<Vec<Piece>> = std::iter::once(header)
        .chain(rows.iter().map(|row| design_pieces(&graph, &nodes, &opened, row)))
        .collect();
    let mut spans = Vec::new();
    let mut at = 0usize;
    for line in &lines {
        spans.extend(pieces_spans(at, line));
        at += pieces_text(line).chars().count() + 1;
    }
    let text = lines.iter().map(|line| pieces_text(line)).collect::<Vec<_>>().join("\n");
    let size = text.chars().count();
    Buffer {
        id: "menu:design".to_string(),
        kind: BufferKind::Menu { title: "design".to_string() },
        text,
        spans: tidy(size, spans),
    }
}

/// `id` and everything under it, for unfolding all of it at once; every node
/// with something under it when `id` is `None`. Visited once each, so a cycle
/// in `refines` ends.
pub fn design_unfolded(nodes: &[Node], id: Option<&str>) -> Vec<String> {
    let mut todo: Vec<String> = match id {
        Some(id) => vec![id.to_string()],
        None => nodes.iter().map(|n| n.id.clone()).collect(),
    };
    let mut out: Vec<String> = Vec::new();
    while let Some(next) = todo.pop() {
        if out.contains(&next) {
            continue;
        }
        todo.extend(children_of(nodes, &next).into_iter().map(|n| n.id.clone()));
        out.push(next);
    }
    out.retain(|id| !children_of(nodes, id).is_empty());
    out.sort();
    out
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

    /// The text a span covers.
    fn under(buffer: &Buffer, span: &Span) -> String {
        buffer.text.chars().skip(span.start).take(span.stop - span.start).collect()
    }

    /// A row counts the clauses holding each mark, each count in the colour of
    /// its mark, and says how much of its code tests ran.
    #[test]
    fn the_index_counts_each_mark_in_its_colour() {
        let node = Node {
            id: "REQ-A".into(),
            title: "A thing".into(),
            refines: vec![],
            marks: crate::evidence::Marks { annotated: true, ..crate::evidence::Marks::NONE },
            clauses: 4,
            modelled: 4,
            held: Tally { implemented: 3, annotated: 4, ..Tally::default() },
            covered: None,
        };
        let shown = requirements_buffer(vec![node.clone()], false);
        assert_eq!(plain_text(shown.clone()), vec!["A---  REQ-A  I3/4 A4/4 J0/4 T0/4 P0/4  A thing"]);
        let implemented = shown.spans.iter().find(|s| matches!(s.role, Role::Claim { .. })).expect("a count");
        assert_eq!(under(&shown, implemented), "I3/4");
        let tested = shown
            .spans
            .iter()
            .find(|s| s.role == Role::Mark { mark: crate::evidence::Mark::Tested, held: true })
            .expect("the tested count, in the tested colour");
        assert_eq!(under(&shown, tested), "T0/4");
        // Each position of the marks is its own span, a gap told from a mark.
        let gaps: Vec<bool> = shown.spans[..4]
            .iter()
            .filter_map(|s| match s.role { Role::Mark { held, .. } => Some(held), _ => None })
            .collect();
        assert_eq!(gaps, vec![true, false, false, false]);
        assert!(faults(shown).is_empty());
        // Measured, the row says how much of its code ran, in red when not all.
        let measured = requirements_buffer(vec![Node { covered: Some((4, 7)), ..node.clone() }], false);
        assert_eq!(plain_text(measured.clone()), vec!["A---  REQ-A  I3/4 A4/4 J0/4 T0/4 P0/4  lines 57%  A thing"]);
        let lines = measured.spans.iter().find(|s| s.role == Role::Removed).expect("a coverage span");
        assert_eq!(under(&measured, lines), "lines 57%");
        assert_eq!(lines.actions, vec!["trace.coverage REQ-A".to_string()], "a coverage figure offered a diff's actions");
        // Code with no line a test could run is not all covered.
        let nothing = requirements_buffer(vec![Node { covered: Some((0, 0)), ..node }], false);
        assert_eq!(plain_text(nothing.clone()), vec!["A---  REQ-A  I3/4 A4/4 J0/4 T0/4 P0/4  lines none  A thing"]);
        assert!(!nothing.spans.iter().any(|s| matches!(s.role, Role::Added | Role::Removed)));
    }

    /// A structural requirement writes its test and proof as not applicable,
    /// counts nothing it cannot hold, and with every clause judged is done.
    ///
    /// @tests REQ-EVID.structural_not_applicable
    #[test]
    fn a_structural_requirement_counts_only_what_applies() {
        let marks = crate::evidence::structural(crate::evidence::Marks {
            annotated: true,
            judged: true,
            ..crate::evidence::Marks::NONE
        });
        let node = Node {
            id: "ARCH-X".into(),
            title: "t".into(),
            refines: vec![],
            marks,
            clauses: 2,
            modelled: 0,
            held: Tally { annotated: 2, judged: 2, ..Tally::default() },
            covered: None,
        };
        let shown = requirements_buffer(vec![node.clone()], false);
        assert_eq!(plain_text(shown.clone()), vec!["AJ··  ARCH-X  I· A2/2 J2/2 T· P·  t"]);
        assert_eq!(shown.spans.iter().filter(|s| matches!(s.role, Role::Mark { .. })).count(), 4);
        assert!(!is_open(&node));
        assert!(requirements_buffer(vec![node], true).text.is_empty());
    }

    fn node(id: &str, refines: &[&str], covered: Option<(u64, u64)>) -> Node {
        Node {
            id: id.into(),
            title: "t".into(),
            refines: refines.iter().map(|s| s.to_string()).collect(),
            marks: crate::evidence::Marks { annotated: true, judged: true, tested: true, proved: true, structural: false },
            clauses: 1,
            modelled: 1,
            held: Tally { implemented: 1, annotated: 1, judged: 1, tested: 1, proved: 1 },
            covered,
        }
    }

    /// The design opens on its roots; a mark unfolds one level, unfolding all
    /// under a node reaches its grandchildren, and a leaf has no mark to press.
    #[test]
    fn the_design_unfolds_a_level_at_a_time_or_all_at_once() {
        let graph = vec![node("ARCH", &[], None), node("REQ-A", &["ARCH"], None), node("REQ-B", &["REQ-A"], None)];
        let header = "[ expand all ]  [ fold all ]  [ open only ]";
        let folded = design_buffer(graph.clone(), vec![], false);
        assert_eq!(plain_text(folded.clone()), vec![header, "▸ AJTP  ARCH  I1/1 A1/1 J1/1 T1/1 P1/1  t"]);
        let toggle = folded.spans.iter().find(|s| s.actions.first().is_some_and(|a| a == "design.toggle ARCH"));
        assert_eq!(toggle.map(|s| (s.start, s.stop)), Some((44, 45)));
        let only = folded.spans.iter().find(|s| s.actions == vec!["design.open_only".to_string()]).expect("a button");
        assert_eq!(under(&folded, only), "[ open only ]");
        assert!(faults(folded).is_empty());

        let one = design_buffer(graph.clone(), vec!["ARCH".into()], false);
        assert_eq!(
            plain_text(one)[1..],
            ["▾ AJTP  ARCH  I1/1 A1/1 J1/1 T1/1 P1/1  t", "  ▸ AJTP  REQ-A  I1/1 A1/1 J1/1 T1/1 P1/1  t"]
        );

        let all = design_buffer(graph.clone(), design_unfolded(&graph, Some("ARCH")), false);
        assert_eq!(plain_text(all.clone()).len(), 4);
        assert_eq!(plain_text(all.clone())[3], "    · AJTP  REQ-B  I1/1 A1/1 J1/1 T1/1 P1/1  t");
        assert!(!all.spans.iter().any(|s| s.actions.iter().any(|a| a == "design.toggle REQ-B")));
        assert!(faults(all).is_empty());
        assert_eq!(design_unfolded(&graph, None), vec!["ARCH".to_string(), "REQ-A".to_string()]);
    }

    /// A row whose own lines all ran says what the requirements under it left
    /// unrun, over itself and all of them; one whose children ran everything
    /// says nothing more.
    ///
    /// @tests REQ-SHOW.children_lines
    #[test]
    fn a_row_says_what_the_requirements_under_it_left_unrun() {
        let graph = vec![
            node("ARCH", &[], Some((10, 10))),
            node("REQ-A", &["ARCH"], Some((5, 10))),
            node("REQ-B", &["REQ-A"], Some((0, 10))),
        ];
        let shown = plain_text(design_buffer(graph.clone(), design_unfolded(&graph, None), false));
        // ARCH: 15 of 30 lines under it ran, with its own 10 of 10.
        assert_eq!(shown[1], "▾ AJTP  ARCH  I1/1 A1/1 J1/1 T1/1 P1/1  lines 100%  +children 50%  t");
        assert_eq!(shown[2], "  ▾ AJTP  REQ-A  I1/1 A1/1 J1/1 T1/1 P1/1  lines 50%  +children 25%  t");
        assert_eq!(shown[3], "    · AJTP  REQ-B  I1/1 A1/1 J1/1 T1/1 P1/1  lines 0%  t");
        let whole = vec![node("ARCH", &[], Some((10, 10))), node("REQ-A", &["ARCH"], Some((3, 3)))];
        assert!(!plain_text(design_buffer(whole, vec![], false))[1].contains("+children"));
    }

    /// With only what is open shown, a finished requirement disappears unless
    /// something under it is open, which keeps the path to what is.
    ///
    /// @tests REQ-SHOW.open_filtered
    #[test]
    fn open_only_keeps_what_is_still_to_do_and_the_path_to_it() {
        let mut short = node("REQ-B", &["REQ-A"], None);
        short.held.judged = 0;
        let graph = vec![node("ARCH", &[], None), node("REQ-A", &["ARCH"], None), short, node("REQ-C", &["ARCH"], None)];
        let open = design_buffer(graph.clone(), design_unfolded(&graph, None), true);
        let ids: Vec<String> = plain_text(open.clone())[1..]
            .iter()
            .map(|line| line.split_whitespace().nth(2).unwrap_or_default().to_string())
            .collect();
        assert_eq!(ids, vec!["ARCH", "REQ-A", "REQ-B"]);
        assert!(plain_text(open)[0].ends_with("[ show all ]"));
        let index = requirements_buffer(graph, true);
        assert_eq!(plain_text(index).len(), 1);
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
