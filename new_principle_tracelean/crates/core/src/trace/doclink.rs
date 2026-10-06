//! Documentation as a link.
//!
//! Documentation rots because nothing connects it to what it describes. The
//! connection already exists here: a requirement carries a hash of its content,
//! a model claims to model it, and when the text changes the hash moves and the
//! bond drops back to needing review.
//!
//! A document gets the same treatment. It names what it describes, records what
//! that hashed to, and when the hash moves the document is *in review*.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::index::Index;

/// What a document claims to describe, and what that hashed to when written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocLink {
    pub file: String,
    /// An anchor identity, or a requirement identifier.
    ///
    /// @implements REQ-DOCLINK.declares_target
    pub target: String,
    /// @implements REQ-DOCLINK.records_hash
    pub recorded_hash: String,
}

/// Where a document stands relative to what it describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// The recorded hash matches. The document was written about this.
    Current,
    /// The hash moved. The document may still be right; nobody has confirmed it
    /// since the change.
    ///
    /// Not an error. Blocking on it would make people stop writing
    /// documentation; hiding it would make the documentation worthless.
    ///
    /// @implements REQ-DOCLINK.hash_moves_review
    /// @implements REQ-DOCLINK.review_is_not_error
    InReview { was: String, now: String },
    /// The target no longer exists.
    ///
    /// @implements REQ-DOCLINK.dangling_reported
    Dangling,
}

impl State {
    pub fn blocks(&self) -> bool {
        matches!(self, State::Dangling)
    }
}

/// Whether a document's state stops the build.
///
/// Only a dangling target does. Blocking on *in review* would make people stop
/// writing documentation, which is the failure this requirement exists to
/// prevent; treating it as current would make the mechanism pointless.
///
/// @implements REQ-DOCLINK.review_is_not_error
/// Named `state_blocks` rather than `blocks` because the type already has a
/// `blocks` method, and a binding resolves a symbol by name: two functions
/// called `blocks` in one file would let the binding point at the wrong one.
///
/// @drt REQ-DOCLINK.review_is_not_error
pub fn state_blocks(state: State) -> bool {
    state.blocks()
}

/// What every target in the project currently hashes to.
///
/// Requirements hash by content; code anchors hash by normalised body. Both are
/// the same machinery the annotations already use — a document is resolved by
/// the same mechanism as a link, not by one written for it.
///
/// @implements REQ-DOCLINK.same_anchor_machinery
pub fn current_hashes(index: &Index) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (id, req) in &index.requirements {
        out.insert(id.clone(), req.content_hash.clone());
    }
    for link in &index.links {
        out.insert(link.anchor.ident(), link.anchor.body_hash.clone());
    }
    out
}

/// Where one document stands.
///
/// `current` is an association list rather than a map because that is the
/// shape the conformance protocol exchanges; the lookup is by first match, and
/// a target that appears twice is a caller's problem, not this function's.
///
/// @implements REQ-DOCLINK.hash_moves_review
/// @drt REQ-DOCLINK.hash_moves_review
/// @drt REQ-DOCLINK.dangling_reported
pub fn state(link: DocLink, current: Vec<(String, String)>) -> State {
    match current.iter().find(|(target, _)| *target == link.target).map(|(_, hash)| hash) {
        None => State::Dangling,
        Some(now) if *now == link.recorded_hash => State::Current,
        Some(now) => State::InReview { was: link.recorded_hash, now: now.clone() },
    }
}

/// The pairs `state` expects, from the map `current_hashes` produces.
pub fn pairs(current: &BTreeMap<String, String>) -> Vec<(String, String)> {
    current.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Parse `describes:` and `described_hash:` out of a document's frontmatter.
///
/// A document may describe several things, so `describes` is a list and
/// `described_hash` is a map keyed by target.
pub fn links_in(file: &str, content: &str) -> Vec<DocLink> {
    let Some(frontmatter) = frontmatter(content) else { return Vec::new() };

    let mut targets: Vec<String> = Vec::new();
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut in_hashes = false;

    for raw in frontmatter.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            if in_hashes {
                // Split from the right: an anchor target contains `::`, so
                // splitting from the left would cut the target in half.
                if let Some((target, hash)) = line.trim().rsplit_once(':') {
                    hashes.insert(target.trim().to_string(), hash.trim().to_string());
                }
            }
            continue;
        }
        in_hashes = false;
        let Some((key, value)) = line.split_once(':') else { continue };
        match key.trim() {
            "describes" => {
                let value = value.trim();
                if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    targets.extend(
                        inner
                            .split(',')
                            .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
                            .filter(|item| !item.is_empty()),
                    );
                } else if !value.is_empty() {
                    targets.push(value.trim_matches(['"', '\'']).to_string());
                }
            }
            "described_hash" => {
                if value.trim().is_empty() {
                    in_hashes = true;
                } else if targets.len() == 1 {
                    hashes.insert(targets[0].clone(), value.trim().to_string());
                }
            }
            _ => {}
        }
    }

    targets
        .into_iter()
        .map(|target| DocLink {
            file: file.to_string(),
            recorded_hash: hashes.get(&target).cloned().unwrap_or_default(),
            target,
        })
        .collect()
}

/// What a document declares, given as lines.
///
/// Returns target and recorded hash, and nothing else: the file a declaration
/// was found in is not part of what it says, and a model that agreed with an
/// implementation which had started consulting the path would be impossible to
/// write.
///
/// A document that declares nothing — a decision record, which describes a
/// moment rather than a subsystem and names the requirements it affects —
/// declares nothing here, rather than declaring an empty target.
///
/// @implements REQ-DOCLINK.declares_target
/// @implements REQ-DOCLINK.records_hash
/// @implements REQ-DOCLINK.decisions_exempt
/// @drt REQ-DOCLINK.declares_target
/// @drt REQ-DOCLINK.records_hash
/// @drt REQ-DOCLINK.decisions_exempt
pub fn declared_in(lines: Vec<String>) -> Vec<(String, String)> {
    links_in("", &lines.join("
"))
        .into_iter()
        .map(|link| (link.target, link.recorded_hash))
        .collect()
}

/// `recorded_frontmatter`, over owned arguments.
///
/// @implements REQ-DOCLINK.confirmation_is_human
/// @drt REQ-DOCLINK.confirmation_is_human
pub fn recorded_of(targets: Vec<String>, current: Vec<(String, String)>) -> String {
    let mut out = String::from("describes: [");
    out.push_str(&targets.join(", "));
    out.push_str("]\ndescribed_hash:\n");
    for target in &targets {
        // First match, the way `state` looks a target up. Collecting into a map
        // here would make a repeated target resolve to the *last* entry while
        // `state` resolved it to the first, so one function in this module
        // would say a document is current and the other would write a different
        // hash for it. Differential testing found exactly that.
        let hash = current
            .iter()
            .find(|(name, _)| name == target)
            .map(|(_, hash)| hash.clone())
            .unwrap_or_else(|| "?".into());
        out.push_str(&format!("  {target}: {hash}\n"));
    }
    out
}

fn frontmatter(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

/// The frontmatter a document should carry, given what its targets hash to now.
///
/// Confirming is a human act — read the document against the changed code, then
/// re-record. This produces the text to record; it never records it.
///
/// @implements REQ-DOCLINK.confirmation_is_human
pub fn recorded_frontmatter(targets: &[String], current: &BTreeMap<String, String>) -> String {
    let mut out = String::from("describes: [");
    out.push_str(&targets.join(", "));
    out.push_str("]\ndescribed_hash:\n");
    for target in targets {
        let hash = current.get(target).cloned().unwrap_or_else(|| "?".into());
        out.push_str(&format!("  {target}: {hash}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    fn link(target: &str, hash: &str) -> DocLink {
        DocLink { file: "docs/x.md".into(), target: target.into(), recorded_hash: hash.into() }
    }

    /// @tests REQ-DOCLINK.hash_moves_review
    #[test]
    fn a_moved_hash_puts_the_document_in_review() {
        let current = hashes(&[("REQ-EVID", "h2")]);
        assert_eq!(
            state(link("REQ-EVID", "h1"), current.clone()),
            State::InReview { was: "h1".into(), now: "h2".into() }
        );
        assert_eq!(state(link("REQ-EVID", "h2"), current), State::Current);
    }

    /// @tests REQ-DOCLINK.review_is_not_error
    #[test]
    fn in_review_does_not_block_and_dangling_does() {
        let review = State::InReview { was: "a".into(), now: "b".into() };
        assert!(!review.blocks());
        assert!(!State::Current.blocks());
        assert!(State::Dangling.blocks());
    }

    /// @tests REQ-DOCLINK.dangling_reported
    #[test]
    fn a_target_that_no_longer_exists_is_dangling() {
        assert_eq!(state(link("REQ-GONE", "h1"), hashes(&[])), State::Dangling);
    }

    /// @tests REQ-DOCLINK.declares_target
    /// @tests REQ-DOCLINK.records_hash
    #[test]
    fn frontmatter_declares_targets_and_their_hashes() {
        let doc = "---\ndescribes: [REQ-EVID, crates/core/src/evidence.rs::assurance]\ndescribed_hash:\n  REQ-EVID: aaa\n  crates/core/src/evidence.rs::assurance: bbb\n---\n\n# Prose\n";
        let links = links_in("docs/x.md", doc);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "REQ-EVID");
        assert_eq!(links[0].recorded_hash, "aaa");
        assert_eq!(links[1].recorded_hash, "bbb");
    }

    #[test]
    fn a_single_target_may_record_its_hash_inline() {
        let doc = "---\ndescribes: REQ-EVID\ndescribed_hash: aaa\n---\nbody";
        let links = links_in("docs/x.md", doc);
        assert_eq!(links, vec![link("REQ-EVID", "aaa")]);
    }

    #[test]
    fn a_document_without_frontmatter_declares_nothing() {
        assert!(links_in("docs/x.md", "# Just prose").is_empty());
    }

    /// A target declared with no recorded hash is in review from the start:
    /// nobody has confirmed the document against anything.
    #[test]
    fn an_unrecorded_hash_is_in_review_not_current() {
        let doc = "---\ndescribes: [REQ-EVID]\n---\nbody";
        let links = links_in("docs/x.md", doc);
        assert_eq!(
            state(links[0].clone(), hashes(&[("REQ-EVID", "h1")])),
            State::InReview { was: String::new(), now: "h1".into() }
        );
    }

    /// @tests REQ-DOCLINK.confirmation_is_human
    #[test]
    fn the_frontmatter_to_record_is_produced_not_applied() {
        let current: BTreeMap<String, String> =
            [("REQ-EVID".to_string(), "h9".to_string())].into_iter().collect();
        let text = recorded_frontmatter(&["REQ-EVID".to_string()], &current);
        assert!(text.contains("describes: [REQ-EVID]"));
        assert!(text.contains("  REQ-EVID: h9"));
    }
}
