//! The generated Rust call site, checked against the model.
//!
//! The failure this guards is a generator that starts naming types — a
//! turbofish, an ascription on a local — so that a binding's argument could be
//! read as a type the function does not take, or a literal written so that the
//! op or a field name the runner dispatches on is not the one the binding said.

mod harness;
mod support;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::rust_runner::{call_site, Entry};
use tracelean_core::drt::schema::Schema;

fn names() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["a".into(), "b".into(), "src".into()] }
}

/// Ops and field names, including ones a literal has to escape.
fn text() -> Schema {
    Schema::Str {
        max_len: Some(3),
        examples: vec![
            "REQ-X.c".into(),
            "a\"b".into(),
            "a\\b".into(),
            "x\ny".into(),
            "caf\u{e9}".into(),
            "\u{1}".into(),
        ],
    }
}

fn entry() -> Schema {
    Schema::Struct {
        fields: [
            ("op".to_string(), text()),
            (
                "call_path".to_string(),
                Schema::Str { max_len: Some(0), examples: vec!["k::f".into(), "tracelean_core::drt::gen::value".into()] },
            ),
            ("parameters".to_string(), Schema::List { inner: Box::new(names()), max_len: Some(3) }),
            (
                "sources".to_string(),
                Schema::List {
                    inner: Box::new(Schema::Str {
                        max_len: Some(0),
                        examples: vec!["a".into(), "b".into(), "src".into(), "x\"y".into()],
                    }),
                    max_len: Some(3),
                },
            ),
        ]
        .into_iter()
        .collect(),
    }
}

fn input() -> Schema {
    Schema::Struct { fields: [("entry".to_string(), entry())].into_iter().collect() }
}

/// @drt REQ-DRT-RUST.types_inferred
/// @tests REQ-DRT-RUST.types_inferred
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_generated_call_site() {
    let op = "REQ-DRT-RUST.types_inferred";
    let scratch = harness::scratch("call-site");
    let implementation = harness::rust_runner(
        "REQ-DRT-RUST",
        "types_inferred",
        "crates/core/src/drt/rust_runner.rs::call_site",
        &scratch,
    );
    let model = harness::lean_runner("TraceLean.RustRunner", "TraceLean.RustRunner.callSite", op, &["entry"], &scratch);
    let result = run(op, &input(), &model, &implementation, RunOptions { seed: 4242, cases: 5_000, shrink_rounds: 100 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The generator reaches entries taking nothing and several arguments, fields
/// read under another name, and literals that need an escape — and no arm it
/// generates names a type.
///
/// @tests REQ-DRT-COVER.law_coverage
/// @tests REQ-DRT-RUST.types_inferred
#[test]
fn generation_reaches_every_shape_of_call_site() {
    let mut rng = gen::Rng::new(4242);
    let mut counts = [0u64; 4];
    for _ in 0..5_000 {
        let v = gen::value(&input(), &mut rng);
        let entry: Entry = serde_json::from_value(v["entry"].clone()).unwrap();
        let escapes = |s: &str| s.chars().any(|c| c == '"' || c == '\\' || !(' '..='~').contains(&c));
        if entry.parameters.is_empty() {
            counts[0] += 1;
        }
        if entry.parameters.len() > 1 {
            counts[1] += 1;
        }
        if entry.parameters.iter().zip(&entry.sources).any(|(p, s)| p != s) {
            counts[2] += 1;
        }
        if escapes(&entry.op) || entry.sources.iter().any(|s| escapes(s)) {
            counts[3] += 1;
        }
        let arm = call_site(entry);
        assert!(!arm.contains("::<") && !arm.contains("let a:"), "the call site named a type:\n{arm}");
    }
    support::covered(
        "REQ-DRT-RUST.types_inferred",
        &[
            ("an entry taking nothing", counts[0]),
            ("an entry taking several arguments", counts[1]),
            ("an argument read from a renamed field", counts[2]),
            ("a literal needing an escape", counts[3]),
        ],
    );
}
