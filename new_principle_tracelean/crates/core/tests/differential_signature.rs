//! Reading a function's parameter names, checked against the model.
//!
//! This parser decides the order arguments are passed in. Get it wrong and the
//! generated runner still compiles, still runs, and compares the model against
//! the implementation called with its arguments swapped — and the divergence
//! that follows looks like a bug in the code under test rather than in the
//! harness.
//!
//! The fragments below are the cases that actually broke it or could: a
//! parenthesis inside a generic bound, a `>` that is half of an arrow, `self`
//! in its four spellings, a pattern parameter with no name to bind, and a comma
//! inside a type.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn source() -> Schema {
    Schema::Str {
        max_len: Some(1),
        examples: vec![
            "pub fn f() {}".into(),
            "fn g() -> u8 { 1 }".into(),
            "impl S { pub fn h(&self) -> u8 { 1 } }".into(),
            "pub fn f(x: u8) -> u8 { x }".into(),
            "pub fn f(a: u8, b: String) {}".into(),
            "fn apply<F: Fn(u8) -> u8>(f: F, x: u8) -> u8 { f(x) }".into(),
            "fn g<T>(items: Vec<T>, pairs: Vec<(A, B)>) {}".into(),
            "fn h(v: [u8; 2], w: &mut Vec<u8>) {}".into(),
            "impl S { pub fn f(&self, x: u8) {} }".into(),
            "impl S { pub fn f(&mut self, x: u8) {} }".into(),
            "impl S { pub fn f(self, x: u8) {} }".into(),
            "impl S { pub fn f(self: Box<Self>, x: u8) {} }".into(),
            "fn f((a, b): (u8, u8)) {}".into(),
            "fn f(mut x: u8) {}".into(),
            "fn f(\n    a: u8,\n    b: u8,\n) {}".into(),
            "fn ff(x: u8) {}\npub fn f(y: u8) {}".into(),
            "fn f(x: u8)".into(),
            "fn f(x: u8".into(),
            "// fn f(x: u8) {}".into(),
            // A signature in a comment or a string above the real one, which
            // the first textual match used to take.
            "// fn f(b: u8, a: u8)\nfn f(a: u8, b: u8) {}".into(),
            "/* fn f(b: u8, a: u8) */ fn f(a: u8, b: u8) {}".into(),
            "const S: &str = \"fn f(b: u8)\";\nfn f(a: u8) {}".into(),
            "const Q: char = '\"';\nfn f(a: u8) {}".into(),
            // Two declarations: refused, not the first one taken.
            "fn f(a: u8) {}\nimpl S { fn f(&self, b: u8) {} }".into(),
            "fn g(a: u8) {}\nfn g(b: u8, c: u8) {}".into(),
            "fn h() {}\nimpl S { fn h(&self) {} }".into(),
            "fn apply(x: u8) {}\n// fn apply(z: u8)\nfn apply(y: u8) {}".into(),
            "fn f(a: u8) {}\nfn g(a: u8) {}\nfn h(a: u8) {}\nfn f(b: u8) {}\nfn g(b: u8) {}\nfn h(b: u8) {}".into(),
            "".into(),
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[
        ("source", source()),
        (
            "symbol",
            Schema::Str {
                max_len: Some(1),
                examples: vec!["f".into(), "g".into(), "h".into(), "apply".into(), "ff".into()],
            },
        ),
    ])
}

/// @drt REQ-DRT-RUST.params_from_source
/// @tests REQ-DRT-RUST.params_from_source
/// @tests REQ-DRT-RUST.types_inferred
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_signature() {
    let scratch = harness::scratch("signature");
    let op = "REQ-DRT-RUST.params_from_source";
    let implementation = harness::rust_runner(
        "REQ-DRT-RUST",
        "params_from_source",
        "crates/core/src/drt/signature.rs::parameters_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Signature",
        "TraceLean.Signature.parameters",
        op,
        &["source", "symbol"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 64, cases: 4_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    // The agreeing half of what this binding needs; the coverage half comes
    // from the generation test, and whichever lands second writes the record.
    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every answer the parser can give must occur: named parameters, none at all,
/// a method whose receiver is dropped, and a signature it refuses to bind.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_answer_the_parser_can_give() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::signature::{declarations, parameters};

    let schema = input_schema();
    let mut rng = gen::Rng::new(64);
    let (mut named, mut empty, mut refused, mut absent) = (0, 0, 0, 0);
    let (mut past_comment, mut ambiguous) = (0, 0);
    // As many cases as the differential run asks, from the same seed.
    for _ in 0..4_000 {
        let v = gen::value(&schema, &mut rng);
        let source = v["source"].as_str().unwrap();
        let symbol = v["symbol"].as_str().unwrap();
        let textual = source.matches(&format!("fn {symbol}(")).count();
        if textual > 1 && declarations(source, symbol) == 1 && parameters(source, symbol).is_some() {
            past_comment += 1;
        }
        if declarations(source, symbol) > 1 && parameters(source, symbol).is_none() {
            ambiguous += 1;
        }
        match parameters(source, symbol) {
            Some(names) if !names.is_empty() => named += 1,
            Some(_) => empty += 1,
            None if source.contains(&format!("fn {symbol}(")) => refused += 1,
            None => absent += 1,
        }
    }
    support::covered(
        "REQ-DRT-RUST.params_from_source",
        &[
            ("a signature yielding names", named),
            ("a signature taking no parameters", empty),
            ("a declared signature refused", refused),
            ("an absent symbol", absent),
            ("a signature in a comment or string passed over", past_comment),
            ("two declarations refused", ambiguous),
        ],
    );
}
