mod harness;

use harness::{name, strukt};

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

fn list(inner: Schema, max_len: usize) -> Schema {
    Schema::List { inner: Box::new(inner), max_len: Some(max_len) }
}

fn state() -> Schema {
    let marks: BTreeMap<String, Option<Box<Schema>>> =
        ["annotated", "judged", "tested", "proved"].iter().map(|m| (m.to_string(), None)).collect();
    strukt(&[
        ("reqId", name(&["REQ-A", "REQ-B"])),
        ("clause", Schema::Option { inner: Box::new(name(&["one", "two"])) }),
        ("text", name(&["shall a", "shall b"])),
        ("held", list(Schema::Enum { variants: marks }, 4)),
        (
            "links",
            list(
                strukt(&[
                    ("role", name(&["models", "tests"])),
                    ("symbol", name(&["f", "g"])),
                    ("bodyHash", name(&["h1", "h2"])),
                ]),
                3,
            ),
        ),
        ("assumed", list(name(&["Ax.copy", "Ax.read"]), 2)),
    ])
}

/// @drt REQ-DIFF.per_clause
/// @tests REQ-DIFF.per_clause
/// @tests REQ-DIFF.text_shown
/// @tests REQ-DIFF.marks_moved
/// @tests REQ-DIFF.links_moved
/// @tests REQ-DIFF.assumptions_moved
/// @tests REQ-DIFF.unchanged_omitted
/// @tests REQ-DIFF.losses_first
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_change_did() {
    let schema = strukt(&[("before", list(state(), 4)), ("after", list(state(), 4))]);
    harness::agree("REQ-DIFF.per_clause", &schema, 81, 3_000);
}
