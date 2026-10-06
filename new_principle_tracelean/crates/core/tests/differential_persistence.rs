//! Replay, checkpoints and truncation, checked against the model.
//!
//! Three properties in one run. The log must replay to exactly the state it was
//! recorded from; replaying from a checkpoint must reach the same place as
//! replaying from the beginning; and a log that ends mid-entry must be reported
//! and replayed up to its last complete entry.
//!
//! The last one is why the input is a list of lines rather than a list of
//! commands: a truncated log is a *text* problem, and it cannot be generated
//! from well-typed commands.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn path_name() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["a".into(), "b".into()] }
}

fn text() -> Schema {
    Schema::Str { max_len: Some(2), examples: vec!["".into(), "hi".into()] }
}

/// Log lines. Most are commands that will apply; some are the things a log
/// contains when a process died while writing one.
///
/// `max_len: 0` so the generator's random branch yields a blank line rather
/// than a random string. Random strings are all unreadable, and the first
/// unreadable entry ends the history — so with them every generated log was two
/// entries long and nothing downstream of truncation was ever reached.
fn log_line() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec![
            r#"{"insert":{"file":"a","offset":0,"text":"x"}}"#.into(),
            r#"{"insert":{"file":"b","offset":0,"text":"yy"}}"#.into(),
            r#"{"insert":{"file":"a","offset":9,"text":"x"}}"#.into(),
            r#"{"delete":{"file":"a","offset":0,"deleted":"x"}}"#.into(),
            r#"{"createFile":{"path":"c"}}"#.into(),
            r#"{"deleteFile":{"path":"a","content":""}}"#.into(),
            r#"{"renameFile":{"from_":"a","to":"c"}}"#.into(),
            r#"{"batch":{"commands":[{"createFile":{"path":"c"}}]}}"#.into(),
            // A half-written entry, an unknown shape, and blank lines.
            r#"{"insert":{"file":"a","offs"#.into(),
            r#"{"insert":{}}"#.into(),
            r#"{"nosuch":{"x":1}}"#.into(),
            "not json".into(),
            "".into(),
            "   ".into(),
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[
        (
            "base",
            strukt(&[(
                "files",
                Schema::Tuple {
                    items: vec![
                        Schema::Tuple { items: vec![path_name(), text()] },
                        Schema::Tuple { items: vec![path_name(), text()] },
                    ],
                },
            )]),
        ),
        ("lines", Schema::List { inner: Box::new(log_line()), max_len: Some(7) }),
        ("checkpointAt", Schema::Nat { max: Some(2), edges: vec![0] }),
    ])
}

/// @drt REQ-PERSIST.replay_exact
/// @tests REQ-PERSIST.replay_exact
/// @tests ARCH-DETERMINISM.replay_exact
/// @tests REQ-PERSIST.checkpoint_equivalent
/// @tests REQ-PERSIST.truncated_is_reported
/// @tests REQ-PERSIST.append_only
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_replaying_a_log() {
    let scratch = harness::scratch("persistence");
    let op = "REQ-PERSIST.replay_exact";
    let implementation = harness::rust_runner_with_params(
        "REQ-PERSIST",
        "replay_exact",
        "crates/core/src/history/persistence.rs::replay_report",
        &[("checkpointAt", "checkpoint_at")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Persistence",
        "TraceLean.Persistence.replayReport",
        op,
        &["base", "lines", "checkpointAt"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 62, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    // The agreeing half of what this binding needs; the coverage half comes
    // from the generation test, and whichever lands second writes the record.
    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The equivalence itself, asserted against the implementation: the two replay
/// paths agree for every generated log — and the generated logs actually
/// contain truncation, refusal and success.
///
/// @tests REQ-PERSIST.checkpoint_equivalent
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_two_replay_paths_agree_on_every_generated_log() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::{Outcome, Workspace};
    use tracelean_core::history::persistence::replay_report;

    let schema = input_schema();
    let mut rng = gen::Rng::new(62);
    let (mut truncated, mut applied, mut refused, mut cut) = (0, 0, 0, 0);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let base: Workspace = serde_json::from_value(v["base"].clone()).unwrap();
        let lines: Vec<String> = serde_json::from_value(v["lines"].clone()).unwrap();
        let at = v["checkpointAt"].as_u64().unwrap() as usize;

        let report = replay_report(base, lines, at);
        assert_eq!(
            report.direct, report.via_checkpoint,
            "replaying from a checkpoint reached a different state"
        );
        if report.truncated_after.is_some() {
            truncated += 1;
        }
        match report.direct {
            Outcome::Ok { .. } => applied += 1,
            Outcome::Refused { .. } => refused += 1,
        }
        if at > 0 && at < report.commands.len() {
            cut += 1;
        }
    }
    support::covered(
        "REQ-PERSIST.replay_exact",
        &[
            ("a truncated log", truncated),
            ("a log that replayed cleanly", applied),
            ("a refused log", refused),
            ("a checkpoint part-way through a log", cut),
        ],
    );
}
