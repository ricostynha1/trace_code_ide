//! The undo tree, checked against its model.
//!
//! Two definitions of one thing: a node's state is *defined* by replaying its
//! ancestry from the base, and `jump_to` computes it by travelling via the
//! nearest common ancestor. The report puts both answers next to each other for
//! every node, so the run checks the model against the implementation and the
//! two definitions against each other at the same time.
//!
//! Scripts rather than single calls, because the property is about a history:
//! the interesting states are reached by undoing, branching, jumping between
//! branches and redoing, and no single operation produces one.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

// A one-letter alphabet. Paths drawn from a wide space would almost never
// collide, so almost every command would be refused for naming a file that does
// not exist, and the histories would all be empty.
fn path_name() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["a".into(), "b".into()] }
}

fn text() -> Schema {
    Schema::Str { max_len: Some(3), examples: vec!["".into(), "hi".into()] }
}

fn command() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "insert".into(),
        Some(Box::new(strukt(&[
            ("file", path_name()),
            ("offset", Schema::Nat { max: Some(1), edges: vec![0] }),
            ("text", text()),
        ]))),
    );
    variants.insert("createFile".into(), Some(Box::new(strukt(&[("path", path_name())]))));
    variants.insert(
        "deleteFile".into(),
        Some(Box::new(strukt(&[("path", path_name()), ("content", text())]))),
    );
    // `delete` and `renameFile` are left out deliberately. Both need a witness
    // or a free name to match, so almost every one is refused — and a refused
    // command creates no node, which is the one thing this test needs its
    // commands to do. What they do is checked by the command suite instead.
    Schema::Enum { variants }
}

fn movement() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert("undo".into(), None);
    variants.insert("redo".into(), None);
    // Small identifiers, so jumps land on nodes the script actually created —
    // and occasionally on one it did not, which must also be handled.
    variants.insert(
        "jump".into(),
        Some(Box::new(strukt(&[("node", Schema::Nat { max: Some(5), edges: vec![0] })]))),
    );
    Schema::Enum { variants }
}

fn step() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert("push".into(), Some(Box::new(strukt(&[("command", command())]))));
    variants.insert("move".into(), Some(Box::new(strukt(&[("movement", movement())]))));
    Schema::Enum { variants }
}

fn input_schema() -> Schema {
    strukt(&[
        // Two files, always. A base that was often empty would make almost
        // every command refuse for naming a file that does not exist, and the
        // histories this is about — branches, jumps between them — would never
        // be built.
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
        ("script", Schema::List { inner: Box::new(step()), max_len: Some(10) }),
    ])
}

/// @drt REQ-UNDO.jump_equivalence
/// @tests REQ-UNDO.jump_equivalence
/// @tests REQ-UNDO.path_via_ancestor
/// @tests REQ-UNDO.reachable
/// @tests REQ-UNDO.no_loss_on_branch
/// @tests REQ-UNDO.preview_is_pure
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_history() {
    let scratch = harness::scratch("tree");
    let op = "REQ-UNDO.jump_equivalence";
    let implementation = harness::rust_runner(
        "REQ-UNDO",
        "jump_equivalence",
        "crates/core/src/history/tree.rs::run_script",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Tree",
        "TraceLean.Tree.runScript",
        op,
        &["base", "script"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 77, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The law itself, asserted against the implementation rather than only
/// through the model: travelling and replaying reach the same state, for every
/// node of every generated history.
///
/// Also checks that the generated histories are worth running — branches, not
/// just straight lines.
///
/// @tests REQ-UNDO.jump_equivalence
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn travelling_and_replaying_agree_on_every_generated_history() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::Workspace;
    use tracelean_core::history::tree::{run_script, Step};

    let schema = input_schema();
    let mut rng = gen::Rng::new(77);
    let (mut branched, mut nonempty) = (0, 0);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let base: Workspace = serde_json::from_value(v["base"].clone()).unwrap();
        let script: Vec<Step> = serde_json::from_value(v["script"].clone()).unwrap();

        let report = run_script(base, script);
        assert_eq!(
            report.travelled, report.replayed,
            "travelling and replaying disagree for nodes {:?}",
            report.nodes
        );
        if report.nodes.len() > 1 {
            nonempty += 1;
        }
        // A branch exists when some node has more than one child, which shows
        // up as two nodes replaying to different states from a shared prefix.
        if report.nodes.len() > 2 {
            branched += 1;
        }
    }
    support::covered(
        "REQ-UNDO.jump_equivalence",
        &[
            ("a history with more than one node", nonempty),
            ("a history deep enough to branch", branched),
        ],
    );
}

