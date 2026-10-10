//! Contrast between a theme's colours, checked against the model: the WCAG
//! ratio, and what is found about each declared pair.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn name(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

/// @drt REQ-LOOK.contrast_sufficient
/// @tests REQ-LOOK.contrast_sufficient
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_contrast_finds() {
    let op = "REQ-LOOK.contrast_sufficient";
    let scratch = harness::scratch("contrast");
    let implementation =
        harness::rust_runner("REQ-LOOK", "contrast_sufficient", "crates/core/src/surface/contrast.rs::findings", &scratch);
    let model = harness::lean_runner("TraceLean.Contrast", "TraceLean.Contrast.findings", op, &["pairs", "colours"], &scratch);
    let keys = ["a", "b", "c", "missing"];
    // Colours at both ends, near the 4.5 and 3 thresholds, in capitals, with
    // alpha, short and not colours at all.
    let colour = name(&[
        "#000000", "#ffffff", "#777777", "#767676", "#949494", "#959595", "#282c34", "#abb2bf", "#5C6370", "#00000088",
        "#fff", "red", "#gggggg", "",
    ]);
    let mut pair = BTreeMap::new();
    pair.insert("text".to_string(), name(&keys));
    pair.insert("on".to_string(), name(&keys));
    pair.insert("least".to_string(), Schema::Nat { max: Some(10), edges: vec![0, 300, 450, 2100, 2101] });
    pair.insert("waived".to_string(), Schema::Option { inner: Box::new(name(&["kept"])) });
    let mut fields = BTreeMap::new();
    fields.insert("pairs".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: pair }), max_len: Some(3) });
    fields.insert(
        "colours".to_string(),
        Schema::List { inner: Box::new(Schema::Tuple { items: vec![name(&keys[..3]), colour] }), max_len: Some(4) },
    );
    let result = run(
        op,
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed: 409, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}
