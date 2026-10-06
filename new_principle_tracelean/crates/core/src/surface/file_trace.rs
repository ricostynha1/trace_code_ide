//! The trace of the file being read: for each item in it that claims a
//! requirement, the clause, what it says, how far its evidence reaches, and
//! everything else that claims the same clause — the tests of a function, the
//! function a test tests, the model both answer to — each a link.
//!
//! The first TraceLean's trace panel stood beside the code for this reason: a
//! programmer reads a function and wants to know why it is there and what
//! else would notice if it changed, without leaving it.
//!
//! @implements REQ-SHOW.requirement_opened

use serde::{Deserialize, Serialize};

use crate::evidence::Level;
use crate::surface::requirement_view::{link_text, Claim};
use crate::surface::sandbox_view::Lines;
use crate::surface::view::{Buffer, Role, TokenKind};

/// One claim the file makes, and the rest of that clause's claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Where the annotation is, one-based.
    pub line: u32,
    /// The item it sits on, when it sits on one.
    pub symbol: Option<String>,
    /// `implements`, `tests`, …
    pub role: String,
    /// `REQ-X.clause`, or `REQ-X` for a requirement without clauses.
    pub clause: String,
    pub text: String,
    pub level: Level,
    /// Every other claim on the same clause, anywhere in the tree.
    pub others: Vec<Claim>,
}

fn claim_role(role: &str) -> Role {
    match crate::trace::annotation::Role::parse(role) {
        Some(role) => Role::Claim { role },
        None => Role::Plain,
    }
}

/// The trace of `path`, as a record titled `trace`: one constant identity,
/// so the side panel showing it follows the document from file to file.
pub fn file_trace_view(path: &str, entries: &[Entry], width: usize) -> Buffer {
    let mut out = Lines::new(width);
    out.line(&[("Trace of ", Role::Heading, &[]), (path, Role::Path, &["file.open"])]);
    if entries.is_empty() {
        out.wrapped("", "Nothing in this file claims a requirement. Annotate an item with `@implements REQ-X.clause` (or @tests, @models, @proves) to link it.", Role::Plain, &[]);
        return out.finish("trace");
    }
    let clauses: std::collections::BTreeSet<&str> = entries.iter().map(|e| e.clause.as_str()).collect();
    // A requirement's own document lists its clauses, each with its claims.
    let own = entries.iter().all(|e| e.role == "clause");
    let unmet = entries.iter().filter(|e| e.others.is_empty()).count();
    let summary = if own {
        format!("{} clauses, {} claimed by nothing", entries.len(), unmet)
    } else {
        format!(
            "{} claim{} on {} clause{}",
            entries.len(),
            if entries.len() == 1 { "" } else { "s" },
            clauses.len(),
            if clauses.len() == 1 { "" } else { "s" }
        )
    };
    out.line(&[(&summary, Role::Plain, &[])]);
    for entry in entries {
        out.blank();
        let place = format!("{path}:{}", entry.line);
        let mut head: Vec<(&str, Role, &[&str])> = Vec::new();
        let name = entry.symbol.clone().unwrap_or_else(|| "(this file)".into());
        head.push((&name, Role::Token { kind: TokenKind::Function }, &[]));
        head.push(("  ", Role::Plain, &[]));
        head.push((&place, Role::Path, &["file.open"]));
        out.line(&head);
        let grade = format!("{:?}", entry.level);
        out.line(&[
            ("  ", Role::Plain, &[]),
            (&entry.role, claim_role(&entry.role), &[]),
            (" ", Role::Plain, &[]),
            (&entry.clause, Role::Requirement, &["trace.requirement", "trace.context"]),
            ("  ", Role::Plain, &[]),
            (&grade, Role::Level { grade: entry.level }, &["trace.rollup"]),
        ]);
        out.wrapped("  ", &entry.text, Role::Plain, &[]);
        if entry.others.is_empty() {
            out.line(&[("  nothing else claims it", Role::Removed, &[])]);
        }
        for other in &entry.others {
            let link = link_text(&other.path, other.line);
            let symbol = other.symbol.as_deref().map(|s| format!("  {s}"));
            let mut pieces: Vec<(&str, Role, &[&str])> =
                vec![("    ", Role::Plain, &[]), (&other.role, claim_role(&other.role), &[]), (" ", Role::Plain, &[]), (&link, Role::Path, &["file.open"])];
            if let Some(symbol) = &symbol {
                pieces.push((symbol, Role::Token { kind: TokenKind::Function }, &[]));
            }
            out.line(&pieces);
        }
    }
    out.finish("trace")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::view::{actions_at, faults};

    fn offset_of(buffer: &Buffer, needle: &str) -> usize {
        buffer.text[..buffer.text.find(needle).expect(needle)].chars().count()
    }

    /// Each claim the file makes shows its clause, and every other claim on
    /// the clause is a link.
    ///
    /// @tests REQ-SHOW.requirement_opened
    #[test]
    fn a_file_shows_what_it_claims_and_what_else_claims_the_same() {
        let entries = vec![Entry {
            line: 3,
            symbol: Some("thing".into()),
            role: "implements".into(),
            clause: "REQ-A.one".into(),
            text: "It does the thing.".into(),
            level: Level::L2,
            others: vec![Claim { role: "tests".into(), path: "tests/t.rs".into(), line: 9, symbol: Some("it_does".into()) }],
        }];
        let shown = file_trace_view("src/a.rs", &entries, 60);
        assert!(faults(shown.clone()).is_empty());
        assert_eq!(shown.id, "record:trace");
        assert_eq!(actions_at(shown.clone(), offset_of(&shown, "tests/t.rs:9")), vec!["file.open".to_string()]);
        assert!(actions_at(shown.clone(), offset_of(&shown, "REQ-A.one")).contains(&"trace.context".to_string()));
        assert!(shown.text.contains("It does the thing."));
    }

    /// A requirement's document lists its clauses, saying which nothing claims.
    #[test]
    fn a_requirement_lists_its_clauses_and_what_claims_each() {
        let clause = |key: &str, others: Vec<Claim>| Entry {
            line: 7,
            symbol: Some(key.into()),
            role: "clause".into(),
            clause: format!("REQ-A.{key}"),
            text: "It does.".into(),
            level: Level::L1,
            others,
        };
        let entries = vec![
            clause("one", vec![Claim { role: "implements".into(), path: "src/a.rs".into(), line: 3, symbol: None }]),
            clause("two", vec![]),
        ];
        let shown = file_trace_view("reqs/REQ-A.md", &entries, 60);
        assert!(faults(shown.clone()).is_empty());
        assert!(shown.text.contains("2 clauses, 1 claimed by nothing"));
        assert_eq!(actions_at(shown.clone(), offset_of(&shown, "src/a.rs:3")), vec!["file.open".to_string()]);
    }

    #[test]
    fn a_file_with_no_claims_says_how_to_make_one() {
        let shown = file_trace_view("src/b.rs", &[], 60);
        assert!(shown.text.contains("@implements"));
    }
}
