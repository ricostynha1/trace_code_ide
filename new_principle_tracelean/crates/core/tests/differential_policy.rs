//! The sandbox path policy, checked against the model.
//!
//! A wrong answer here is a security hole, and the function is pure, so it is
//! among the most worthwhile differential tests in the project.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

const OP: &str = "REQ-SBX.classification_total";

/// Paths chosen to reach every answer, including the ones trying not to be
/// answered. Random strings would classify as `mirrored` almost every time and
/// the run would report coverage it did not have.
fn input_schema() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "path".to_string(),
        Schema::Str {
            max_len: Some(12),
            examples: vec![
                "src/main.rs".into(),
                "a.rs".into(),
                "./src/a.rs".into(),
                ".git".into(),
                ".git/config".into(),
                ".tracelean/drt.json".into(),
                "src/../.git/x".into(),
                "target/debug/x".into(),
                "crates/core/target/x".into(),
                "formal/.lake/build/x".into(),
                "node_modules/a.js".into(),
                "../secrets".into(),
                "a/../../b".into(),
                "/etc/passwd".into(),
                "".into(),
                ".".into(),
                "..".into(),
                "a/../b.rs".into(),
                "C:/Windows".into(),
                "a//b".into(),
                "a/./b".into(),
                "venv/lib/x".into(),
            ],
        },
    );
    Schema::Struct { fields }
}

/// @drt REQ-SBX.classification_total
/// @tests REQ-SBX.classification_total
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_classification() {
    let scratch = harness::scratch("policy");
    let implementation = harness::rust_runner(
        "REQ-SBX",
        "classification_total",
        "crates/core/src/observe/policy.rs::classify",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Policy", "TraceLean.Policy.classify", OP, &["path"], &scratch);

    let result = run(
        OP,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 13, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    // The agreeing half of what this binding needs; the coverage half comes
    // from the generation test, and whichever lands second writes the record.
    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every answer must actually be reached, or agreement is vacuous.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_classification() {
    use tracelean_core::drt::gen;
    use tracelean_core::observe::{classify, Class};

    let schema = input_schema();
    let mut rng = gen::Rng::new(13);
    let mut seen: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let path = v["path"].as_str().unwrap_or_default().to_string();
        *seen.entry(format!("{:?}", classify(path))).or_default() += 1;
    }
    let names: Vec<String> =
        [Class::Protected, Class::Mirrored, Class::PassThrough, Class::Outside]
            .iter()
            .map(|class| format!("{class:?}"))
            .collect();
    let counts: Vec<(&str, u64)> = names
        .iter()
        .map(|name| (name.as_str(), seen.get(name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-SBX.classification_total", &counts);
}
