//! The round-trip law, checked against the model.
//!
//! A pure state machine, a trivial model, and random command sequences: if this
//! holds for every variant, undo cannot corrupt a buffer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use tracelean_core::drt::lean_runner::{self, LeanEntry};
use tracelean_core::drt::rust_runner;
use tracelean_core::drt::run::{run, RunOptions, RunnerSpec};
use tracelean_core::drt::schema::Schema;
use tracelean_core::drt::{Binding, CallSpec};

mod support;

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct {
        fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
    }
}

fn path_name() -> Schema {
    Schema::Str { max_len: None, examples: vec!["a.rs".into(), "b.rs".into(), "c.rs".into()] }
}

fn text() -> Schema {
    Schema::Str { max_len: Some(4), examples: vec!["".into(), "hi".into()] }
}

fn offset() -> Schema {
    Schema::Nat { max: Some(8), edges: vec![0] }
}

/// Commands that do not contain other commands.
fn simple_command() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "insert".into(),
        Some(Box::new(strukt(&[("file", path_name()), ("offset", offset()), ("text", text())]))),
    );
    variants.insert(
        "delete".into(),
        Some(Box::new(strukt(&[
            ("file", path_name()),
            ("offset", offset()),
            ("deleted", text()),
        ]))),
    );
    variants.insert("createFile".into(), Some(Box::new(strukt(&[("path", path_name())]))));
    variants.insert(
        "deleteFile".into(),
        Some(Box::new(strukt(&[("path", path_name()), ("content", text())]))),
    );
    variants.insert(
        "renameFile".into(),
        Some(Box::new(strukt(&[("from_", path_name()), ("to", path_name())]))),
    );
    Schema::Enum { variants }
}

/// Commands including one level of batching.
///
/// The schema grammar has no recursion, so batches are one deep. That is enough
/// to exercise `batch_reverses` — the property is about the order members are
/// inverted in, which a two-member batch already distinguishes — and the limit
/// is stated rather than left to be discovered.
fn command() -> Schema {
    let Schema::Enum { mut variants } = simple_command() else { unreachable!() };
    variants.insert(
        "batch".into(),
        Some(Box::new(strukt(&[(
            "commands",
            Schema::List { inner: Box::new(simple_command()), max_len: Some(3) },
        )]))),
    );
    Schema::Enum { variants }
}

fn workspace() -> Schema {
    strukt(&[(
        "files",
        Schema::List {
            inner: Box::new(Schema::Tuple { items: vec![path_name(), text()] }),
            max_len: Some(3),
        },
    )])
}

fn input_schema() -> Schema {
    strukt(&[("workspace", workspace()), ("command", command())])
}

fn rust_runner_for(entry_path: &str, op_clause: &str, scratch: &Path) -> RunnerSpec {
    let root = project_root();
    let binding = Binding {
        req_id: "REQ-CMD".into(),
        clause: Some(op_clause.into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: entry_path.into(),
            params: BTreeMap::new(),
        },
    };
    let entry = rust_runner::resolve(&root, &binding).expect("binding resolves");
    let mut deps = BTreeMap::new();
    deps.insert(
        "tracelean-core".to_string(),
        root.join("crates").join("core").display().to_string(),
    );
    rust_runner::materialize(scratch, &[entry], &deps).expect("generated");

    let dir = rust_runner::package_dir(scratch);
    let built = Process::new("cargo")
        .args(["build", "--release", "--quiet"])
        .current_dir(&dir)
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "the generated Rust runner did not compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    RunnerSpec {
        cmd: vec![dir.join("target/release/tracelean-drt-runner").display().to_string()],
        cwd: None,
    }
}

fn lean_runner_for(function: &str, op: &str, arguments: &[&str], scratch: &Path) -> RunnerSpec {
    lean_runner::toolchain().expect("a Lean toolchain");
    let root = project_root();
    let entries = [LeanEntry {
        op: op.to_string(),
        function: function.to_string(),
        arguments: arguments.iter().map(|a| a.to_string()).collect(),
    }];
    lean_runner::materialize(
        scratch,
        &["TraceLean.Command"],
        "tracelean",
        &root.join("formal").display().to_string(),
        &entries,
    )
    .expect("generated");

    let dir = lean_runner::package_dir(scratch);
    let built = Process::new("lake").arg("build").current_dir(&dir).output().expect("lake runs");
    assert!(
        built.status.success(),
        "the generated Lean runner did not build:\n{}\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    RunnerSpec {
        cmd: vec![dir.join(".lake/build/bin/drtRunner").display().to_string()],
        cwd: None,
    }
}

/// @drt REQ-CMD.round_trip
/// @tests REQ-CMD.round_trip
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_round_trip() {
    let scratch = std::env::temp_dir().join("tracelean-drt-command");
    let _ = std::fs::remove_dir_all(&scratch);

    let op = "REQ-CMD.round_trip";
    let implementation = rust_runner_for(
        "crates/core/src/history/command.rs::round_trip_outcome",
        "round_trip",
        &scratch,
    );
    let model = lean_runner_for("TraceLean.Command.roundTripOutcome", op, &["workspace", "command"], &scratch);

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 11, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);
    assert_eq!(result.cases, 3_000);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Applying is checked separately from round-tripping: a model that refused
/// everything would round-trip perfectly and be useless.
///
/// @drt REQ-CMD.total_or_refused
/// @tests REQ-CMD.total_or_refused
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_applying() {
    let scratch = std::env::temp_dir().join("tracelean-drt-apply");
    let _ = std::fs::remove_dir_all(&scratch);

    let op = "REQ-CMD.total_or_refused";
    let implementation = rust_runner_for(
        "crates/core/src/history/command.rs::apply_outcome",
        "total_or_refused",
        &scratch,
    );
    let model = lean_runner_for("TraceLean.Command.applyOutcome", op, &["workspace", "command"], &scratch);

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 5, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    // Reports the agreeing half of what this binding needs; the coverage half
    // comes from the generation test below, and whichever lands second writes
    // the L3 record.
    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The generator must reach the cases that matter: a run of only refusals would
/// agree vacuously.
///
/// @tests REQ-DRT-COVER.floor_stated
#[test]
fn generation_reaches_successful_applications() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::{apply_outcome, Command, Outcome, Workspace};

    let schema = input_schema();
    let mut rng = gen::Rng::new(11);
    let (mut ok, mut batches) = (0, 0);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let workspace: Workspace = serde_json::from_value(v["workspace"].clone()).unwrap();
        let command: Command = serde_json::from_value(v["command"].clone()).unwrap();
        if matches!(command, Command::Batch { .. }) {
            batches += 1;
        }
        if matches!(apply_outcome(workspace, command), Outcome::Ok { .. }) {
            ok += 1;
        }
    }
    support::covered(
        "REQ-CMD.total_or_refused",
        &[("applied successfully", ok), ("was a batch", batches)],
    );
}

/// A round trip only says something where the command applied.
///
/// `round_trip` applies a command and then its inverse. Where the command was
/// refused the whole thing is a refusal, and a run made of refusals would agree
/// with any model at all — including one whose inverse was wrong. So the floor
/// is on round trips that actually returned to the state they started from, and
/// separately on batches, which are where a wrong inverse hides.
///
/// @tests REQ-CMD.round_trip
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_round_trips_that_actually_returned() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::{apply, round_trip, Command, Workspace};

    let schema = input_schema();
    let mut rng = gen::Rng::new(11);
    let (mut returned, mut refused, mut batched) = (0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let workspace: Workspace = serde_json::from_value(v["workspace"].clone()).unwrap();
        let command: Command = serde_json::from_value(v["command"].clone()).unwrap();

        match round_trip(workspace.clone(), command.clone()) {
            Ok(back) => {
                // The law itself: applying a command and then its inverse is
                // the identity on the states where the command applied.
                assert_eq!(back, workspace, "a round trip did not return to where it started");
                returned += 1;
                if matches!(command, Command::Batch { .. }) {
                    batched += 1;
                }
            }
            Err(_) => {
                // A round trip is two applications, so a refusal must come
                // from one of them. The second is the interesting one: it
                // means a command applied and its inverse would not, which is
                // a broken inverse rather than a command that did not fit.
                assert!(
                    apply(&workspace, &command).is_err(),
                    "a command applied and its own inverse was refused: {command:?}"
                );
                refused += 1;
            }
        }
    }
    support::covered(
        "REQ-CMD.round_trip",
        &[
            ("a round trip that returned", returned),
            ("a round trip refused", refused),
            ("a batch that round-tripped", batched),
        ],
    );
}

/// Inverting a batch reverses it.
///
/// Worth a differential test rather than a unit test because the wrong version
/// — mapping `inverse` over the members and keeping their order — passes every
/// single-command case and every batch whose members commute. Random batches
/// find it immediately.
///
/// @drt REQ-CMD.batch_reverses
/// @tests REQ-CMD.batch_reverses
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_inverses() {
    let scratch = std::env::temp_dir().join("tracelean-drt-inverse");
    let _ = std::fs::remove_dir_all(&scratch);

    let op = "REQ-CMD.batch_reverses";
    let implementation =
        rust_runner_for("crates/core/src/history/command.rs::inverse_of", "batch_reverses", &scratch);
    let model = lean_runner_for("TraceLean.Command.inverse", op, &["command"], &scratch);

    let result = run(
        op,
        &strukt(&[("command", command())]),
        &model,
        &implementation,
        RunOptions { seed: 71, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The batches that can tell the right inverse from the wrong one.
///
/// Mapping `inverse` over a batch without reversing it passes every single
/// command and every batch of one. It also passes a batch whose members
/// commute. So the floor is on batches of more than one member, and separately
/// on batches whose members do *not* commute — those are the only cases in
/// which the law has any content, and a run made of the others would agree and
/// mean nothing.
///
/// @tests REQ-CMD.batch_reverses
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_batches_that_would_catch_a_wrong_inverse() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::{inverse_of, Command};

    let schema = strukt(&[("command", command())]);
    let mut rng = gen::Rng::new(71);
    let (mut single, mut batch, mut long, mut ordered) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let command: Command = serde_json::from_value(v["command"].clone()).unwrap();

        let inverted = inverse_of(command.clone());
        match (&command, &inverted) {
            (Command::Batch { commands }, Command::Batch { commands: back }) => {
                assert_eq!(back.len(), commands.len(), "inverting a batch changed its length");
                batch += 1;
                // The law, stated directly: each member comes back inverted, in
                // the opposite order.
                let expected: Vec<Command> =
                    commands.iter().rev().cloned().map(inverse_of).collect();
                assert_eq!(*back, expected, "the inverse of a batch is not its members reversed");
                if commands.len() > 1 {
                    long += 1;
                    // A batch whose members are all alike cannot show the
                    // reversal, so count separately the ones where it is visible.
                    if commands.iter().any(|member| *member != commands[0]) {
                        ordered += 1;
                    }
                }
            }
            (Command::Batch { .. }, _) => panic!("the inverse of a batch is not a batch"),
            _ => single += 1,
        }
    }
    support::covered(
        "REQ-CMD.batch_reverses",
        &[
            ("a single command", single),
            ("a batch", batch),
            ("a batch of more than one command", long),
            ("a batch whose members are not all alike", ordered),
        ],
    );
}
