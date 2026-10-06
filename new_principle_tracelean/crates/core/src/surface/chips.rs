//! What each declaration of a file is claimed for, as marks beside its line:
//! `M` models, `I` implements, `T` tests, `D` differential test, `P` proves.
//!
//! Read from the text being edited rather than from the index, so a mark
//! follows its declaration while lines are added above it and appears as soon
//! as the annotation is typed.

use crate::trace::annotation::Role;
use crate::trace::index::links_of;

/// One mark: the zero-based line it sits beside, its letter, and the
/// requirement (`REQ-X.clause`, or `REQ-X`) it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub line: usize,
    pub letter: char,
    pub requirement: String,
}

/// The letter a role is marked with, in the order marks are laid out. A pin
/// is a record, not a claim about the code, and has none.
pub fn letter(role: Role) -> Option<char> {
    match role {
        Role::Models => Some('M'),
        Role::Implements => Some('I'),
        Role::Tests => Some('T'),
        Role::Drt => Some('D'),
        Role::Proves => Some('P'),
        Role::Pins => None,
    }
}

/// Every mark in a file's text, by line and then in `M I T D P` order, one per
/// claim.
///
/// @implements REQ-SHOW.claims_beside_code
pub fn chips(file: &str, text: &str) -> Vec<Chip> {
    const ORDER: &str = "MITDP";
    let mut out: Vec<Chip> = links_of(file, text)
        .into_iter()
        .filter_map(|link| {
            Some(Chip {
                line: link.anchor.start_line as usize,
                letter: letter(link.role)?,
                requirement: match &link.clause {
                    Some(clause) => format!("{}.{clause}", link.req_id),
                    None => link.req_id.clone(),
                },
            })
        })
        .collect();
    out.sort_by_key(|c| (c.line, ORDER.find(c.letter), c.requirement.clone()));
    out.dedup();
    out
}

/// The chips of a requirement's own document: on each clause's line in its
/// frontmatter, a letter for each kind of claim on that clause — so what meets
/// a clause is seen while the clause is written. `claims` is every claim on
/// requirement `id`, as its clause and role.
///
/// @implements REQ-SHOW.claims_beside_code
pub fn clause_chips(text: &str, id: &str, claims: &[(Option<String>, Role)]) -> Vec<Chip> {
    const ORDER: &str = "MITDP";
    let mut out = Vec::new();
    for (line, key) in clause_lines(text) {
        let mut letters: Vec<char> = claims
            .iter()
            .filter(|(clause, _)| clause.as_deref() == Some(key.as_str()))
            .filter_map(|(_, role)| letter(*role))
            .collect();
        letters.sort_by_key(|l| ORDER.find(*l));
        letters.dedup();
        out.extend(letters.into_iter().map(|letter| Chip { line, letter, requirement: format!("{id}.{key}") }));
    }
    out
}

/// Each clause of a requirement's document, as its zero-based line and key: an
/// indented `key: text` in the frontmatter.
pub fn clause_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fences = 0;
    for (line, row) in text.lines().enumerate() {
        if row.trim() == "---" {
            fences += 1;
            if fences == 2 {
                break;
            }
            continue;
        }
        let Some((key, _)) = row.strip_prefix("  ").and_then(|r| r.split_once(':')) else { continue };
        let key = key.trim();
        if !key.is_empty() && !key.contains(' ') {
            out.push((line, key.to_string()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A requirement's document marks each clause with what claims it.
    ///
    /// @tests REQ-SHOW.claims_beside_code
    #[test]
    fn a_clause_is_marked_with_the_claims_on_it() {
        let text = "---\nid: REQ-A\nclauses:\n  one: It does.\n  two: It also does.\n---\n  three: not a clause\n";
        let claims = vec![
            (Some("one".to_string()), Role::Tests),
            (Some("one".to_string()), Role::Implements),
            (Some("one".to_string()), Role::Implements),
        ];
        let got = clause_chips(text, "REQ-A", &claims);
        assert_eq!(
            got,
            vec![
                Chip { line: 3, letter: 'I', requirement: "REQ-A.one".into() },
                Chip { line: 3, letter: 'T', requirement: "REQ-A.one".into() },
            ]
        );
    }

    /// @tests REQ-SHOW.claims_beside_code
    #[test]
    fn a_claim_is_marked_on_the_declaration_it_claims() {
        let text = "// head\n/// @tests REQ-B.y\n/// @implements REQ-A.x\nfn f() {}\n";
        let got = chips("src/a.rs", text);
        assert_eq!(
            got,
            vec![
                Chip { line: 3, letter: 'I', requirement: "REQ-A.x".into() },
                Chip { line: 3, letter: 'T', requirement: "REQ-B.y".into() },
            ]
        );
    }

    #[test]
    fn a_file_with_no_grammar_or_no_claims_has_no_marks() {
        assert!(chips("notes.txt", "@implements REQ-A.x").is_empty());
        assert!(chips("src/a.rs", "fn f() {}\n").is_empty());
    }
}
