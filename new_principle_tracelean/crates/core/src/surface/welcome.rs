//! The page a project opens on: where to start, then the folders opened
//! before, each opening as the project when clicked.

use crate::surface::produce::{menu_buffer, tidy, MenuEntry};
use crate::surface::view::{Buffer, Role, Span};

/// The starts as a menu, and under a `Recent` heading each folder in
/// `recent` as a path that opens it.
///
/// @implements REQ-SHOW.recent_reopens
pub fn welcome(starts: Vec<MenuEntry>, recent: &[String]) -> Buffer {
    let mut buffer = menu_buffer("welcome".into(), starts);
    if recent.is_empty() {
        return buffer;
    }
    let mut text = buffer.text.clone();
    let mut spans = buffer.spans.clone();
    let mut push = |text: &mut String, piece: &str, role: Role, actions: &[&str]| {
        let at = text.chars().count();
        text.push_str(piece);
        if role != Role::Plain || !actions.is_empty() {
            let stop = at + piece.chars().count();
            spans.push(Span { start: at, stop, role, actions: actions.iter().map(|a| a.to_string()).collect() });
        }
    };
    push(&mut text, "\n\n", Role::Plain, &[]);
    push(&mut text, "Recent", Role::Heading, &[]);
    for folder in recent {
        push(&mut text, "\n  ", Role::Plain, &[]);
        push(&mut text, folder, Role::Path, &["file.open"]);
    }
    let size = text.chars().count();
    buffer.text = text;
    buffer.spans = tidy(size, spans);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::plain_text;

    /// @tests REQ-SHOW.recent_reopens
    #[test]
    fn each_recent_folder_is_a_path_that_opens_it() {
        let start = MenuEntry { key: "a".into(), description: "Start".into(), action: Some("sandbox.new".into()) };
        let shown = welcome(vec![start], &["/home/u/one".into(), "/home/u/two".into()]);
        assert_eq!(plain_text(shown.clone()), vec!["a  Start", "", "Recent", "  /home/u/one", "  /home/u/two"]);
        let chars: Vec<char> = shown.text.chars().collect();
        let opens: Vec<String> = shown
            .spans
            .iter()
            .filter(|s| s.actions == ["file.open"])
            .map(|s| chars[s.start..s.stop].iter().collect())
            .collect();
        assert_eq!(opens, vec!["/home/u/one", "/home/u/two"]);
    }

    #[test]
    fn with_nothing_recent_it_is_the_starts_alone() {
        let start = MenuEntry { key: "a".into(), description: "Start".into(), action: None };
        assert_eq!(welcome(vec![start.clone()], &[]), menu_buffer("welcome".into(), vec![start]));
    }
}
