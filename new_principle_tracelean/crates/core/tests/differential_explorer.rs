//! The project's files as a tree, checked against the model: which rows a set
//! of files makes with some folders open, in which order, at which depth.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn paths(examples: &[&str]) -> Schema {
    let path = Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() };
    Schema::List { inner: Box::new(path), max_len: Some(5) }
}

/// @drt REQ-SHOW.listing_is_a_tree
/// @tests REQ-SHOW.listing_is_a_tree
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_rows_of_the_tree() {
    let op = "REQ-SHOW.listing_is_a_tree";
    let scratch = harness::scratch("explorer");
    let implementation = harness::rust_runner(
        "REQ-SHOW",
        "listing_is_a_tree",
        "crates/core/src/surface/explorer.rs::rows_owned",
        &scratch,
    );
    let model = harness::lean_runner("TraceLean.Explorer", "TraceLean.Explorer.rows", op, &["files", "opened"], &scratch);
    // Nested folders, a file and a folder sharing a name's start, a folder
    // name sorting between files, a name past ASCII, and odd paths: a leading
    // slash, a doubled one, a trailing one.
    let mut fields = BTreeMap::new();
    fields.insert(
        "files".to_string(),
        paths(&["src/a.rs", "src/b/c.rs", "src/b/d.rs", "src.rs", "README.md", "é/x", "/top", "a//b", "dir/"]),
    );
    fields.insert("opened".to_string(), paths(&["src", "src/b", "é", "", "a", "a/", "dir"]));
    let result = run(
        op,
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed: 405, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}
