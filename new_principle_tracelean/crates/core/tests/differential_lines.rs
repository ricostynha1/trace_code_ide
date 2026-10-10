//! Line coverage, checked against the model.
//!
//! The failure this guards is a line said to be run by a test that never ran
//! it, a count summed wrongly across tests, or a clause shown fully covered
//! while one of its lines ran in no test.

mod harness;
mod support;

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::trace::lines::{merged, span_coverage, LineHits, TestLines};

fn small(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

fn fields(pairs: Vec<(&str, Schema)>) -> BTreeMap<String, Schema> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn line() -> Schema {
    Schema::Nat { max: Some(5), edges: vec![1] }
}

fn count() -> Schema {
    Schema::Nat { max: Some(3), edges: vec![0, 0] }
}

fn runs() -> Schema {
    let one = Schema::Struct {
        fields: fields(vec![
            ("test", small(&["t1", "t2"])),
            ("file", small(&["a.rs", "a.rs", "b.rs"])),
            ("lines", Schema::List { inner: Box::new(Schema::Tuple { items: vec![line(), count()] }), max_len: Some(4) }),
        ]),
    };
    Schema::List { inner: Box::new(one), max_len: Some(4) }
}

fn per_test() -> BTreeMap<String, Schema> {
    fields(vec![("runs", runs()), ("file", small(&["a.rs"]))])
}

fn summary() -> BTreeMap<String, Schema> {
    let hits = Schema::Struct {
        fields: fields(vec![
            ("line", line()),
            ("hits", count()),
            ("tests", Schema::List { inner: Box::new(Schema::Tuple { items: vec![small(&["t1", "t2"]), count()] }), max_len: Some(2) }),
        ]),
    };
    fields(vec![
        ("lines", Schema::List { inner: Box::new(hits), max_len: Some(5) }),
        ("start", line()),
        ("stop", line()),
    ])
}

fn check(clause: &str, function: &str, entry: &str, arguments: &[&str], params: &[(&str, &str)], fields: BTreeMap<String, Schema>, seed: u64) {
    let op = format!("REQ-LINECOV.{clause}");
    let scratch = harness::scratch(&format!("lines-{clause}"));
    let implementation = harness::rust_runner_with_params("REQ-LINECOV", clause, entry, params, &scratch);
    let model = harness::lean_runner("TraceLean.Lines", function, &op, arguments, &scratch);
    let result = run(&op, &Schema::Struct { fields }, &model, &implementation, RunOptions { seed, cases: 3_000, shrink_rounds: 100 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-LINECOV.per_test
/// @tests REQ-LINECOV.per_test
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_who_ran_each_line() {
    check("per_test", "TraceLean.Lines.merged", "crates/core/src/trace/lines.rs::merged", &["runs", "file"], &[], per_test(), 331);
}

/// @drt REQ-LINECOV.clause_summary
/// @tests REQ-LINECOV.clause_summary
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_much_of_a_span_ran() {
    check(
        "clause_summary",
        "TraceLean.Lines.spanCoverage",
        "crates/core/src/trace/lines.rs::span_coverage",
        &["lines", "start", "stop"],
        &[("stop", "end")],
        summary(),
        332,
    );
}

fn measured() -> BTreeMap<String, Schema> {
    let hits = Schema::Struct {
        fields: fields(vec![
            ("line", line()),
            ("hits", count()),
            ("tests", Schema::List { inner: Box::new(Schema::Tuple { items: vec![small(&["t1", "t2"]), count()] }), max_len: Some(2) }),
        ]),
    };
    let at = Schema::Tuple {
        items: vec![small(&["h1", "h2"]), Schema::List { inner: Box::new(hits), max_len: Some(3) }],
    };
    fields(vec![("measured", Schema::Option { inner: Box::new(at) }), ("hash", small(&["h1", "h2", "h3"]))])
}

/// @drt REQ-LINECOV.stale_hidden
/// @tests REQ-LINECOV.stale_hidden
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_when_measured_lines_are_shown() {
    check(
        "stale_hidden",
        "TraceLean.Lines.visible",
        "crates/core/src/trace/lines.rs::visible",
        &["measured", "hash"],
        &[],
        measured(),
        333,
    );
}

/// @drt REQ-LINECOV.uncovered_shown
/// @tests REQ-LINECOV.uncovered_shown
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_marks_beside_lines() {
    let mut window = summary();
    window.remove("start");
    window.remove("stop");
    let lines = window.remove("lines").expect("the summary has lines");
    // Line 0 included: no line, so no mark.
    let Schema::List { inner, max_len } = lines else { unreachable!() };
    let Schema::Struct { fields: mut hit } = *inner else { unreachable!() };
    hit.insert("line".to_string(), Schema::Nat { max: Some(6), edges: vec![0, 1] });
    window.insert("lines".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: hit }), max_len });
    window.insert("top".to_string(), Schema::Nat { max: Some(4), edges: vec![0] });
    window.insert("height".to_string(), Schema::Nat { max: Some(4), edges: vec![0, 100] });
    check(
        "uncovered_shown",
        "TraceLean.Lines.markers",
        "crates/core/src/trace/lines.rs::markers",
        &["lines", "top", "height"],
        &[],
        window,
        334,
    );
}

/// The generators reach lines run by two tests, lines run by none, spans
/// partly run and spans with nothing executable in them.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_shared_unrun_and_partial_lines() {
    let mut rng = gen::Rng::new(331);
    let (mut shared, mut unrun) = (0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&Schema::Struct { fields: per_test() }, &mut rng);
        let runs: Vec<TestLines> = serde_json::from_value(v["runs"].clone()).unwrap();
        let lines = merged(runs, "a.rs".into());
        if lines.iter().any(|l| l.tests.len() > 1) {
            shared += 1;
        }
        if lines.iter().any(|l| l.hits == 0) {
            unrun += 1;
        }
    }
    support::covered("REQ-LINECOV.per_test", &[("a line two tests ran", shared), ("a line no test ran", unrun)]);

    let mut rng = gen::Rng::new(332);
    let (mut partial, mut empty) = (0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&Schema::Struct { fields: summary() }, &mut rng);
        let lines: Vec<LineHits> = serde_json::from_value(v["lines"].clone()).unwrap();
        let reach = span_coverage(lines, v["start"].as_u64().unwrap() as u32, v["stop"].as_u64().unwrap() as u32);
        if reach.run > 0 && reach.run < reach.all {
            partial += 1;
        }
        if reach.all == 0 {
            empty += 1;
        }
    }
    support::covered("REQ-LINECOV.clause_summary", &[("some lines run and some not", partial), ("no executable line inside", empty)]);

    let mut rng = gen::Rng::new(333);
    let (mut same, mut other) = (0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&Schema::Struct { fields: measured() }, &mut rng);
        if let Some(at) = v["measured"].as_array() {
            if at[0] == v["hash"] {
                same += 1;
            } else {
                other += 1;
            }
        }
    }
    support::covered(
        "REQ-LINECOV.stale_hidden",
        &[("measured against the file's text", same), ("measured against other text", other)],
    );
}
