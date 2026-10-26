mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

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
    let mut fields = BTreeMap::new();
    fields.insert(
        "files".to_string(),
        paths(&["src/a.rs", "src/b/c.rs", "src/b/d.rs", "src.rs", "README.md", "é/x", "/top", "a//b", "dir/"]),
    );
    fields.insert("opened".to_string(), paths(&["src", "src/b", "é", "", "a", "a/", "dir"]));
    harness::agree(op, &Schema::Struct { fields }, 405, 2_000);
}
