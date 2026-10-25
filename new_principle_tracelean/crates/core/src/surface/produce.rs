//! @implements REQ-SHOW.core_produces

use serde::{Deserialize, Serialize};

use crate::observe::transcript::Event;
use crate::surface::view::{Buffer, BufferKind, Role, Span};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub start: usize,
    pub stop: usize,
    pub role: Role,
}

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
        Role::Token { .. } => &[],
        Role::Claim { .. } => &[],
    };
    names.iter().map(|name| name.to_string()).collect()
}

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

/// @implements REQ-SHOW.well_formed_by_construction
/// @implements REQ-SHOW.producers_are_total
/// @drt REQ-SHOW.well_formed_by_construction
/// @drt REQ-SHOW.producers_are_total
pub fn tidy(size: usize, spans: Vec<Span>) -> Vec<Span> {
    let mut clamped: Vec<Span> = spans
        .into_iter()
        .map(|span| Span { start: span.start.min(size), stop: span.stop.min(size), ..span })
        .collect();
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub role: Role,
    pub text: String,
}

fn common_prefix(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn marked(marker: &str, role: Role, line: &str) -> DiffLine {
    DiffLine { role, text: format!("{marker}{line}") }
}

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

fn split_lines(text: &str) -> Vec<String> {
    text.split('\n').map(str::to_string).collect()
}

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

/// @implements REQ-SHOW.sandbox_from_observation
pub fn sandbox_buffer(
    changed: Vec<String>,
    said: Vec<Event>,
    spend: crate::observe::cost::Spend,
) -> Buffer {
    record_buffer("sandbox".to_string(), sandbox_events(changed, said, spend))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub implemented: usize,
    pub annotated: usize,
    pub judged: usize,
    pub tested: usize,
    pub proved: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub title: String,
    pub refines: Vec<String>,
    pub marks: crate::evidence::Marks,
    pub clauses: usize,
    pub modelled: usize,
    pub held: Tally,
    #[serde(default)]
    pub covered: Option<(u64, u64)>,
}

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

fn children_short(nodes: &[Node], node: &Node) -> Option<String> {
    let (run, all) = children_of(nodes, &node.id).into_iter().fold((0, 0), |(run, all), child| {
        let (r, a) = lines_under(nodes.len(), nodes, child);
        (run + r, all + a)
    });
    let (own_run, own_all) = node.covered.unwrap_or((0, 0));
    (run < all).then(|| format!("+children {}%", (own_run + run) * 100 / (own_all + all)))
}

fn count_piece(letter: char, held: usize, of: usize, role: Role) -> Piece {
    match of {
        0 => plain(format!("{letter}·")),
        _ => spanned(format!("{letter}{held}/{of}"), role),
    }
}

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

pub fn is_open(node: &Node) -> bool {
    let t = node.held;
    t.implemented < node.modelled
        || t.tested < node.modelled
        || t.proved < node.modelled
        || t.annotated < node.clauses
        || t.judged < node.clauses
        || node.covered.is_some_and(|(run, all)| run < all)
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub indent: usize,
    pub node: Node,
}

fn open_under(fuel: usize, nodes: &[Node], node: &Node) -> bool {
    is_open(node)
        || (fuel > 0 && children_of(nodes, &node.id).into_iter().any(|child| open_under(fuel - 1, nodes, child)))
}

fn shown(nodes: &[Node], open_only: bool) -> Vec<Node> {
    nodes.iter().filter(|node| !open_only || open_under(nodes.len(), nodes, node)).cloned().collect()
}

/// @implements REQ-SHOW.index_from_requirements
/// @implements REQ-SHOW.producer_is_pure
/// @implements REQ-SHOW.well_formed_by_construction
/// @implements REQ-SHOW.open_filtered
/// @implements REQ-SHOW.children_lines
/// @drt REQ-SHOW.index_from_requirements
/// @drt REQ-SHOW.children_lines
pub fn requirements_buffer(nodes: Vec<Node>, open_only: bool) -> Buffer {
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

pub fn design_rows(nodes: Vec<Node>, open: Vec<String>) -> Vec<Row> {
    let roots: Vec<&Node> = nodes.iter().filter(|node| node.refines.is_empty()).collect();
    expand_all(nodes.len(), &nodes, &open, 0, &roots)
}

fn fold_mark(nodes: &[Node], open: &[String], node: &Node) -> char {
    match (children_of(nodes, &node.id).is_empty(), open.contains(&node.id)) {
        (true, _) => '·',
        (false, true) => '▾',
        (false, false) => '▸',
    }
}

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

pub fn design_header(open_only: bool) -> Vec<(String, String)> {
    vec![
        ("[ expand all ]".into(), "design.expand_everything".into()),
        ("[ fold all ]".into(), "design.fold_all".into()),
        ((if open_only { "[ show all ]" } else { "[ open only ]" }).into(), "design.open_only".into()),
    ]
}

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

fn line_start(lines: &[String], start: usize) -> usize {
    lines.iter().take(start).map(|line| line.chars().count() + 1).sum()
}

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

    fn under(buffer: &Buffer, span: &Span) -> String {
        buffer.text.chars().skip(span.start).take(span.stop - span.start).collect()
    }

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
        let gaps: Vec<bool> = shown.spans[..4]
            .iter()
            .filter_map(|s| match s.role { Role::Mark { held, .. } => Some(held), _ => None })
            .collect();
        assert_eq!(gaps, vec![true, false, false, false]);
        assert!(faults(shown).is_empty());
        let measured = requirements_buffer(vec![Node { covered: Some((4, 7)), ..node.clone() }], false);
        assert_eq!(plain_text(measured.clone()), vec!["A---  REQ-A  I3/4 A4/4 J0/4 T0/4 P0/4  lines 57%  A thing"]);
        let lines = measured.spans.iter().find(|s| s.role == Role::Removed).expect("a coverage span");
        assert_eq!(under(&measured, lines), "lines 57%");
        assert_eq!(lines.actions, vec!["trace.coverage REQ-A".to_string()], "a coverage figure offered a diff's actions");
        let nothing = requirements_buffer(vec![Node { covered: Some((0, 0)), ..node }], false);
        assert_eq!(plain_text(nothing.clone()), vec!["A---  REQ-A  I3/4 A4/4 J0/4 T0/4 P0/4  lines none  A thing"]);
        assert!(!nothing.spans.iter().any(|s| matches!(s.role, Role::Added | Role::Removed)));
    }

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

    /// @tests REQ-SHOW.children_lines
    #[test]
    fn a_row_says_what_the_requirements_under_it_left_unrun() {
        let graph = vec![
            node("ARCH", &[], Some((10, 10))),
            node("REQ-A", &["ARCH"], Some((5, 10))),
            node("REQ-B", &["REQ-A"], Some((0, 10))),
        ];
        let shown = plain_text(design_buffer(graph.clone(), design_unfolded(&graph, None), false));
        assert_eq!(shown[1], "▾ AJTP  ARCH  I1/1 A1/1 J1/1 T1/1 P1/1  lines 100%  +children 50%  t");
        assert_eq!(shown[2], "  ▾ AJTP  REQ-A  I1/1 A1/1 J1/1 T1/1 P1/1  lines 50%  +children 25%  t");
        assert_eq!(shown[3], "    · AJTP  REQ-B  I1/1 A1/1 J1/1 T1/1 P1/1  lines 0%  t");
        let whole = vec![node("ARCH", &[], Some((10, 10))), node("REQ-A", &["ARCH"], Some((3, 3)))];
        assert!(!plain_text(design_buffer(whole, vec![], false))[1].contains("+children"));
    }

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
        assert_eq!(buffer.spans.len(), 2);
        for span in &buffer.spans {
            assert_eq!(span.actions, vec!["observe.accept", "observe.reject"]);
        }
    }

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
        assert_eq!(middle.spans.len(), 1);
        assert_eq!((middle.spans[0].start, middle.spans[0].stop), (0, 3));
        assert_eq!(middle.id, whole.id);

        assert_eq!(window(whole.clone(), 0, 9).text, whole.text);
        assert_eq!(window(whole.clone(), 9, 4).text, "");
        assert_eq!(window(whole.clone(), 0, 0).text, "");

        let straddling = file_buffer(
            "a.rs".into(),
            "one\ntwo\nthree".into(),
            vec![mark(0, 12, Role::Heading)],
        );
        let cut = window(straddling, 1, 1);
        assert_eq!((cut.spans[0].start, cut.spans[0].stop), (0, 3));
        assert_eq!(faults(cut), vec![]);
    }

    /// @tests REQ-SHOW.well_formed_by_construction
    #[test]
    fn a_span_is_measured_in_characters_not_bytes() {
        let buffer = record_buffer(
            "wide".into(),
            vec![Event { kind: "é".into(), text: "ü".into() }],
        );
        assert_eq!(faults(buffer.clone()), vec![]);
        assert_eq!(buffer.spans[0].stop, 4);
        assert_eq!(buffer.text.len(), 6);
    }
}
