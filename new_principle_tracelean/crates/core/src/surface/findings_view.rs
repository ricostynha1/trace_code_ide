//! The checker's findings, each a way to the place it is about.
//!
//! A finding names a file and a line, and usually a requirement. Shown as one
//! undifferentiated row, neither could be followed; here the location is a
//! `path:line` link (`file.open`, which the shell opens at that line) and every
//! requirement named in the message opens that requirement. The text is the
//! same `kind: path:line message` a report has always read.
//!
//! @implements REQ-SHOW.findings_lead_somewhere

use crate::surface::highlight::names;
use crate::surface::produce::tidy;
use crate::surface::view::{Buffer, BufferKind, Role, Span};

/// One finding, as the view needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// What kind of finding, in lower case: `unmodeled`, `imprecise`, …
    pub kind: String,
    pub file: String,
    /// One-based.
    pub line: u32,
    pub message: String,
}

/// The findings as a record titled `title`, one row a finding.
pub fn findings_view(title: &str, found: &[Found]) -> Buffer {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut at = 0usize;
    let mut row = |text: &mut String, pieces: &[(String, Role, &[&str])], names_in_last: bool| {
        if !text.is_empty() {
            text.push('\n');
            at += 1;
        }
        for (n, (piece, role, actions)) in pieces.iter().enumerate() {
            let length = piece.chars().count();
            if names_in_last && n == pieces.len() - 1 {
                let chars: Vec<char> = piece.chars().collect();
                for (from, to) in names(&chars, 0, chars.len()) {
                    spans.push(Span {
                        start: at + from,
                        stop: at + to,
                        role: Role::Requirement,
                        actions: vec!["trace.requirement".to_string()],
                    });
                }
            } else if *role != Role::Plain {
                spans.push(Span {
                    start: at,
                    stop: at + length,
                    role: *role,
                    actions: actions.iter().map(|a| a.to_string()).collect(),
                });
            }
            text.push_str(piece);
            at += length;
        }
    };
    if found.is_empty() {
        row(&mut text, &[("clean: nothing to report".to_string(), Role::Entry, &[])], false);
    }
    for finding in found {
        let location = format!("{}:{}", finding.file, finding.line);
        row(
            &mut text,
            &[
                (format!("{}: ", finding.kind), Role::Entry, &[]),
                (location, Role::Path, &["file.open"]),
                (format!(" {}", finding.message), Role::Plain, &[]),
            ],
            true,
        );
    }
    let size = text.chars().count();
    Buffer {
        id: format!("record:{title}"),
        kind: BufferKind::Record { title: title.to_string() },
        text,
        spans: tidy(size, spans),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults};

    /// A finding's place opens the file there, and the requirement it names
    /// opens the requirement.
    ///
    /// @tests REQ-SHOW.findings_lead_somewhere
    #[test]
    fn a_finding_links_to_its_place_and_its_requirement() {
        let found = vec![Found {
            kind: "unmodeled".into(),
            file: "reqs/REQ-X.md".into(),
            line: 3,
            message: "REQ-X.holds has no model".into(),
        }];
        let shown = findings_view("findings", &found);
        assert_eq!(shown.text, "unmodeled: reqs/REQ-X.md:3 REQ-X.holds has no model");
        assert!(faults(shown.clone()).is_empty());
        let at = |needle: &str| shown.text[..shown.text.find(needle).unwrap()].chars().count();
        assert_eq!(actions_at(shown.clone(), at("reqs/")), vec!["file.open".to_string()]);
        assert_eq!(actions_at(shown.clone(), at("REQ-X.holds")), vec!["trace.requirement".to_string()]);
        assert_eq!(findings_view("check", &[]).text, "clean: nothing to report");
    }
}
