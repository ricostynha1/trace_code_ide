//! The sandbox station: a session, the command that enters it, what the agent
//! changed, and what it said.
//!
//! The first TraceLean's sandbox was usable because it put everything on one
//! panel: a button to make a session, a command to copy, the changes as they
//! landed, and the agent's conversation live beside them. This is that panel as
//! a buffer. Every button is a span carrying the action a key would dispatch,
//! so a click, a key and a right-click reach the same thing.
//!
//! The conversation and cost rows are `produce::sandbox_events`, the modelled
//! producer, so what the agent said is still shown by the function the model
//! checks. Lines are wrapped to the width the pane has: a conversation clipped
//! at the pane's edge is a conversation nobody can read.
//!
//! @implements REQ-SHOW.sandbox_session_shown

use serde::{Deserialize, Serialize};

use crate::observe::cost::Spend;
use crate::observe::transcript::Event;
use crate::surface::produce::{sandbox_events, tidy};
use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// A change the agent made, waiting for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    /// `created`, `modified` or `deleted`.
    pub kind: String,
    pub path: String,
    /// Lines the change adds and removes, as its diff shows them.
    #[serde(default)]
    pub added: usize,
    #[serde(default)]
    pub removed: usize,
}

/// The session, as the panel shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    pub id: String,
    /// The containment, said in words: `bubblewrap`, or why there is none.
    pub containment: String,
    pub command: String,
}

/// Everything the panel is produced from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxView {
    pub session: Option<Shown>,
    pub pending: Vec<Pending>,
    pub said: Vec<Event>,
    pub spend: Spend,
    /// The characters a line may take before it wraps.
    pub width: usize,
}

/// Text built a line at a time, each piece with its role and actions. Shared
/// by the panels that are laid out rather than parsed.
pub(crate) struct Lines {
    text: String,
    spans: Vec<Span>,
    at: usize,
    width: usize,
}

impl Lines {
    pub(crate) fn new(width: usize) -> Lines {
        Lines { text: String::new(), spans: Vec::new(), at: 0, width: width.max(12) }
    }

    /// One line of pieces. Not wrapped: buttons and headings are short.
    pub(crate) fn line(&mut self, pieces: &[(&str, Role, &[&str])]) {
        if !self.text.is_empty() {
            self.text.push('\n');
            self.at += 1;
        }
        for (text, role, actions) in pieces {
            let length = text.chars().count();
            if *role != Role::Plain || !actions.is_empty() {
                self.spans.push(Span {
                    start: self.at,
                    stop: self.at + length,
                    role: *role,
                    actions: actions.iter().map(|a| a.to_string()).collect(),
                });
            }
            self.text.push_str(text);
            self.at += length;
        }
    }

    /// A long text, wrapped to the width, every row with the same role and
    /// actions — so any part of a wrapped command still copies the command.
    pub(crate) fn wrapped(&mut self, indent: &str, text: &str, role: Role, actions: &[&str]) {
        let room = self.width.saturating_sub(indent.chars().count()).max(8);
        let characters: Vec<char> = text.chars().collect();
        if characters.is_empty() {
            self.line(&[(indent, Role::Plain, &[])]);
            return;
        }
        for row in break_words(&characters, room) {
            self.line(&[(indent, Role::Plain, &[]), (&row, role, actions)]);
        }
    }

    pub(crate) fn blank(&mut self) {
        self.line(&[]);
    }

    /// The lines as a record titled `title`.
    pub(crate) fn finish(self, title: &str) -> Buffer {
        let size = self.text.chars().count();
        Buffer {
            id: format!("record:{title}"),
            kind: BufferKind::Record { title: title.to_string() },
            text: self.text,
            spans: tidy(size, self.spans),
        }
    }
}

/// Rows of at most `room` characters, broken after a space where the text has
/// one in the row's second half and through a word otherwise; a line break in
/// the text starts a row.
fn break_words(text: &[char], room: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for paragraph in text.split(|c| *c == '\n') {
        let mut rest = paragraph;
        loop {
            if rest.len() <= room {
                rows.push(rest.iter().collect());
                break;
            }
            let cut = match rest[..=room].iter().rposition(|c| *c == ' ') {
                // A break that would leave most of the row empty is worse
                // than a word broken: a path after a short word, say.
                Some(space) if space >= room / 2 => space + 1,
                _ => room,
            };
            rows.push(rest[..cut].iter().collect::<String>().trim_end().to_string());
            rest = &rest[cut..];
        }
    }
    rows
}

/// What role a row of the conversation is drawn in.
fn role_of(kind: &str) -> Role {
    match kind {
        "you" => Role::Requirement,
        "tool" => Role::Path,
        "failed" => Role::Removed,
        "cost" => Role::Heading,
        "thinking" | "result" => Role::Entry,
        _ => Role::Plain,
    }
}

/// The estimate under a heading that says what it estimates, in the price
/// table's own unit.
fn cost_lines(out: &mut Lines, estimate: &str) {
    out.line(&[("Agent cost (price table's unit)", role_of("cost"), &[])]);
    out.wrapped("  ", estimate, Role::Plain, &[]);
}

/// The sandbox panel.
///
/// @implements REQ-SHOW.sandbox_session_shown
/// @implements REQ-SHOW.producer_is_pure
pub fn sandbox_view(view: SandboxView) -> Buffer {
    let mut out = Lines::new(view.width);
    let Some(session) = &view.session else {
        out.line(&[("Sandbox", Role::Heading, &[])]);
        out.blank();
        out.wrapped(
            "",
            "A sandbox is a copy of this project. You run your agent in it, from your own terminal; what it changes appears here for you to accept or reject, and what it says appears below.",
            Role::Plain,
            &[],
        );
        out.blank();
        out.line(&[("[ New sandbox ]", Role::Entry, &["sandbox.new"])]);
        // The estimate is shown, and called one, even with nothing behind it
        // (`REQ-COST.estimate_is_labelled`).
        if let Some(cost) = sandbox_events(Vec::new(), view.said, view.spend).last() {
            out.blank();
            cost_lines(&mut out, &cost.text);
        }
        return out.finish("sandbox");
    };

    out.line(&[("Sandbox ", Role::Heading, &[]), (&session.id, Role::Heading, &[])]);
    out.wrapped("", &session.containment, Role::Entry, &[]);
    out.blank();
    out.wrapped("", "Run this in your own terminal, then start your agent (for example `claude`):", Role::Plain, &[]);
    out.wrapped("  ", &session.command, Role::Path, &["sandbox.copy"]);
    out.blank();
    out.line(&[
        ("[ Copy command ]", Role::Entry, &["sandbox.copy"]),
        (" ", Role::Plain, &[]),
        ("[ Look for changes ]", Role::Entry, &["observe.start"]),
    ]);
    out.line(&[("[ End sandbox ]", Role::Entry, &["sandbox.end"])]);
    out.blank();

    let count = view.pending.len().to_string();
    out.line(&[("Changes waiting (", Role::Heading, &[]), (&count, Role::Heading, &[]), (")", Role::Heading, &[])]);
    if view.pending.is_empty() {
        out.line(&[("  none yet", Role::Plain, &[])]);
    }
    for change in &view.pending {
        let role = match change.kind.as_str() {
            "created" => Role::Added,
            "deleted" => Role::Removed,
            _ => Role::Requirement,
        };
        // Accept and reject this file first, so they stay on screen however
        // long the path is.
        let label = format!(" {:<9}", change.kind);
        let (added, removed) = (format!(" +{}", change.added), format!(" −{}", change.removed));
        out.line(&[
            ("  ", Role::Plain, &[]),
            ("[✓]", Role::Added, &["observe.accept_file"]),
            (" ", Role::Plain, &[]),
            ("[✗]", Role::Removed, &["observe.reject_file"]),
            (&label, role, &[]),
            (&change.path, Role::Path, &["observe.diff"]),
            (&added, Role::Added, &[]),
            (&removed, Role::Removed, &[]),
        ]);
    }
    if !view.pending.is_empty() {
        out.line(&[
            ("[ Accept all ]", Role::Added, &["observe.accept"]),
            (" ", Role::Plain, &[]),
            ("[ Reject all ]", Role::Removed, &["observe.reject"]),
        ]);
    }
    out.blank();

    out.line(&[("Conversation, newest first", Role::Heading, &[])]);
    let rows = sandbox_events(Vec::new(), view.said, view.spend);
    if rows.len() <= 1 {
        out.wrapped("  ", "Nothing yet. It appears here as the agent works.", Role::Plain, &[]);
    }
    // The estimate is last in the modelled order and stays last here; the
    // conversation above it is reversed so the newest is where a person looks.
    let (cost, said) = rows.split_last().map(|(c, s)| (Some(c), s)).unwrap_or((None, &[]));
    for event in said.iter().rev() {
        out.wrapped("", &format!("{}: {}", event.kind, event.text), role_of(&event.kind), &[]);
    }
    if let Some(cost) = cost {
        out.blank();
        cost_lines(&mut out, &cost.text);
    }
    out.finish("sandbox")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults, plain_text};

    fn view(session: bool) -> SandboxView {
        SandboxView {
            session: session.then(|| Shown {
                id: "s1".into(),
                containment: "contained by bubblewrap".into(),
                command: "bwrap --ro-bind / / bash".into(),
            }),
            pending: vec![Pending { kind: "modified".into(), path: "src/a.rs".into(), added: 3, removed: 1 }],
            said: vec![
                Event { kind: "you".into(), text: "first".into() },
                Event { kind: "agent".into(), text: "second".into() },
            ],
            spend: Spend::default(),
            width: 40,
        }
    }

    fn offset_of(buffer: &Buffer, needle: &str) -> usize {
        let at = buffer.text.find(needle).expect(needle);
        buffer.text[..at].chars().count()
    }

    /// Wrapping breaks between words, and through one only when it is longer
    /// than a row.
    ///
    /// @tests REQ-SHOW.sandbox_session_shown
    #[test]
    fn text_wraps_between_words() {
        let chars = |s: &str| s.chars().collect::<Vec<char>>();
        assert_eq!(break_words(&chars("start your agent now"), 10), vec!["start your", "agent now"]);
        assert_eq!(break_words(&chars("abcdefghijkl"), 5), vec!["abcde", "fghij", "kl"]);
        assert_eq!(break_words(&chars("sh /a/long/path"), 8), vec!["sh /a/lo", "ng/path"]);
        assert_eq!(break_words(&chars("one\ntwo"), 10), vec!["one", "two"]);
    }

    /// @tests REQ-SHOW.sandbox_session_shown
    #[test]
    fn without_a_session_the_panel_offers_to_make_one() {
        let panel = sandbox_view(view(false));
        assert!(faults(panel.clone()).is_empty());
        let at = offset_of(&panel, "[ New sandbox ]");
        assert_eq!(actions_at(panel, at), vec!["sandbox.new".to_string()]);
    }

    /// @tests REQ-SHOW.sandbox_session_shown
    #[test]
    fn a_session_shows_its_command_its_changes_and_the_conversation_newest_first() {
        let panel = sandbox_view(view(true));
        assert!(faults(panel.clone()).is_empty());
        assert_eq!(actions_at(panel.clone(), offset_of(&panel, "bwrap")), vec!["sandbox.copy".to_string()]);
        assert_eq!(actions_at(panel.clone(), offset_of(&panel, "src/a.rs")), vec!["observe.diff".to_string()]);
        assert!(panel.text.contains("src/a.rs +3 −1"), "the change's size is not shown:\n{}", panel.text);
        assert_eq!(actions_at(panel.clone(), offset_of(&panel, "[ Accept all ]")), vec!["observe.accept".to_string()]);
        assert!(offset_of(&panel, "agent: second") < offset_of(&panel, "you: first"));
        assert!(plain_text(panel).iter().all(|line| line.chars().count() <= 40));
    }
}
