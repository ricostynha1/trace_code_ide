//! The history tree, drawn as a tree a pointer can go back through.
//!
//! Every change is a node; the row names it by number (`#12`) and says what it
//! did. The number carries `history.jump`, so a click goes straight to that
//! point the way undo and redo go one step — through the tree's own path via
//! the nearest common ancestor.
//!
//! Drawn as the first TraceLean drew it: a change made after a node continues
//! straight down from it, and a change made after undoing back to a node opens
//! a branch in a column of its own, joined to that node by `─╮`. `●` is where
//! the workspace is, `○` every other state. Above it, the view chooses what to
//! show — every node, those touching the open file, or those at which the work
//! was saved — and pointing at a node shows the change it made.
//!
//! @implements REQ-UNDO.tree_is_shown

use serde::{Deserialize, Serialize};

use crate::surface::produce::{diff_lines, tidy};
use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// One node as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub node: u64,
    /// The node it was made after; `None` for one made on the tree as opened.
    pub parent: Option<u64>,
    /// Whether the workspace is here.
    pub here: bool,
    /// What the change did, in words.
    pub said: String,
    /// The file it touched, when it touched one.
    pub file: Option<String>,
    /// Whether the work was saved at this node.
    pub saved: bool,
}

/// A row of the drawing: the node it is (`None` for the tree as opened) and
/// its graph cells, two to a column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRow {
    pub node: Option<u64>,
    pub cells: String,
}

/// Which nodes the view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    All,
    File,
    Saved,
}

const FILTERS: [(Filter, &str); 3] = [(Filter::All, "All"), (Filter::File, "File"), (Filter::Saved, "Saved")];

/// The filter a switch's label names.
pub fn filter_named(name: &str) -> Option<Filter> {
    FILTERS.iter().find(|(_, label)| *label == name).map(|(f, _)| *f)
}

fn is_here(points: &[Point], node: Option<u64>) -> bool {
    match node {
        None => !points.iter().any(|p| p.here),
        Some(n) => points.iter().any(|p| p.node == n && p.here),
    }
}

/// The first column from `from` on that no line runs through and no branch
/// of this row has taken.
fn free_column(open: &[bool], taken: &[usize], from: usize) -> usize {
    let mut c = from;
    while open.get(c).copied().unwrap_or(false) || taken.contains(&c) {
        c += 1;
    }
    c
}

/// The tree as rows, top to bottom: the tree as opened, then each node under
/// what it was made after, each once.
///
/// What is still to draw is a stack, so a node's branches come before the run
/// that continues its column, and each row says which columns carry a line
/// past it. A node already drawn is not drawn again, and there is at most a
/// row a node and one for the start — so a history whose parents name each
/// other, or a node twice, still draws in finite rows.
///
/// @implements REQ-UNDO.tree_is_drawn
/// @drt REQ-UNDO.tree_is_drawn
pub fn history_graph(points: Vec<Point>) -> Vec<GraphRow> {
    let mut out = Vec::new();
    let mut open: Vec<bool> = Vec::new();
    let mut pending: Vec<(Option<u64>, usize)> = vec![(None, 0)];
    let mut drawn: Vec<u64> = Vec::new();
    for _ in 0..points.len() + 1 {
        let Some((node, col)) = pending.pop() else { break };
        if let Some(n) = node {
            if drawn.contains(&n) {
                continue;
            }
            drawn.push(n);
        }
        let mut kids: Vec<u64> = Vec::new();
        for p in &points {
            if p.parent == node && !drawn.contains(&p.node) && !kids.contains(&p.node) {
                kids.push(p.node);
            }
        }
        let mut taken: Vec<usize> = Vec::new();
        for _ in kids.iter().skip(1) {
            let c = free_column(&open, &taken, col + 1);
            taken.push(c);
        }
        let width = taken.iter().fold(col + 1, |w, c| w.max(c + 1)).max(open.len());
        open.resize(width, false);
        let mut cells: Vec<char> = open.iter().flat_map(|o| [if *o { '│' } else { ' ' }, ' ']).collect();
        cells[col * 2] = if is_here(&points, node) { '●' } else { '○' };
        if let Some(last) = taken.last().copied() {
            for c in col * 2 + 1..last * 2 {
                if cells[c] == ' ' {
                    cells[c] = '─';
                }
            }
            for c in &taken {
                cells[c * 2] = if *c == last { '╮' } else { '┬' };
            }
        }
        out.push(GraphRow { node, cells: cells.into_iter().collect() });
        open[col] = !kids.is_empty();
        for c in &taken {
            open[*c] = true;
        }
        if let Some(first) = kids.first() {
            pending.push((Some(*first), col));
        }
        for (kid, c) in kids.iter().skip(1).zip(&taken).rev() {
            pending.push((Some(*kid), *c));
        }
    }
    out
}

fn keep(filter: Filter, file: &str, p: &Point) -> bool {
    match filter {
        Filter::All => true,
        Filter::File => p.file.as_deref() == Some(file),
        Filter::Saved => p.saved,
    }
}

/// The nearest node from `start` up, `start` included, that the filter keeps.
fn kept_ancestor(points: &[Point], filter: Filter, file: &str, start: Option<u64>) -> Option<u64> {
    let mut at = start;
    for _ in 0..points.len() + 1 {
        let n = at?;
        let p = points.iter().find(|p| p.node == n)?;
        if keep(filter, file, p) {
            return Some(n);
        }
        at = p.parent;
    }
    None
}

/// The nodes `filter` keeps, each under its nearest kept ancestor, and the
/// one nearest the workspace's position marked as where it is.
///
/// @implements REQ-UNDO.filtered_view
/// @drt REQ-UNDO.filtered_view
pub fn shown_points(points: Vec<Point>, filter: Filter, file: String) -> Vec<Point> {
    let current = points.iter().find(|p| p.here).map(|p| p.node);
    let target = kept_ancestor(&points, filter, &file, current);
    points
        .iter()
        .filter(|p| keep(filter, &file, p))
        .map(|p| Point {
            parent: kept_ancestor(&points, filter, &file, p.parent),
            here: Some(p.node) == target,
            ..p.clone()
        })
        .collect()
}

/// The history as a record titled `history`: the filter switches, then the
/// tree as `filter` shows it. `file` is the file the document shows, which
/// the `File` switch keeps to.
pub fn history_view(points: &[Point], filter: Filter, file: Option<&str>) -> Buffer {
    let shown = shown_points(points.to_vec(), filter, file.unwrap_or_default().to_string());
    let drawn = history_graph(shown.clone());
    let mut text = String::new();
    let mut spans = Vec::new();
    // The switches, the chosen one marked.
    for (f, label) in FILTERS {
        let at = text.chars().count();
        let role = if f == filter { Role::Heading } else { Role::Entry };
        spans.push(Span { start: at, stop: at + label.len(), role, actions: vec!["history.filter".to_string()] });
        text.push_str(label);
        text.push_str("  ");
    }
    if let (Filter::File, Some(file)) = (filter, file) {
        text.push_str(file);
    }
    // Every column is a glyph and a space; the last column's space is the gap
    // before the name, written once below.
    let graph = drawn.iter().map(|r| r.cells.chars().count()).max().unwrap_or(1).saturating_sub(1);
    for row in &drawn {
        text.push('\n');
        let at = text.chars().count();
        let mut cells: String = row.cells.chars().take(graph).collect();
        cells.push_str(&" ".repeat(graph - cells.chars().count()));
        let here = row.cells.contains('●');
        let (name, said) = match row.node {
            None => ("base".to_string(), "the tree as it was opened".to_string()),
            Some(n) => (format!("#{n}"), shown.iter().find(|p| p.node == n).map(|p| p.said.clone()).unwrap_or_default()),
        };
        let from = at + graph + 1;
        let role = if here { Role::Heading } else { Role::Entry };
        // The base is where everything starts; it is not a node to jump to.
        let actions = if row.node.is_some() { vec!["history.jump".to_string()] } else { Vec::new() };
        spans.push(Span { start: from, stop: from + name.chars().count(), role, actions });
        text.push_str(&format!("{cells} {name}  {said}"));
    }
    let size = text.chars().count();
    Buffer {
        id: "record:history".to_string(),
        kind: BufferKind::Record { title: "history".to_string() },
        text,
        spans: tidy(size, spans),
    }
}

/// The change a node made, as a record a pointer resting on the node shows:
/// what it did, then for each file it touched the lines it added and removed
/// with two lines either side. `changed` is each file's path, text before and
/// text after.
///
/// @implements REQ-UNDO.hover_shows_change
pub fn change_view(node: u64, said: &str, changed: &[(String, String, String)]) -> Buffer {
    let mut lines: Vec<(String, Role)> = vec![(format!("#{node}  {said}"), Role::Heading)];
    for (path, before, after) in changed {
        lines.push((path.clone(), Role::Path));
        let diff = diff_lines(
            before.split('\n').map(str::to_string).collect(),
            after.split('\n').map(str::to_string).collect(),
        );
        let near = |i: usize| {
            diff[i.saturating_sub(2)..(i + 3).min(diff.len())].iter().any(|d| d.role != Role::Plain)
        };
        let mut skipped = false;
        for (i, d) in diff.iter().enumerate() {
            if near(i) {
                lines.push((d.text.clone(), d.role));
                skipped = false;
            } else if !skipped {
                lines.push(("…".to_string(), Role::Plain));
                skipped = true;
            }
        }
    }
    let mut text = String::new();
    let mut spans = Vec::new();
    for (line, role) in lines {
        if !text.is_empty() {
            text.push('\n');
        }
        let at = text.chars().count();
        if role != Role::Plain {
            spans.push(Span { start: at, stop: at + line.chars().count(), role, actions: Vec::new() });
        }
        text.push_str(&line);
    }
    let size = text.chars().count();
    Buffer {
        id: format!("record:change #{node}"),
        kind: BufferKind::Record { title: format!("change #{node}") },
        text,
        spans: tidy(size, spans),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults};

    fn point(node: u64, parent: Option<u64>, here: bool) -> Point {
        Point { node, parent, here, said: format!("change {node}"), file: Some("a.rs".into()), saved: false }
    }

    fn tree_of(shown: &Buffer) -> String {
        shown.text.split_once('\n').map(|(_, t)| t.to_string()).unwrap_or_default()
    }

    /// Each point is named by its number, which goes there; the one the
    /// workspace is at is marked; a straight run of changes is one column.
    ///
    /// @tests REQ-UNDO.tree_is_shown
    #[test]
    fn each_point_goes_to_itself_and_the_current_one_is_marked() {
        let shown = history_view(&[point(0, None, false), point(1, Some(0), true)], Filter::All, None);
        assert!(faults(shown.clone()).is_empty());
        assert_eq!(tree_of(&shown), "○ base  the tree as it was opened\n○ #0  change 0\n● #1  change 1");
        let at = shown.text[..shown.text.find("#1").unwrap()].chars().count();
        assert_eq!(actions_at(shown.clone(), at), vec!["history.jump".to_string()]);
        assert!(tree_of(&history_view(&[], Filter::All, None)).starts_with("● base"));
        assert_eq!(actions_at(shown, 0), vec!["history.filter".to_string()]);
    }

    /// A change made after undoing opens a branch beside the run it left,
    /// joined to the node it was made after.
    ///
    /// @tests REQ-UNDO.tree_is_drawn
    #[test]
    fn an_undone_run_and_the_new_change_are_two_branches() {
        // base → #0 → #1, then undo to base and type again: #2.
        let shown = history_view(&[point(0, None, false), point(1, Some(0), false), point(2, None, true)], Filter::All, None);
        assert_eq!(
            tree_of(&shown),
            "○─╮ base  the tree as it was opened\n│ ● #2  change 2\n○   #0  change 0\n○   #1  change 1"
        );
        assert!(faults(shown).is_empty());
    }

    /// Parents that name each other, or a node itself, still draw each node
    /// once, in finite rows.
    #[test]
    fn a_history_that_loops_is_drawn_once() {
        let rows = history_graph(vec![point(0, Some(0), false), point(1, None, false), point(1, Some(1), true)]);
        assert_eq!(rows.iter().filter(|r| r.node == Some(1)).count(), 1);
        assert!(rows.len() <= 4);
    }

    /// Kept to the saved points, each hangs under its nearest saved ancestor,
    /// and the one nearest where the workspace is stands for it.
    ///
    /// @tests REQ-UNDO.filtered_view
    #[test]
    fn a_filter_keeps_its_nodes_under_their_nearest_kept_ancestor() {
        let mut points = vec![point(0, None, false), point(1, Some(0), false), point(2, Some(1), false), point(3, Some(2), true)];
        points[0].saved = true;
        points[2].saved = true;
        let shown = shown_points(points.clone(), Filter::Saved, String::new());
        assert_eq!(shown.iter().map(|p| (p.node, p.parent, p.here)).collect::<Vec<_>>(), vec![(0, None, false), (2, Some(0), true)]);
        points[1].file = Some("b.rs".into());
        let only_b = shown_points(points, Filter::File, "b.rs".into());
        assert_eq!(only_b.iter().map(|p| (p.node, p.parent, p.here)).collect::<Vec<_>>(), vec![(1, None, true)]);
    }

    /// Pointing at a node shows the lines it changed, near them only.
    ///
    /// @tests REQ-UNDO.hover_shows_change
    #[test]
    fn a_change_shows_its_lines_and_little_else() {
        let before = (1..=20).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        let after = before.replace("line 10", "line ten");
        let shown = change_view(4, "typed", &[("a.rs".into(), before, after)]);
        assert!(faults(shown.clone()).is_empty());
        assert!(shown.text.contains("-line 10") && shown.text.contains("+line ten"), "{}", shown.text);
        assert!(!shown.text.contains("line 2\n"), "far lines are left out: {}", shown.text);
    }
}
