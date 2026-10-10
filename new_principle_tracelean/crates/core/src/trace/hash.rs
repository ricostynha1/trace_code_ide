//! Normalised hashing of an anchored body.
//!
//! The hash decides staleness, so both directions matter and they fail
//! differently: a hash that does not move when the body does leaves stale
//! evidence looking valid, and one that moves when the body did not makes
//! people stop annotating.

use std::collections::BTreeMap;

/// A short, stable digest. FNV-1a over the normalised bytes: not cryptographic,
/// because nothing here defends against an adversary choosing a collision — it
/// only has to change when the input does.
fn digest(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

/// Collapse runs of whitespace outside protected ranges, and drop the ranges
/// listed in `removed` entirely.
///
/// `protected` ranges — string and character literals — are copied through
/// byte for byte, because whitespace inside a literal is content.
///
/// @implements REQ-ANCHOR.whitespace_normalised
/// @implements REQ-ANCHOR.comments_excluded
pub fn normalize(source: &str, removed: &[(usize, usize)], protected: &[(usize, usize)]) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut last_was_space = true; // leading whitespace is dropped
    let mut i = 0usize;

    while i < bytes.len() {
        if let Some((_, end)) = removed.iter().find(|(s, e)| *s <= i && i < *e) {
            i = *end;
            // A removed range separates tokens, so it counts as whitespace.
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
            continue;
        }
        if let Some((start, end)) = protected.iter().find(|(s, e)| *s <= i && i < *e) {
            // Clamped rather than trusted. These ranges come from the scanner
            // and are always inside the source, but slicing past the end would
            // turn a scanner bug into a panic in the one function everything
            // else depends on.
            let from = (*start).min(source.len());
            let to = (*end).min(source.len()).max(from);
            out.push_str(source.get(from..to).unwrap_or(""));
            last_was_space = false;
            i = *end;
            continue;
        }
        let ch = source[i..].chars().next().unwrap_or(' ');
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
        i += ch.len_utf8();
    }

    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Hash of text exactly as it is: where a line number means something, as it
/// does for coverage, a blank line added is a change.
pub fn text(source: &str) -> String {
    digest(source.as_bytes())
}

/// Hash of a normalised body.
///
/// @implements REQ-ANCHOR.hash_tracks_body
pub fn body(source: &str, removed: &[(usize, usize)], protected: &[(usize, usize)]) -> String {
    digest(normalize(source, removed, protected).as_bytes())
}

/// Keyed text, `key ␁ text ␂` per entry in key order.
fn keyed(entries: &BTreeMap<String, String>) -> String {
    let mut buffer = String::new();
    for (key, text) in entries {
        buffer.push_str(key);
        buffer.push('\u{1}');
        buffer.push_str(text.trim());
        buffer.push('\u{2}');
    }
    buffer
}

/// Hash of keyed text and a normalised body: a whole requirement document
/// (what a document describing it is reviewed against), or any other set of
/// named parts, such as a link's identity.
pub fn requirement(clauses: &BTreeMap<String, String>, body_text: &str) -> String {
    let mut buffer = keyed(clauses);
    buffer.push_str(normalize(body_text, &[], &[]).as_str());
    digest(buffer.as_bytes())
}

/// Hash of one clause: what evidence about it rests on.
///
/// Its key, its text and its narrowings, and nothing else — so rewording a
/// sibling clause, or the prose, leaves the clause's evidence standing, and a
/// narrowing added re-opens it. Keyed by name, so a renamed clause is a
/// different clause. Without a key it is a requirement's one implicit clause,
/// whose text is the body, hashed as the body always was.
///
/// @implements REQ-REQDOC.clause_addressable
/// @implements REQ-STALE.requirement_reopens_all
pub fn clause(key: Option<&str>, text: &str, narrowings: &BTreeMap<String, String>) -> String {
    match key {
        None => digest(normalize(text, &[], &[]).as_bytes()),
        Some(key) => {
            let mut buffer = String::from(key);
            buffer.push('\u{1}');
            buffer.push_str(text.trim());
            buffer.push('\u{2}');
            buffer.push('\u{3}');
            buffer.push_str(&keyed(narrowings));
            digest(buffer.as_bytes())
        }
    }
}

/// `body`, in the shape the conformance protocol exchanges.
///
/// @implements REQ-ANCHOR.hash_tracks_body
/// @implements REQ-ANCHOR.whitespace_normalised
/// @implements REQ-ANCHOR.comments_excluded
/// @drt REQ-ANCHOR.hash_tracks_body
/// @drt REQ-ANCHOR.whitespace_normalised
/// @drt REQ-ANCHOR.comments_excluded
pub fn body_of(
    source: String,
    removed: Vec<(usize, usize)>,
    protected: Vec<(usize, usize)>,
) -> String {
    body(&source, &removed, &protected)
}

/// `normalize`, in the same shape. Bound separately from `body_of` because a
/// hash is a poor thing to diverge on: the digest tells you the two sides
/// disagree, the normalised text tells you where.
pub fn normalize_of(
    source: String,
    removed: Vec<(usize, usize)>,
    protected: Vec<(usize, usize)>,
) -> String {
    normalize(&source, &removed, &protected)
}

/// `clause`, in the same shape. A narrowing given twice: the later wins, as
/// the parser keeps it.
///
/// @implements REQ-REQDOC.clause_addressable
/// @drt REQ-REQDOC.clause_addressable
/// @drt REQ-STALE.requirement_reopens_all
pub fn clause_of(key: Option<String>, text: String, narrowings: Vec<(String, String)>) -> String {
    clause(key.as_deref(), &text, &narrowings.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-ANCHOR.hash_tracks_body
    #[test]
    fn reindentation_does_not_move_the_hash() {
        let a = "fn f() {\n    let x = 1;\n}";
        let b = "fn f() {\n\t\tlet x = 1;\n}";
        assert_eq!(body(a, &[], &[]), body(b, &[], &[]));
    }

    /// @tests REQ-ANCHOR.hash_tracks_body
    #[test]
    fn a_real_change_moves_the_hash() {
        let a = "fn f() { let x = 1; }";
        let b = "fn f() { let x = 2; }";
        assert_ne!(body(a, &[], &[]), body(b, &[], &[]));
    }

    /// The loop this closes: the annotation lives in a comment attached to what
    /// it anchors, so if comments counted, writing the annotation would
    /// invalidate the evidence it carries.
    ///
    /// @tests REQ-ANCHOR.comments_excluded
    #[test]
    fn editing_a_comment_does_not_invalidate_its_own_evidence() {
        let src = "// @implements REQ-X.c\nfn f() { 1 }";
        let edited = "// @implements REQ-X.c reason=\"clearer\"\nfn f() { 1 }";
        // The comment is everything up to the newline, in both.
        let comment = |s: &str| vec![(0usize, s.find('\n').unwrap())];
        assert_eq!(body(src, &comment(src), &[]), body(edited, &comment(edited), &[]));
    }

    /// @tests REQ-ANCHOR.whitespace_normalised
    #[test]
    fn whitespace_inside_a_literal_is_content() {
        let a = "let s = \"two  spaces\";";
        let b = "let s = \"two spaces\";";
        let lit_a = [(8usize, 21usize)];
        let lit_b = [(8usize, 20usize)];
        assert_ne!(body(a, &[], &lit_a), body(b, &[], &lit_b));
    }

    #[test]
    fn removed_ranges_still_separate_tokens() {
        // Deleting a comment between two tokens must not glue them together.
        let src = "a/*x*/b";
        assert_eq!(normalize(src, &[(1, 6)], &[]), "a b");
    }

    /// @tests REQ-REQDOC.clause_addressable
    #[test]
    fn renaming_or_narrowing_a_clause_changes_its_hash() {
        let none = BTreeMap::new();
        let text = "Levels are ordered.";
        assert_ne!(clause(Some("ladder"), text, &none), clause(Some("order"), text, &none));
        let mut narrowed = BTreeMap::new();
        narrowed.insert("empty".to_string(), "No levels: L1.".to_string());
        assert_ne!(clause(Some("ladder"), text, &none), clause(Some("ladder"), text, &narrowed));
        // The implicit clause hashes its body as the whole document did.
        assert_eq!(clause(None, "body  text", &none), requirement(&none, "body text"));
    }
}
