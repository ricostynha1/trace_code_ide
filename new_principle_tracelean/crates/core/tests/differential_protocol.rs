mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

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
    let op = "REQ-DRT-PROTO.reply_one_line";

    harness::agree(op, &input_schema(), 17, 2_000);
}

/// @drt REQ-DRT-PROTO.ops_unique
/// @tests REQ-DRT-PROTO.ops_unique
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_ops_collide() {
    let op = "REQ-DRT-PROTO.ops_unique";

    harness::agree(op, &ops_input(), 23, 2_000);
}

fn ops_input() -> Schema {
    strukt(&[(
        "ops",
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: Some(0),
                examples: vec!["a".into(), "b".into(), "c".into()],
            }),
            max_len: Some(5),
        },
    )])
}

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
    harness::covered("REQ-DRT-PROTO.ops_unique", &[("a collision", collided), ("no collision", clean)]);
}

/// @drt REQ-DRT-PROTO.reply_exclusive
/// @tests REQ-DRT-PROTO.reply_exclusive
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_exclusive() {
    let op = "REQ-DRT-PROTO.reply_exclusive";
    harness::agree(op, &exclusive_input(), 29, 1_000);
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
    harness::covered(
        "REQ-DRT-PROTO.reply_exclusive",
        &[
            ("an output only", counts[0]),
            ("an error only", counts[1]),
            ("both", counts[2]),
            ("neither", counts[3]),
        ],
    );
}

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
    let op = "REQ-DRT-PROTO.failure_named";
    harness::agree(op, &event_input(), 31, 2_000);
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
    harness::covered(
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
    let op = "REQ-DRT-PROTO.case_one_line";
    harness::agree(op, &case_input(), 37, 2_000);
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
        counts[0] += u64::from(text.contains("\\n") || text.contains("\\r"));
        counts[1] += u64::from(text.contains("\\\"") || text.contains("\\\\"));
        counts[2] += u64::from(text.contains("\\t") || text.contains("\\u0001") || text.contains("\\b"));
    }
    harness::covered(
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
    let op = "REQ-DRT-PROTO.runner_shared";
    harness::agree(op, &placed_input(), 41, 2_000);
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
    harness::covered(
        "REQ-DRT-PROTO.runner_shared",
        &[("two languages", mixed), ("an op placed twice", repeated)],
    );
}

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

    harness::covered(
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
        RunOptions { seed: 83, cases: 2_000 },
    )
    .expect("both runners answer");

    harness::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

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
        RunOptions { seed: 89, cases: 200 },
    )
    .expect("both runners answer");

    harness::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
