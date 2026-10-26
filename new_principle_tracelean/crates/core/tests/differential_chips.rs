mod harness;

use harness::name;

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

fn document() -> Schema {
    name(&[
        "---\nid: REQ-A\nclauses:\n  one: It does.\n  two: It also does.\n---\n  three: not a clause\n",
        "---\nclauses:\n  one:\n    text: deeper\n  two words: no\n\tone: tab\n---",
        "---\r\n  one: x\r\n  two: y\r\n---\r\n",
        "  one: before any fence\n---\n  two: inside\n",
        "",
        "---\n  : empty\n  one:two: colon\n",
    ])
}

/// @drt REQ-SHOW.claims_beside_code
/// @tests REQ-SHOW.claims_beside_code
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_marks_beside_clauses() {
    let op = "REQ-SHOW.claims_beside_code";
    let claim = Schema::Tuple {
        items: vec![
            Schema::Option { inner: Box::new(name(&["one", "two", "three"])) },
            Schema::simple_enum(&["models", "specifies", "implements", "tests", "drt", "proves", "pins"]),
        ],
    };
    let mut fields = BTreeMap::new();
    fields.insert("text".to_string(), document());
    fields.insert("id".to_string(), name(&["REQ-A"]));
    fields.insert("claims".to_string(), Schema::List { inner: Box::new(claim), max_len: Some(5) });
    harness::agree(op, &Schema::Struct { fields }, 404, 1_500);
}
