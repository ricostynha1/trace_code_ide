//! The annotation grammar.
//!
//! Annotations are the only mechanism linking anything to anything here, which
//! makes totality the load-bearing property: every candidate either parses or
//! is named as a problem. A scanner that can silently drop a link reports less
//! coverage than the project has, and trains people to distrust it.
//!
//! This module is pure. Finding the comments is the shell's job; deciding what
//! a comment says is this.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a link claims.
///
/// `Models` is exactly the function that computes what the clause talks about,
/// and `Specifies` the `Prop` that says which answers are right (ADR-0014).
///
/// @implements REQ-ANNOT.role_vocabulary
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Models,
    Specifies,
    Implements,
    Tests,
    Drt,
    Proves,
    Pins,
}

impl Role {
    pub fn parse(word: &str) -> Option<Role> {
        Some(match word {
            "models" => Role::Models,
            "specifies" => Role::Specifies,
            "implements" => Role::Implements,
            "tests" => Role::Tests,
            "drt" => Role::Drt,
            "proves" => Role::Proves,
            "pins" => Role::Pins,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Models => "models",
            Role::Specifies => "specifies",
            Role::Implements => "implements",
            Role::Tests => "tests",
            Role::Drt => "drt",
            Role::Proves => "proves",
            Role::Pins => "pins",
        }
    }
}

/// Modifies the claim made by the nearest annotation.
///
/// @implements REQ-ANNOT.qualifiers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Qualifier {
    /// Caps a clause's contribution below full, and suppresses nothing.
    Partial { reason: Option<String> },
    /// Removes a clause from the coverage denominator entirely — which is why
    /// it must carry a reason and an approver.
    /// `judgedBy` and `expires` rather than `by` and `until`: the model's
    /// language reserves both words, and a name escaped there cannot be read by
    /// the grammar this project parses Lean with (ADR-0008). The attributes in
    /// the annotation text are still spelled `by=` and `until=`.
    Exempt { reason: Option<String>, judged_by: Option<String>, expires: Option<String> },
    /// This model cannot be pinned, and here is why.
    Nondeterministic { reason: Option<String> },
    /// This clause is a property of the tree rather than of a value, so there
    /// is no data-to-data function to model and no pair of functions to compare.
    ///
    /// It still has to be implemented and still has to be tested — by a test
    /// that reads the repository and says what it found. What it cannot reach
    /// is L3: a structural check is a check of this tree, not a check of a law
    /// over all inputs.
    ///
    /// Unlike an exemption this does not leave the denominator. A structural
    /// clause is answered, just answered differently, and a project that
    /// quietly dropped its architectural constraints out of its own coverage
    /// figures would be flattering itself.
    Structural { reason: Option<String> },
}

impl Qualifier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Qualifier::Partial { .. } => "partial",
            Qualifier::Exempt { .. } => "exempt",
            Qualifier::Nondeterministic { .. } => "nondeterministic",
            Qualifier::Structural { .. } => "structural",
        }
    }

    fn parse(word: &str, attrs: &BTreeMap<String, String>) -> Option<Qualifier> {
        let get = |k: &str| attrs.get(k).cloned();
        Some(match word {
            "partial" => Qualifier::Partial { reason: get("reason") },
            "exempt" => Qualifier::Exempt {
                reason: get("reason"),
                judged_by: get("by"),
                expires: get("until"),
            },
            "nondeterministic" => Qualifier::Nondeterministic { reason: get("reason") },
            "structural" => Qualifier::Structural { reason: get("reason") },
            _ => return None,
        })
    }
}

/// One directive found in a comment, before anything is resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Directive {
    Annotation { annotation: RawAnnotation },
    Qualified { qualifier: Qualifier, req_id: Option<String>, clause: Option<String>, line: u32 },
    /// Closes the nearest open region.
    End { line: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAnnotation {
    pub role: Role,
    pub req_id: String,
    pub clause: Option<String>,
    /// Sorted pairs on the wire: the model's association list, and the order
    /// attributes come back in is part of what the two sides agree on.
    #[serde(with = "crate::wire::pairs")]
    pub attrs: BTreeMap<String, String>,
    /// `@role ID begin` opens a region ended by `@end`.
    pub opens_region: bool,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProblemKind {
    UnknownRole,
    /// A role with no requirement identifier after it.
    MissingId,
    /// A qualifier with neither a preceding annotation nor its own identifier.
    OrphanQualifier,
    UnclosedRegion,
    StrayEnd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    pub kind: ProblemKind,
    pub line: u32,
    pub message: String,
}

/// Everything one comment said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ParsedComment {
    pub directives: Vec<Directive>,
    pub problems: Vec<Problem>,
}

/// Parse the directives in one comment's text.
///
/// `first_line` is the 0-indexed line the comment starts on, so that reported
/// lines are file lines.
///
/// Every `@word` yields a directive or a problem. Nothing is dropped.
///
/// @implements REQ-ANNOT.totality
/// @implements REQ-ANNOT.unknown_role_named
pub fn parse_comment(text: &str, first_line: u32) -> ParsedComment {
    let mut out = ParsedComment::default();

    for (offset, raw) in text.lines().enumerate() {
        let line = first_line + offset as u32;
        // Every at-sign on the line is considered, and only those starting a
        // word: `a@example.com` in prose is an address, not a directive, while
        // a directive is always preceded by a space or the line start.
        for (at, _) in raw.match_indices('@') {
            if at > 0 && !raw.as_bytes()[at - 1].is_ascii_whitespace() {
                continue;
            }
            let rest = &raw[at + 1..];

            let word_len = rest
                .char_indices()
                .take_while(|(_, c)| c.is_ascii_lowercase() || *c == '_')
                .count();
            if word_len == 0 {
                // A bare `@` in prose is not a directive and not a problem.
                continue;
            }
            let word = &rest[..word_len];
            let tail = &rest[word_len..];

            if word == "end" {
                out.directives.push(Directive::End { line });
                continue;
            }

            let (req_id, clause, after_id) = split_identifier(tail);
            let attrs = parse_attrs(after_id);
            let opens_region = after_id.split_whitespace().any(|w| w == "begin");

            if let Some(role) = Role::parse(word) {
                match req_id {
                    Some(id) => out.directives.push(Directive::Annotation { annotation: RawAnnotation {
                        role,
                        req_id: id,
                        clause,
                        attrs,
                        opens_region,
                        line,
                    } }),
                    None => out.problems.push(Problem {
                        kind: ProblemKind::MissingId,
                        line,
                        message: format!("`@{word}` names no requirement"),
                    }),
                }
                continue;
            }

            if let Some(qualifier) = Qualifier::parse(word, &attrs) {
                out.directives.push(Directive::Qualified { qualifier, req_id, clause, line });
                continue;
            }

            out.problems.push(Problem {
                kind: ProblemKind::UnknownRole,
                line,
                message: format!("`@{word}` is not a role or a qualifier"),
            });
        }
    }

    out
}

/// `parse_comment`, over the lines a comment is made of.
///
/// The conformance protocol exchanges structured values, and a generator that
/// could only produce a flat string would never produce a newline — so the
/// multi-line comments, where line numbering is what can go wrong, would never
/// be generated. The function under test is still `parse_comment`.
///
/// @implements REQ-ANNOT.totality
/// @drt REQ-ANNOT.totality
/// @drt REQ-ANNOT.role_vocabulary
/// @drt REQ-ANNOT.qualifiers
/// @drt REQ-ANNOT.unknown_role_named
pub fn parse_comment_lines(lines: Vec<String>, first_line: u32) -> ParsedComment {
    parse_comment(&lines.join("\n"), first_line)
}

/// Split a leading ` ID` or ` ID.clause` off, returning what follows.
///
/// An identifier starts with an upper-case letter; a clause is word characters
/// only. Both are deliberately narrow, because the alternative to refusing an
/// odd identifier is resolving it to something the author did not mean.
fn split_identifier(tail: &str) -> (Option<String>, Option<String>, &str) {
    let trimmed = tail.trim_start();
    let skipped = tail.len() - trimmed.len();
    if skipped == 0 && !tail.is_empty() {
        // `@modelsREQ-X` — the word did not end where an identifier begins.
        return (None, None, tail);
    }

    let first = trimmed.chars().next();
    if !matches!(first, Some(c) if c.is_ascii_uppercase()) {
        return (None, None, tail);
    }

    let id_len = trimmed
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .count();
    let id = trimmed[..id_len].to_string();
    let after = &trimmed[id_len..];

    if let Some(dotted) = after.strip_prefix('.') {
        let clause_len = dotted
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_')
            .count();
        if clause_len > 0 {
            return (Some(id), Some(dotted[..clause_len].to_string()), &dotted[clause_len..]);
        }
    }
    (Some(id), None, after)
}

/// `key="quoted value"` or `key=bare`.
fn parse_attrs(text: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i] != b'=' {
            i += 1;
            continue;
        }
        // Walk back over the key.
        let mut key_start = i;
        while key_start > 0 {
            let c = bytes[key_start - 1];
            if c.is_ascii_lowercase() || c == b'_' {
                key_start -= 1;
            } else {
                break;
            }
        }
        if key_start == i {
            i += 1;
            continue;
        }
        let key = text[key_start..i].to_string();

        let value_start = i + 1;
        if value_start >= bytes.len() {
            break;
        }
        if bytes[value_start] == b'"' {
            match text[value_start + 1..].find('"') {
                Some(end) => {
                    attrs.insert(key, text[value_start + 1..value_start + 1 + end].to_string());
                    i = value_start + end + 2;
                }
                // An unterminated quote takes the rest of the line rather than
                // discarding what the author wrote.
                None => {
                    attrs.insert(key, text[value_start + 1..].to_string());
                    i = bytes.len();
                }
            }
        } else {
            let end = text[value_start..]
                .find(char::is_whitespace)
                .map(|e| value_start + e)
                .unwrap_or(bytes.len());
            attrs.insert(key, text[value_start..end].to_string());
            i = end;
        }
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> ParsedComment {
        parse_comment(text, 0)
    }

    /// @tests REQ-ANNOT.role_vocabulary
    #[test]
    fn a_role_with_a_clause() {
        let parsed = one("@implements REQ-EVID.weakest_link");
        assert!(parsed.problems.is_empty());
        match &parsed.directives[0] {
            Directive::Annotation { annotation: a } => {
                assert_eq!(a.role, Role::Implements);
                assert_eq!(a.req_id, "REQ-EVID");
                assert_eq!(a.clause.as_deref(), Some("weakest_link"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// A specification is its own role, not a second model.
    ///
    /// @tests REQ-ANNOT.role_vocabulary
    #[test]
    fn a_specification_is_its_own_role() {
        let parsed = one("@specifies REQ-THERMO.to_fahrenheit");
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
        match &parsed.directives[0] {
            Directive::Annotation { annotation: a } => assert_eq!(a.role, Role::Specifies),
            other => panic!("{other:?}"),
        }
        assert_eq!(Role::Specifies.as_str(), "specifies");
    }

    /// A hyphen is legal in an identifier and not in a clause, so the clause
    /// stops at it. Stated as a test because the silent version of this is the
    /// worst kind of bug: the annotation resolves, to the wrong clause.
    #[test]
    fn a_clause_stops_at_a_hyphen() {
        let parsed = one("@models REQ-X.a-b");
        match &parsed.directives[0] {
            Directive::Annotation { annotation: a } => assert_eq!(a.clause.as_deref(), Some("a")),
            other => panic!("{other:?}"),
        }
    }

    /// @tests REQ-ANNOT.unknown_role_named
    #[test]
    fn an_unknown_role_is_named_not_dropped() {
        let parsed = one("@implments REQ-X.c");
        assert!(parsed.directives.is_empty());
        assert_eq!(parsed.problems[0].kind, ProblemKind::UnknownRole);
    }

    /// @tests REQ-ANNOT.totality
    #[test]
    fn a_role_without_an_identifier_is_reported() {
        let parsed = one("@models");
        assert_eq!(parsed.problems[0].kind, ProblemKind::MissingId);
    }

    /// Prose containing an at-sign is neither a directive nor a problem.
    ///
    /// @tests REQ-ANNOT.totality
    #[test]
    fn prose_is_left_alone() {
        let parsed = one("write to a@example.com, or see @ the docs");
        assert!(parsed.directives.is_empty());
        assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    }

    /// @tests REQ-ANNOT.qualifiers
    #[test]
    fn qualifiers_carry_their_attributes() {
        let parsed = one("@exempt reason=\"platform specific\" by=ana until=2027-01-01");
        match &parsed.directives[0] {
            Directive::Qualified {
                qualifier: Qualifier::Exempt { reason, judged_by, expires },
                ..
            } => {
                assert_eq!(reason.as_deref(), Some("platform specific"));
                assert_eq!(judged_by.as_deref(), Some("ana"));
                assert_eq!(expires.as_deref(), Some("2027-01-01"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_region_opener_is_marked() {
        let parsed = one("@implements REQ-X begin\nsomething\n@end");
        match &parsed.directives[0] {
            Directive::Annotation { annotation: a } => assert!(a.opens_region),
            other => panic!("{other:?}"),
        }
        assert!(matches!(parsed.directives[1], Directive::End { line: 2 }));
    }

    #[test]
    fn lines_are_reported_relative_to_the_file() {
        let parsed = parse_comment("first\n@models REQ-X", 10);
        match &parsed.directives[0] {
            Directive::Annotation { annotation: a } => assert_eq!(a.line, 11),
            other => panic!("{other:?}"),
        }
    }

    /// An unterminated quote keeps what the author wrote rather than dropping
    /// the attribute.
    #[test]
    fn an_unterminated_quote_is_not_discarded() {
        let parsed = one("@partial reason=\"half of it");
        match &parsed.directives[0] {
            Directive::Qualified { qualifier: Qualifier::Partial { reason }, .. } => {
                assert_eq!(reason.as_deref(), Some("half of it"));
            }
            other => panic!("{other:?}"),
        }
    }

    /// Every directive-shaped token produces exactly one outcome.
    ///
    /// @tests REQ-ANNOT.totality
    #[test]
    fn nothing_is_silently_dropped() {
        let text = "@models REQ-A\n@nonsense REQ-B\n@tests\n@end\n@partial reason=x";
        let parsed = one(text);
        assert_eq!(parsed.directives.len() + parsed.problems.len(), 5);
    }
}
