//! The history tree, as rows a pointer can go back to.
//!
//! Every change is a node; the row names it by number (`#12`) and says what it
//! did. The number carries `history.jump`, so a click goes straight to that
//! point the way undo and redo go one step — through the tree's own path via
//! the nearest common ancestor. The row the workspace is at is marked.
//!
//! @implements REQ-UNDO.tree_is_shown

use crate::surface::produce::tidy;
use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// One node as the view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub node: u64,
    /// How many branches were taken to reach it.
    pub depth: usize,
    /// Whether the workspace is here.
    pub here: bool,
    /// What the change did, in words.
    pub said: String,
}

/// The history as a record titled `history`. `at_base` says the workspace is
/// at the tree as it was opened, before any node.
pub fn history_view(points: &[Point], at_base: bool) -> Buffer {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut push = |text: &mut String, line: String, span: Option<(usize, usize, Role, &str)>| {
        let at = if text.is_empty() { 0 } else { text.chars().count() + 1 };
        if !text.is_empty() {
            text.push('\n');
        }
        if let Some((from, to, role, action)) = span {
            spans.push(Span {
                start: at + from,
                stop: at + to,
                role,
                actions: if action.is_empty() { Vec::new() } else { vec![action.to_string()] },
            });
        }
        text.push_str(&line);
    };
    let base = format!("{} base  the tree as it was opened", if at_base { "●" } else { " " });
    push(&mut text, base, Some((2, 6, Role::Heading, "")));
    for point in points {
        let mark = if point.here { "●" } else { " " };
        let name = format!("#{}", point.node);
        let line = format!("{mark} {}{name}  {}", "  ".repeat(point.depth), point.said);
        let from = 2 + 2 * point.depth;
        let role = if point.here { Role::Heading } else { Role::Entry };
        push(&mut text, line, Some((from, from + name.chars().count(), role, "history.jump")));
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

    /// Each point is named by its number, which goes there; the one the
    /// workspace is at is marked.
    ///
    /// @tests REQ-UNDO.tree_is_shown
    #[test]
    fn each_point_goes_to_itself_and_the_current_one_is_marked() {
        let points = vec![
            Point { node: 0, depth: 0, here: false, said: "made a.rs".into() },
            Point { node: 1, depth: 1, here: true, said: "typed \"x\" in a.rs".into() },
        ];
        let shown = history_view(&points, false);
        assert!(faults(shown.clone()).is_empty());
        assert_eq!(shown.text, "  base  the tree as it was opened\n  #0  made a.rs\n●   #1  typed \"x\" in a.rs");
        let at = shown.text[..shown.text.find("#1").unwrap()].chars().count();
        assert_eq!(actions_at(shown.clone(), at), vec!["history.jump".to_string()]);
        assert!(history_view(&[], true).text.starts_with("● base"));
    }
}
