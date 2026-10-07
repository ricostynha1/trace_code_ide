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
//! the workspace is, `○` every other state.
//!
//! @implements REQ-UNDO.tree_is_shown

use crate::surface::produce::tidy;
use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// One node as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub node: u64,
    /// The node it was made after; `None` for one made on the tree as opened.
    pub parent: Option<u64>,
    /// Whether the workspace is here.
    pub here: bool,
    /// What the change did, in words.
    pub said: String,
}

/// A row of the drawing: its graph cells, and the node it is (`None` for base).
struct Row {
    cells: Vec<char>,
    node: Option<u64>,
}

/// The rows, top to bottom: base, then each node under what it was made after.
fn rows(points: &[Point]) -> Vec<Row> {
    let children = |of: Option<u64>| -> Vec<u64> {
        points.iter().filter(|p| p.parent == of).map(|p| p.node).collect()
    };
    let here = |node: Option<u64>| match node {
        None => !points.iter().any(|p| p.here),
        Some(n) => points.iter().any(|p| p.node == n && p.here),
    };
    let mut out = Vec::new();
    // Which columns carry a line down past the row being drawn.
    let mut open: Vec<bool> = Vec::new();
    // What is still to draw: a node, its column. Last in, first drawn.
    let mut pending: Vec<(Option<u64>, usize)> = vec![(None, 0)];
    while let Some((node, col)) = pending.pop() {
        let kids = children(node);
        // A branch takes the first free column to the right of its parent.
        let mut taken: Vec<usize> = Vec::new();
        for _ in kids.iter().skip(1) {
            let mut c = col + 1;
            while open.get(c).copied().unwrap_or(false) || taken.contains(&c) {
                c += 1;
            }
            taken.push(c);
        }
        let width = taken.iter().copied().chain([col]).max().unwrap_or(col) + 1;
        if open.len() < width {
            open.resize(width, false);
        }
        let mut cells: Vec<char> = open.iter().flat_map(|o| [if *o { '│' } else { ' ' }, ' ']).collect();
        cells[col * 2] = if here(node) { '●' } else { '○' };
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
        out.push(Row { cells, node });
        open[col] = !kids.is_empty();
        for c in &taken {
            open[*c] = true;
        }
        // The first child continues down this column, drawn after the
        // branches so their rows sit between this node and it.
        if let Some(first) = kids.first() {
            pending.push((Some(*first), col));
        }
        for (kid, c) in kids.iter().skip(1).zip(&taken).rev() {
            pending.push((Some(*kid), *c));
        }
        // A column whose node has no children closes when it is drawn.
        if kids.is_empty() && col < open.len() {
            open[col] = false;
        }
    }
    out
}

/// The history as a record titled `history`.
pub fn history_view(points: &[Point]) -> Buffer {
    let drawn = rows(points);
    // Every column is a glyph and a space; the last column's space is the gap
    // before the name, written once below.
    let graph = drawn.iter().map(|r| r.cells.len()).max().unwrap_or(1) - 1;
    let mut text = String::new();
    let mut spans = Vec::new();
    for row in &drawn {
        if !text.is_empty() {
            text.push('\n');
        }
        let at = text.chars().count();
        let mut cells: String = row.cells.iter().take(graph).collect();
        cells.push_str(&" ".repeat(graph - cells.chars().count()));
        let here = row.cells.contains(&'●');
        let (name, said, action) = match row.node {
            None => ("base".to_string(), "the tree as it was opened".to_string(), "history.jump"),
            Some(n) => (
                format!("#{n}"),
                points.iter().find(|p| p.node == n).map(|p| p.said.clone()).unwrap_or_default(),
                "history.jump",
            ),
        };
        let from = at + graph + 1;
        let role = if here { Role::Heading } else { Role::Entry };
        // The base is where everything starts; it is not a node to jump to.
        let actions = if row.node.is_some() { vec![action.to_string()] } else { Vec::new() };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults};

    fn point(node: u64, parent: Option<u64>, here: bool) -> Point {
        Point { node, parent, here, said: format!("change {node}") }
    }

    /// Each point is named by its number, which goes there; the one the
    /// workspace is at is marked; a straight run of changes is one column.
    ///
    /// @tests REQ-UNDO.tree_is_shown
    #[test]
    fn each_point_goes_to_itself_and_the_current_one_is_marked() {
        let shown = history_view(&[point(0, None, false), point(1, Some(0), true)]);
        assert!(faults(shown.clone()).is_empty());
        assert_eq!(shown.text, "○ base  the tree as it was opened\n○ #0  change 0\n● #1  change 1");
        let at = shown.text[..shown.text.find("#1").unwrap()].chars().count();
        assert_eq!(actions_at(shown.clone(), at), vec!["history.jump".to_string()]);
        assert!(history_view(&[]).text.starts_with("● base"));
    }

    /// A change made after undoing opens a branch beside the run it left,
    /// joined to the node it was made after.
    #[test]
    fn an_undone_run_and_the_new_change_are_two_branches() {
        // base → #0 → #1, then undo to base and type again: #2.
        let shown = history_view(&[point(0, None, false), point(1, Some(0), false), point(2, None, true)]);
        assert_eq!(
            shown.text,
            "○─╮ base  the tree as it was opened\n│ ● #2  change 2\n○   #0  change 0\n○   #1  change 1"
        );
        assert!(faults(shown).is_empty());
    }
}
