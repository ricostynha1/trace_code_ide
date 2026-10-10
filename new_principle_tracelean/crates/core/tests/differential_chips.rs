//! The marks beside a requirement's clauses, checked against the model: which
//! lines are clauses, and which letters each carries.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn name(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

/// Documents with clauses, lines that look like clauses but are not (indented
/// deeper, two words, past the frontmatter), carriage returns and no final
/// newline.
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
    let scratch = harness::scratch("chips");
    let implementation = harness::rust_runner(
        "REQ-SHOW",
        "claims_beside_code",
        "crates/core/src/surface/chips.rs::clause_chips_owned",
        &scratch,
    );
    let model = harness::lean_runner("TraceLean.Chips", "TraceLean.Chips.clauseChips", op, &["text", "id", "claims"], &scratch);
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
    let result = run(
        op,
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed: 404, cases: 1_500, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}
