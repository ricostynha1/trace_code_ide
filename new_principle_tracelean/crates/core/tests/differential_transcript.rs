//! Reading a transcript that is still being written, checked against the model.
//!
//! The case that matters is the one that is hard to write by hand: a stream of
//! records where the last one is cut off mid-write, sometimes after a complete
//! JSON object, sometimes in the middle of a string literal, sometimes after
//! nothing at all. Generating them from fragments produces all three without
//! anyone having to think of them.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Fragments a transcript gets joined out of: complete records, records of a
/// shape nothing recognises, text that is not JSON at all, blank lines, and
/// records cut off part-way through.
fn fragment() -> Schema {
    Schema::Str {
        max_len: Some(2),
        examples: vec![
            r#"{"type":"say","text":"one"}"#.into(),
            r#"{"type":"tool","content":"ls"}"#.into(),
            r#"{"type":"say","text":"one","extra":3}"#.into(),
            r#"{"something":"else"}"#.into(),
            r#"{"type":7,"text":"one"}"#.into(),
            r#"{"type":"say"}"#.into(),
            r#"{"type":"say","te"#.into(),
            "not json".into(),
            "".into(),
            "   ".into(),
            "[1,2]".into(),
            "null".into(),
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[(
        "chunks",
        Schema::List { inner: Box::new(fragment()), max_len: Some(4) },
    )])
}

/// @drt REQ-TRANSCRIPT.partial_line_held
/// @tests REQ-TRANSCRIPT.partial_line_held
/// @tests REQ-TRANSCRIPT.unknown_preserved
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_complete() {
    let scratch = harness::scratch("transcript");
    let op = "REQ-TRANSCRIPT.partial_line_held";
    let implementation = harness::rust_runner(
        "REQ-TRANSCRIPT",
        "partial_line_held",
        "crates/core/src/observe/transcript.rs::read_chunks",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Transcript",
        "TraceLean.Transcript.readChunks",
        op,
        &["chunks"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 91, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every outcome the requirement names must occur: a recognised record, an
/// unrecognised one, and something held back.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_events_unrecognised_and_held() {
    use tracelean_core::drt::gen;
    use tracelean_core::observe::transcript::read_chunks;

    let schema = input_schema();
    let mut rng = gen::Rng::new(91);
    let (mut events, mut unknown, mut held, mut nothing_held) = (0, 0, 0, 0);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let chunks: Vec<String> = serde_json::from_value(v["chunks"].clone()).unwrap();
        let result = read_chunks(chunks);
        if !result.events.is_empty() {
            events += 1;
        }
        if !result.unrecognised.is_empty() {
            unknown += 1;
        }
        if result.held.is_empty() { nothing_held += 1 } else { held += 1 }
    }
    support::covered(
        "REQ-TRANSCRIPT.partial_line_held",
        &[
            ("a read producing an event", events),
            ("a read producing an unrecognised record", unknown),
            ("a read holding something back", held),
            ("a read consuming everything", nothing_held),
        ],
    );
}
