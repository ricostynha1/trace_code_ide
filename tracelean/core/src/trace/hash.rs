//! Content hashing for anchors, links and requirements.
//!
//! Two distinct hashes exist and they answer different questions:
//!
//! * `body_hash` — the *behaviour* under an anchor, with comment text removed.
//!   Drives staleness: change the code and evidence about it must go grey, but
//!   reformatting it, or editing the annotation comment itself, must not.
//! * `link_hash` — the *identity* of a link (role, requirement, clause,
//!   attributes, anchor). Drives evidence-key validity, so retargeting an
//!   annotation from one requirement to another cannot silently inherit the
//!   old evidence.
//!
//! Every hash is prefixed with the normalization scheme version, so the scheme
//! can change later without silently invalidating (or silently accepting)
//! every record written under the old one.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Bumped whenever `normalize` changes.
pub const SCHEME: &str = "v1";

fn digest(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0u8]); // domain separator, so ("ab","c") != ("a","bc")
    }
    format!("{}:{:x}", SCHEME, h.finalize())
}

/// Collapse insignificant whitespace, leaving text inside string and character
/// literals untouched.
///
/// `protected` holds byte ranges (relative to `src`) that must be copied
/// verbatim — string literals, char literals — and `removed` holds ranges to
/// drop entirely, which is how comment text is excluded.
pub fn normalize(src: &str, protected: &[(usize, usize)], removed: &[(usize, usize)]) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut last_was_space = true; // suppresses leading whitespace
    let mut i = 0usize;

    while i < bytes.len() {
        if let Some(&(_, end)) = removed.iter().find(|&&(s, e)| s <= i && i < e) {
            // Dropped range (a comment). Treat it as a single separator so
            // `a/*x*/b` does not become `ab`.
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
            i = end;
            continue;
        }

        if let Some(&(start, end)) = protected.iter().find(|&&(s, e)| s <= i && i < e) {
            let slice = &src[start.max(i)..end.min(src.len())];
            out.push_str(slice);
            last_was_space = false;
            i = end;
            continue;
        }

        let ch = src[i..].chars().next().unwrap_or(' ');
        let len = ch.len_utf8();
        if ch.is_ascii_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
        i += len;
    }

    out.trim_end().to_string()
}

/// Hash of a normalized anchor body.
pub fn hash_body(normalized: &str) -> String {
    digest(&["body", normalized])
}

/// Hash of a link's identity.
pub fn hash_link(
    role: &str,
    req_id: &str,
    clause: Option<&str>,
    attrs: &BTreeMap<String, String>,
    anchor_ident: &str,
) -> String {
    let mut parts: Vec<String> = vec![
        "link".into(),
        role.into(),
        req_id.into(),
        clause.unwrap_or("").into(),
        anchor_ident.into(),
    ];
    for (k, v) in attrs {
        parts.push(format!("{k}={v}"));
    }
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    digest(&refs)
}

/// Hash of a requirement's semantic content: its clauses and its body.
///
/// Deliberately excludes `status`, `title` and file path — a workflow status
/// change is not a change of meaning and must not invalidate a judge verdict.
pub fn hash_requirement(clauses: &BTreeMap<String, String>, body: &str) -> String {
    let mut parts: Vec<String> = vec!["req".into()];
    for (k, v) in clauses {
        parts.push(format!("{k}:{}", v.split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    parts.push(body.split_whitespace().collect::<Vec<_>>().join(" "));
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    digest(&refs)
}

/// Hash of a single clause's text — what the judge's evidence is keyed on, so
/// editing one clause does not invalidate verdicts about its siblings.
pub fn hash_clause(text: &str) -> String {
    digest(&["clause", &text.split_whitespace().collect::<Vec<_>>().join(" ")])
}
