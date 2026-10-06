//! The page a project opens on: where to start, then the folders opened
//! before, each opening as the project when clicked.

use crate::surface::produce::{menu_buffer, tidy, MenuEntry};
use crate::surface::view::{Buffer, Role, Span};

/// The starts as a menu, and under a `Recent` heading each folder in
/// `recent` as a path that opens it.
///
/// @implements REQ-SHOW.recent_reopens
/// What the page says before its starts: what this editor is, and where to
/// look first.
pub const INTRO: &[&str] = &[
    "TraceLean — an editor that knows why your code exists",
    "",
    "Requirements live in reqs/. Code, tests and Lean models name the clause",
    "they serve (@implements, @tests, @models, @proves); the gutter chips and",
    "the letters in the file tree show where each clause is met.",
    "",
    "🔗 on the left shows what the open file claims and what else claims it;",
    "📋 lists the requirements. Put the cursor on a requirement name and the",
    "bar at the bottom offers to open it, or to gather what an agent needs to",
    "change it (Space c o). Space shows every key; F1 lists them all.",
    "",
    "Start here",
];

pub fn welcome(starts: Vec<MenuEntry>, recent: &[String]) -> Buffer {
    let menu = menu_buffer("welcome".into(), starts);
    // The introduction first, the starts' spans moved down past it.
    let intro = INTRO.join("\n") + "\n";
    let shift = intro.chars().count();
    let title = INTRO[0].chars().count();
    let heading = INTRO[INTRO.len() - 1];
    let heading_at = shift - 1 - heading.chars().count();
    let mut spans = vec![
        Span { start: 0, stop: title, role: Role::Heading, actions: Vec::new() },
        Span { start: heading_at, stop: heading_at + heading.chars().count(), role: Role::Heading, actions: Vec::new() },
    ];
    spans.extend(menu.spans.iter().map(|s| Span { start: s.start + shift, stop: s.stop + shift, ..s.clone() }));
    let mut text = intro + &menu.text;
    let mut buffer = Buffer { text: text.clone(), spans: spans.clone(), ..menu };
    if recent.is_empty() {
        let size = buffer.text.chars().count();
        buffer.spans = tidy(size, spans);
        return buffer;
    }
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
        let lines = plain_text(shown.clone());
        assert_eq!(lines[..INTRO.len()], INTRO.iter().map(|l| l.to_string()).collect::<Vec<_>>()[..]);
        assert_eq!(lines[INTRO.len()..], ["a  Start", "", "Recent", "  /home/u/one", "  /home/u/two"]);
        let chars: Vec<char> = shown.text.chars().collect();
        let opens: Vec<String> = shown
            .spans
            .iter()
            .filter(|s| s.actions == ["file.open"])
            .map(|s| chars[s.start..s.stop].iter().collect())
            .collect();
        assert_eq!(opens, vec!["/home/u/one", "/home/u/two"]);
    }

    /// With nothing recent it is the introduction and the starts, each start
    /// still doing what it did.
    #[test]
    fn with_nothing_recent_it_is_the_introduction_and_the_starts() {
        let start = MenuEntry { key: "a".into(), description: "Start".into(), action: Some("sandbox.new".into()) };
        let shown = welcome(vec![start], &[]);
        assert!(crate::surface::view::faults(shown.clone()).is_empty());
        assert_eq!(plain_text(shown.clone()).last().map(String::as_str), Some("a  Start"));
        let at = shown.text.find("a  Start").map(|b| shown.text[..b].chars().count()).unwrap();
        assert_eq!(crate::surface::view::actions_at(shown, at), vec!["sandbox.new".to_string()]);
    }
}
