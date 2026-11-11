use crate::trace::annotation::Role;
use crate::trace::index::links_of;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Chip {
    pub line: usize,
    pub letter: char,
    pub requirement: String,
}

pub fn letter(role: Role) -> Option<char> {
    match role {
        Role::Models | Role::Specifies => Some('M'),
        Role::Implements => Some('I'),
        Role::Tests => Some('T'),
        Role::Drt => Some('D'),
        Role::Proves => Some('P'),
        Role::Pins => None,
    }
}

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

/// @drt REQ-SHOW.claims_beside_code
pub fn clause_chips_owned(text: String, id: String, claims: Vec<(Option<String>, Role)>) -> Vec<Chip> {
    clause_chips(&text, &id, &claims)
}

/// The line and key of each clause of a requirement document: `- **key**: text` under `## Clauses`.
pub fn clause_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_clauses = false;
    let mut fenced = false;
    for (line, row) in text.lines().enumerate() {
        if row.trim_start().starts_with("```") || row.trim_start().starts_with("~~~") {
            fenced = !fenced;
        }
        if fenced {
            continue;
        }
        if row.trim() == "## Clauses" {
            in_clauses = true;
            continue;
        }
        if in_clauses && row.starts_with("## ") {
            in_clauses = false;
        }
        if !in_clauses {
            continue;
        }
        if let Some((key, _)) = row.strip_prefix("- **").and_then(|r| r.split_once("**:")) {
            let key = key.trim();
            if !key.is_empty() && !key.contains(' ') {
                out.push((line, key.to_string()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-SHOW.claims_beside_code
    #[test]
    fn a_clause_is_marked_with_the_claims_on_it() {
        let text = "# REQ-A: REQ-A\n\n- status: draft\n\n  three: not a clause\n\n## Clauses\n\n- **one**: It does.\n- **two**: It also does.\n";
        let claims = vec![
            (Some("one".to_string()), Role::Tests),
            (Some("one".to_string()), Role::Implements),
            (Some("one".to_string()), Role::Implements),
        ];
        let got = clause_chips(text, "REQ-A", &claims);
        assert_eq!(
            got,
            vec![
                Chip { line: 8, letter: 'I', requirement: "REQ-A.one".into() },
                Chip { line: 8, letter: 'T', requirement: "REQ-A.one".into() },
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
