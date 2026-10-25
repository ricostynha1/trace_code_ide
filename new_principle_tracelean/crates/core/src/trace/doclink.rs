use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::index::Index;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocLink {
    pub file: String,
    /// @implements REQ-DOCLINK.declares_target
    pub target: String,
    /// @implements REQ-DOCLINK.records_hash
    pub recorded_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    Current,
    /// @implements REQ-DOCLINK.hash_moves_review
    /// @implements REQ-DOCLINK.review_is_not_error
    InReview { was: String, now: String },
    /// @implements REQ-DOCLINK.dangling_reported
    Dangling,
}

impl State {
    pub fn blocks(&self) -> bool {
        matches!(self, State::Dangling)
    }
}

/// @implements REQ-DOCLINK.review_is_not_error
/// @drt REQ-DOCLINK.review_is_not_error
pub fn state_blocks(state: State) -> bool {
    state.blocks()
}

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

pub fn pairs(current: &BTreeMap<String, String>) -> Vec<(String, String)> {
    current.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

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

/// @implements REQ-DOCLINK.confirmation_is_human
/// @drt REQ-DOCLINK.confirmation_is_human
pub fn recorded_of(targets: Vec<String>, current: Vec<(String, String)>) -> String {
    let mut out = String::from("describes: [");
    out.push_str(&targets.join(", "));
    out.push_str("]\ndescribed_hash:\n");
    for target in &targets {
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

    /// @tests REQ-DOCLINK.declares_target
    /// @tests REQ-DOCLINK.records_hash
    #[test]
    fn a_documents_lines_declare_its_targets_and_their_hashes() {
        let lines = ["---", "describes: [REQ-A, REQ-B]", "described_hash:", "  REQ-A: h1", "  REQ-B: h2", "---", "# Doc"];
        let pairs = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            declared_in(lines.iter().map(|l| l.to_string()).collect()),
            vec![pairs("REQ-A", "h1"), pairs("REQ-B", "h2")]
        );
        assert!(declared_in(vec!["# no frontmatter".into()]).is_empty());
    }
}
