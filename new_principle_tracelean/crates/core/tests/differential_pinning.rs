//! Pinning per input, checked against the model.
//!
//! The failure this guards is a clause shown pinned when Lean never accepted
//! the theorem as stated — an obligation text that drifted, an answer read too
//! generously, or a verdict outliving the declarations it was about.

mod harness;
mod support;

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::trace::pinning::{accepted, standing, PinRecord};

fn small(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

fn fields(pairs: Vec<(&str, Schema)>) -> BTreeMap<String, Schema> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn statement() -> BTreeMap<String, Schema> {
    fields(vec![
        ("spec", small(&["P", "Thermo.ToFahrenheit"])),
        ("model", small(&["f", "Thermo.toFahrenheit"])),
        ("inputs", Schema::Nat { max: Some(4), edges: vec![0, 1, 4] }),
    ])
}

const OUTPUTS: &[&str] = &[
    "'T.p' depends on axioms: [propext, Quot.sound]\n",
    "'T.p' depends on axioms: [propext, sorryAx]\n",
    "warning: declaration uses 'sorry'\n'T.p' depends on axioms: [sorryAx]\n",
    "Check.lean:3:2: error: type mismatch\n'T.p' depends on axioms: [propext]\n",
    "'T.p' does not depend on any axioms\n",
    "'T.q' depends on axioms: [propext]\n",
    "",
];

fn answer() -> BTreeMap<String, Schema> {
    fields(vec![
        ("theoremName", small(&["T.p", "T.q"])),
        ("output", small(OUTPUTS)),
        ("exitedOk", Schema::Bool),
    ])
}

fn verdict() -> BTreeMap<String, Schema> {
    let record = Schema::Struct {
        fields: fields(vec![
            ("theoremName", small(&["t", "u"])),
            ("key", small(&["a+b", "a+c"])),
            ("pinned", Schema::Bool),
            ("said", small(&["", "error"])),
        ]),
    };
    fields(vec![
        ("theoremName", Schema::Option { inner: Box::new(small(&["t", "u"])) }),
        ("record", Schema::Option { inner: Box::new(record) }),
        ("key", small(&["a+b", "a+c"])),
    ])
}

fn check(clause: &str, function: &str, entry: &str, arguments: &[&str], fields: BTreeMap<String, Schema>, seed: u64) {
    let op = format!("REQ-STRENGTH.{clause}");
    let scratch = harness::scratch(&format!("pinning-{clause}"));
    let params: &[(&str, &str)] = match clause {
        "kernel_decides" => &[("theoremName", "theorem_name"), ("exitedOk", "exited_ok")],
        "verdict_kept" => &[("theoremName", "theorem_name")],
        _ => &[],
    };
    let implementation = harness::rust_runner_with_params("REQ-STRENGTH", clause, entry, params, &scratch);
    let model = harness::lean_runner("TraceLean.Pinning", function, &op, arguments, &scratch);
    let result = run(&op, &Schema::Struct { fields }, &model, &implementation, RunOptions { seed, cases: 2_000, shrink_rounds: 100 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-STRENGTH.per_input
/// @tests REQ-STRENGTH.per_input
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_obligation() {
    check("per_input", "TraceLean.Pinning.statement", "crates/core/src/trace/pinning.rs::statement", &["spec", "model", "inputs"], statement(), 311);
}

/// @drt REQ-STRENGTH.kernel_decides
/// @tests REQ-STRENGTH.kernel_decides
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_lean_accepted() {
    check("kernel_decides", "TraceLean.Pinning.accepted", "crates/core/src/trace/pinning.rs::accepted", &["theoremName", "output", "exitedOk"], answer(), 312);
}

/// @drt REQ-STRENGTH.verdict_kept
/// @tests REQ-STRENGTH.verdict_kept
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_a_clause_stands() {
    check("verdict_kept", "TraceLean.Pinning.standing", "crates/core/src/trace/pinning.rs::standing", &["theoremName", "record", "key"], verdict(), 313);
}

/// The generators reach what the clauses are about.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_each_answer_and_each_verdict() {
    let mut rng = gen::Rng::new(311);
    let (mut none, mut several) = (0u64, 0u64);
    for _ in 0..2_000 {
        match gen::value(&Schema::Struct { fields: statement() }, &mut rng)["inputs"].as_u64() {
            Some(0) => none += 1,
            Some(n) if n > 1 => several += 1,
            _ => {}
        }
    }
    support::covered("REQ-STRENGTH.per_input", &[("a model of no arguments", none), ("a model of several arguments", several)]);

    let mut rng = gen::Rng::new(312);
    let (mut yes, mut sorry, mut error) = (0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&Schema::Struct { fields: answer() }, &mut rng);
        let (name, output, ok) = (v["theoremName"].as_str().unwrap(), v["output"].as_str().unwrap(), v["exitedOk"].as_bool().unwrap());
        if accepted(name.into(), output.into(), ok) {
            yes += 1;
        } else if ok && output.contains("sorryAx") && output.contains(&format!("'{name}'")) && !output.contains(": error") {
            sorry += 1;
        } else if output.contains(": error") && output.contains(&format!("'{name}'")) {
            error += 1;
        }
    }
    support::covered("REQ-STRENGTH.kernel_decides", &[("accepted", yes), ("rejected for sorry", sorry), ("rejected for an error", error)]);

    let mut rng = gen::Rng::new(313);
    let (mut pinned, mut other) = (0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&Schema::Struct { fields: verdict() }, &mut rng);
        let theorem: Option<String> = serde_json::from_value(v["theoremName"].clone()).unwrap();
        let record: Option<PinRecord> = serde_json::from_value(v["record"].clone()).unwrap();
        let key = v["key"].as_str().unwrap().to_string();
        if record.as_ref().is_some_and(|r| r.pinned && r.key != key) && theorem.is_some() {
            other += 1;
        }
        if standing(theorem, record, key).as_str() == "pinned" {
            pinned += 1;
        }
    }
    support::covered("REQ-STRENGTH.verdict_kept", &[("pinned", pinned), ("a verdict about other declarations", other)]);
}
