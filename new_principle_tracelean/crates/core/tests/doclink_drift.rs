//! The documentation rule, applied to this project's own documentation.
//!
//! @tests ARCH-SELFHOST.docs_checked
//! @structural ARCH-SELFHOST.docs_checked reason="a claim about every document in this repository at once"

use std::path::{Path, PathBuf};

use tracelean_core::trace::doclink::{current_hashes, pairs, state, State};
use tracelean_core::trace::index::build;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// Every document in this tree describes something that exists.
///
/// @tests REQ-DOCLINK.dangling_reported
#[test]
fn no_document_describes_something_that_does_not_exist() {
    let index = build(&project_root());
    let hashes = pairs(&current_hashes(&index));
    assert!(!index.doc_links.is_empty(), "no document declares what it describes");

    let dangling: Vec<&str> = index
        .doc_links
        .iter()
        .filter(|link| state((*link).clone(), hashes.clone()) == State::Dangling)
        .map(|link| link.target.as_str())
        .collect();
    assert!(dangling.is_empty(), "documents describe things that do not exist: {dangling:?}");
}

/// The mechanism must actually notice: a changed target puts its documents in
/// review, and this checks that on a copy rather than trusting it.
///
/// @tests REQ-DOCLINK.hash_moves_review
#[test]
fn changing_what_a_document_describes_puts_it_in_review() {
    let index = build(&project_root());
    let hashes = pairs(&current_hashes(&index));

    let link = index
        .doc_links
        .iter()
        .find(|link| state((*link).clone(), hashes.clone()) == State::Current)
        .expect("at least one confirmed document")
        .clone();

    // What the document would say if its target changed underneath it.
    let moved: Vec<(String, String)> = hashes
        .iter()
        .map(|(target, hash)| {
            let hash = if *target == link.target { "a-different-hash".to_string() } else { hash.clone() };
            (target.clone(), hash)
        })
        .collect();

    assert!(
        matches!(state(link.clone(), moved), State::InReview { .. }),
        "a moved hash did not put {} in review",
        link.file
    );
    assert_eq!(state(link, hashes), State::Current);
}
