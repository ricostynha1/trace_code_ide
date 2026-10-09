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

use std::collections::BTreeMap;

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

/// @drt REQ-DRT-PROTO.reply_one_line
/// @tests REQ-DRT-PROTO.reply_one_line
/// @tests REQ-DRT-PROTO.case_echoed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_line_says() {
    let scratch = harness::scratch("protocol");
    let op = "REQ-DRT-PROTO.reply_one_line";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "reply_one_line",
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

/// @drt REQ-DRT-PROTO.ops_unique
/// @tests REQ-DRT-PROTO.ops_unique
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_ops_collide() {
    let scratch = harness::scratch("protocol-ops");
    let op = "REQ-DRT-PROTO.ops_unique";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "ops_unique",
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

    let result = run(
        op,
        &ops_input(),
        &model,
        &implementation,
        RunOptions { seed: 23, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

fn ops_input() -> Schema {
    strukt(&[(
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
    )])
}

/// Both an op that collides and one that does not must occur.
///
/// @tests REQ-DRT-PROTO.ops_unique
#[test]
fn generation_reaches_collisions_and_their_absence() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::protocol::duplicate_ops;

    let schema = ops_input();
    let mut rng = gen::Rng::new(23);
    let (mut collided, mut clean) = (0, 0);
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let ops: Vec<String> = serde_json::from_value(value["ops"].clone()).unwrap();
        if duplicate_ops(ops).is_empty() {
            clean += 1;
        } else {
            collided += 1;
        }
    }
    support::covered("REQ-DRT-PROTO.ops_unique", &[("a collision", collided), ("no collision", clean)]);
}

/// Whether a reply carries exactly one of an output and an error.
///
/// @drt REQ-DRT-PROTO.reply_exclusive
/// @tests REQ-DRT-PROTO.reply_exclusive
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_exclusive() {
    let scratch = harness::scratch("protocol-exclusive");
    let op = "REQ-DRT-PROTO.reply_exclusive";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "reply_exclusive",
        "crates/core/src/drt/protocol.rs::is_exclusive",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Protocol",
        "TraceLean.Protocol.isExclusive",
        op,
        &["output", "error"],
        &scratch,
    );
    let result = run(
        op,
        &exclusive_input(),
        &model,
        &implementation,
        RunOptions { seed: 29, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

fn exclusive_input() -> Schema {
    strukt(&[
        ("output", Schema::Option { inner: Box::new(Schema::Nat { max: Some(3), edges: vec![0] }) }),
        (
            "error",
            Schema::Option {
                inner: Box::new(Schema::Str { max_len: Some(2), examples: vec!["boom".into()] }),
            },
        ),
    ])
}

/// @tests REQ-DRT-PROTO.reply_exclusive
#[test]
fn generation_reaches_every_shape_of_reply() {
    use tracelean_core::drt::gen;

    let schema = exclusive_input();
    let mut rng = gen::Rng::new(29);
    let mut counts = [0u64; 4];
    for _ in 0..1_000 {
        let value = gen::value(&schema, &mut rng);
        let at = match (value["output"].is_null(), value["error"].is_null()) {
            (false, true) => 0,
            (true, false) => 1,
            (false, false) => 2,
            (true, true) => 3,
        };
        counts[at] += 1;
    }
    support::covered(
        "REQ-DRT-PROTO.reply_exclusive",
        &[
            ("an output only", counts[0]),
            ("an error only", counts[1]),
            ("both", counts[2]),
            ("neither", counts[3]),
        ],
    );
}

/// What a case came to, including the ways a runner fails to answer at all.
///
/// The process-level failures cannot be produced on demand by a real runner,
/// so the events are generated: what is compared is the conclusion drawn from
/// each, which is the decision `Runner::ask` delegates to `outcome`.
fn event_input() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    let reason = || Schema::Str { max_len: Some(3), examples: vec!["exit 101".into()] };
    variants.insert("couldNotStart".into(), Some(Box::new(strukt(&[("reason", reason())]))));
    variants.insert("noReplyInTime".into(), None);
    variants.insert("pipeBroke".into(), Some(Box::new(strukt(&[("reason", reason())]))));
    variants.insert(
        "read".into(),
        Some(Box::new(strukt(&[
            ("expected", Schema::Nat { max: Some(2), edges: vec![0, 1, 2] }),
            // Weighted toward answers to each case `expected` can be, so that
            // an answer is as common as each way of failing to give one.
            (
                "got",
                Schema::Option {
                    inner: Box::new(Schema::Str {
                        max_len: Some(0),
                        examples: vec![
                            r#"{"case":0,"output":1}"#.into(),
                            r#"{"case":1,"output":null}"#.into(),
                            r#"{"case":2,"error":"boom"}"#.into(),
                            r#"{"case":1}"#.into(),
                            "not json".into(),
                        ],
                    }),
                },
            ),
        ]))),
    );
    strukt(&[("event", Schema::Enum { variants })])
}

/// @drt REQ-DRT-PROTO.failure_named
/// @tests REQ-DRT-PROTO.failure_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_case_came_to() {
    let scratch = harness::scratch("protocol-outcome");
    let op = "REQ-DRT-PROTO.failure_named";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "failure_named",
        "crates/core/src/drt/protocol.rs::outcome",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Protocol",
        "TraceLean.Protocol.outcome",
        op,
        &["event"],
        &scratch,
    );
    let result = run(
        op,
        &event_input(),
        &model,
        &implementation,
        RunOptions { seed: 31, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-PROTO.failure_named
#[test]
fn generation_reaches_every_outcome() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::protocol::{outcome, Event, Heard, Outcome};

    let schema = event_input();
    let mut rng = gen::Rng::new(31);
    let mut counts = [0u64; 5];
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let event: Event = serde_json::from_value(value["event"].clone()).unwrap();
        let at = match outcome(event) {
            Outcome::CannotStart { .. } => 0,
            Outcome::TimedOut => 1,
            Outcome::Died { .. } => 2,
            Outcome::Heard { conclusion: Heard::Answered { .. } } => 4,
            Outcome::Heard { .. } => 3,
        };
        counts[at] += 1;
    }
    support::covered(
        "REQ-DRT-PROTO.failure_named",
        &[
            ("cannot start", counts[0]),
            ("timed out", counts[1]),
            ("died", counts[2]),
            ("a non-reply", counts[3]),
            ("answered", counts[4]),
        ],
    );
}

/// Inputs carrying the characters that would end a line, or a string, if they
/// were written unescaped.
fn case_input() -> Schema {
    let text = Schema::Str {
        max_len: Some(3),
        examples: vec![
            "a\nb".into(),
            "\r\n".into(),
            "say \"hi\"".into(),
            "back\\slash".into(),
            "\ttab".into(),
            "\u{1}\u{8}\u{c}".into(),
            "é ☃".into(),
        ],
    };
    strukt(&[
        ("number", Schema::Nat { max: Some(1_000_000), edges: vec![0, 1] }),
        ("op", Schema::Str { max_len: Some(4), examples: vec!["REQ-X.c".into(), "a\"b".into()] }),
        (
            "input",
            strukt(&[
                ("text", text.clone()),
                ("items", Schema::List { inner: Box::new(text), max_len: Some(2) }),
                ("count", Schema::Int { min: Some(-5), max: Some(5) }),
                ("flag", Schema::Option { inner: Box::new(Schema::Bool) }),
            ]),
        ),
    ])
}

/// @drt REQ-DRT-PROTO.case_one_line
/// @tests REQ-DRT-PROTO.case_one_line
/// @tests REQ-DRT-PROTO.case_names_op
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_a_case_is_written() {
    let scratch = harness::scratch("protocol-case");
    let op = "REQ-DRT-PROTO.case_one_line";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "case_one_line",
        "crates/core/src/drt/protocol.rs::case_line",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Protocol",
        "TraceLean.Protocol.caseLine",
        op,
        &["number", "op", "input"],
        &scratch,
    );
    let result = run(
        op,
        &case_input(),
        &model,
        &implementation,
        RunOptions { seed: 37, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-PROTO.case_one_line
#[test]
fn generation_reaches_every_character_that_needs_escaping() {
    use tracelean_core::drt::gen;

    let schema = case_input();
    let mut rng = gen::Rng::new(37);
    let mut counts = [0u64; 3];
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let text = value["input"].to_string() + &value["op"].to_string();
        // Searched in the escaped text: `\n` there is a line break in the value.
        counts[0] += u64::from(text.contains("\\n") || text.contains("\\r"));
        counts[1] += u64::from(text.contains("\\\"") || text.contains("\\\\"));
        counts[2] += u64::from(text.contains("\\t") || text.contains("\\u0001") || text.contains("\\b"));
    }
    support::covered(
        "REQ-DRT-PROTO.case_one_line",
        &[
            ("an input with a line break", counts[0]),
            ("an input with a quote or backslash", counts[1]),
            ("an input with a control character", counts[2]),
        ],
    );
}

fn placed_input() -> Schema {
    strukt(&[(
        "placed",
        Schema::List {
            inner: Box::new(strukt(&[
                ("op", Schema::Str { max_len: Some(0), examples: vec!["a".into(), "b".into(), "c".into()] }),
                (
                    "language",
                    Schema::Str {
                        max_len: Some(0),
                        examples: vec!["rust".into(), "typescript".into(), "lean".into()],
                    },
                ),
            ])),
            max_len: Some(5),
        },
    )])
}

/// @drt REQ-DRT-PROTO.runner_shared
/// @tests REQ-DRT-PROTO.runner_shared
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_runners_a_project_needs() {
    let scratch = harness::scratch("protocol-shared");
    let op = "REQ-DRT-PROTO.runner_shared";
    let implementation = harness::rust_runner(
        "REQ-DRT-PROTO",
        "runner_shared",
        "crates/core/src/drt/protocol.rs::shared_runners",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Protocol",
        "TraceLean.Protocol.sharedRunners",
        op,
        &["placed"],
        &scratch,
    );
    let result = run(
        op,
        &placed_input(),
        &model,
        &implementation,
        RunOptions { seed: 41, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-PROTO.runner_shared
#[test]
fn generation_reaches_mixed_languages_and_repeated_ops() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::protocol::Placed;

    let schema = placed_input();
    let mut rng = gen::Rng::new(41);
    let (mut mixed, mut repeated) = (0, 0);
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let placed: Vec<Placed> = serde_json::from_value(value["placed"].clone()).unwrap();
        let languages: std::collections::BTreeSet<&str> =
            placed.iter().map(|p| p.language.as_str()).collect();
        mixed += u64::from(languages.len() > 1);
        let pairs: std::collections::BTreeSet<(&str, &str)> =
            placed.iter().map(|p| (p.op.as_str(), p.language.as_str())).collect();
        repeated += u64::from(pairs.len() < placed.len());
    }
    support::covered(
        "REQ-DRT-PROTO.runner_shared",
        &[("two languages", mixed), ("an op placed twice", repeated)],
    );
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
        "REQ-DRT-PROTO.reply_one_line",
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
