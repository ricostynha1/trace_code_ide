//! The wire protocol, checked against the model.
//!
//! This is the one coupling everything else in the differential machinery rests
//! on, which makes it the worst place for a silent disagreement: a harness that
//! misreads a reply reports divergences that are not there, or — much worse —
//! agreement that is not there either.
//!
//! The generator builds lines out of fragments rather than from random strings.
//! A random string is not JSON, so a random alphabet would test one branch two
//! thousand times.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Lines a runner might emit: correct replies, replies to the wrong case,
/// replies carrying both fields or neither, a `null` output (which is an
/// answer), an error that is not a string, and three ways of not being a reply
/// at all.
fn line() -> Schema {
    Schema::Str {
        max_len: Some(1),
        examples: vec![
            r#"{"case":0,"output":1}"#.into(),
            r#"{"case":1,"output":"x"}"#.into(),
            r#"{"case":1,"output":null}"#.into(),
            r#"{"case":1,"error":"boom"}"#.into(),
            r#"{"case":1,"output":1,"error":"boom"}"#.into(),
            r#"{"case":1}"#.into(),
            // Ill-formed replies need the case to match before they are
            // reached at all, so there is one for each `expected` the
            // generator draws rather than only for case 1.
            r#"{"case":0}"#.into(),
            r#"{"case":2}"#.into(),
            r#"{"case":0,"output":1,"error":"boom"}"#.into(),
            r#"{"case":2,"output":[1,2]}"#.into(),
            r#"{"case":1,"error":7}"#.into(),
            r#"{"case":"one","output":1}"#.into(),
            r#"{"output":1}"#.into(),
            "  {\"case\":1,\"output\":1}  ".into(),
            "[1,2]".into(),
            "7".into(),
            "not json".into(),
            "".into(),
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[
        ("expected", Schema::Nat { max: Some(2), edges: vec![0, 1, 2] }),
        ("line", line()),
    ])
}

/// @drt REQ-DRT-PROTO.line_delimited
/// @tests REQ-DRT-PROTO.line_delimited
/// @tests REQ-DRT-PROTO.case_echoed
/// @tests REQ-DRT-PROTO.reply_exclusive
/// @tests REQ-DRT-PROTO.failure_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_line_says() {
    let scratch = harness::scratch("protocol");
    let op = "REQ-DRT-PROTO.line_delimited";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "line_delimited",
        "crates/core/src/drt/protocol.rs::hear",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Protocol",
        "TraceLean.Protocol.hear",
        op,
        &["expected", "line"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 17, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-DRT-PROTO.op_dispatch
/// @tests REQ-DRT-PROTO.op_dispatch
/// @tests REQ-DRT-PROTO.runner_shared
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_ops_collide() {
    let scratch = harness::scratch("protocol-ops");
    let op = "REQ-DRT-PROTO.op_dispatch";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "op_dispatch",
        "crates/core/src/drt/protocol.rs::duplicate_ops",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Protocol",
        "TraceLean.Protocol.duplicateOps",
        op,
        &["ops"],
        &scratch,
    );

    let schema = strukt(&[(
        "ops",
        Schema::List {
            // A three-name alphabet, so collisions are the common case rather
            // than a coincidence: the law is about what happens when two
            // bindings share an op, and distinct random names never do.
            inner: Box::new(Schema::Str {
                max_len: Some(0),
                examples: vec!["a".into(), "b".into(), "c".into()],
            }),
            max_len: Some(5),
        },
    )]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 23, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every conclusion the protocol can reach must occur, or agreement is
/// agreement about one branch.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_conclusion() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::protocol::{hear, Heard, NotAReply};

    let schema = input_schema();
    let mut rng = gen::Rng::new(17);
    let (mut answered, mut wrong, mut exclusive, mut reasons) =
        (0, 0, 0, std::collections::BTreeSet::new());
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let expected = value["expected"].as_u64().unwrap();
        let line = value["line"].as_str().unwrap().to_string();
        match hear(expected, line) {
            Heard::Answered { .. } => answered += 1,
            Heard::WrongCase { .. } => wrong += 1,
            Heard::NotExclusive { .. } => exclusive += 1,
            Heard::NotAReply { reason } => {
                reasons.insert(format!("{reason:?}"));
            }
        }
    }

    support::covered(
        "REQ-DRT-PROTO.line_delimited",
        &[
            ("an answer", answered),
            ("an answer to another case", wrong),
            ("an ill-formed reply", exclusive),
        ],
    );
    assert_eq!(
        reasons.len(),
        4,
        "not every way of not being a reply occurred: {reasons:?}"
    );
    let _ = NotAReply::NotJson;
}

/// When two answers agree, checked against the model.
///
/// The case that matters is the one an obvious implementation gets wrong: both
/// sides refusing an input is agreement, not a failed run. Getting that wrong
/// makes every partial function untestable, and the failure is silent — the
/// suite reports an error rather than a divergence, and somebody assumes the
/// harness is broken.
///
/// @drt REQ-DRT.error_is_an_answer
/// @tests REQ-DRT.error_is_an_answer
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_when_two_answers_agree() {
    let scratch = harness::scratch("agree");
    let op = "REQ-DRT.error_is_an_answer";
    let implementation = harness::rust_runner(
        "REQ-DRT",
        "error_is_an_answer",
        "crates/core/src/drt/protocol.rs::agree",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Protocol", "TraceLean.Protocol.agree", op, &["model", "implementation"], &scratch);

    // Answers a model and an implementation might give, including `null` —
    // which is an answer, not an absence.
    let answer = || Schema::Option {
        inner: Box::new(Schema::Enum {
            variants: [
                ("number".to_string(), Some(Box::new(Schema::Nat { max: Some(2), edges: vec![0] }))),
                (
                    "text".to_string(),
                    Some(Box::new(Schema::Str { max_len: Some(1), examples: vec!["a".into()] })),
                ),
            ]
            .into_iter()
            .collect(),
        }),
    };
    let schema = strukt(&[("model", answer()), ("implementation", answer())]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 83, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What a clean run is worth, checked against the model.
///
/// @drt REQ-DRT.falsification_only
/// @tests REQ-DRT.falsification_only
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_that_finding_nothing_is_not_a_proof() {
    let scratch = harness::scratch("falsification");
    let op = "REQ-DRT.falsification_only";
    let implementation = harness::rust_runner(
        "REQ-DRT",
        "falsification_only",
        "crates/core/src/drt/protocol.rs::drt_level",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Protocol", "TraceLean.Protocol.drtLevel", op, &["agreed"], &scratch);

    let result = run(
        op,
        &strukt(&[("agreed", Schema::Bool)]),
        &model,
        &implementation,
        RunOptions { seed: 89, cases: 200, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
