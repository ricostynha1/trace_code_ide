//! Mirroring, checked against the model.
//!
//! The law is that applying the derived mutations to the observed-from tree
//! yields the observed tree. Generate pairs of trees, derive, apply, compare.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

const OP: &str = "REQ-MIRROR.apply_reproduces";

fn path_name() -> Schema {
    Schema::Str {
        max_len: None,
        examples: vec![
            "a.rs".into(),
            "b.rs".into(),
            "c.rs".into(),
            // Paths that must be excluded, so the run exercises the exclusion
            // rather than only the ordinary case.
            ".git/HEAD".into(),
            ".tracelean/trace.lock.json".into(),
            "target/debug/x".into(),
        ],
    }
}

fn workspace() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "files".to_string(),
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![
                    path_name(),
                    Schema::Str { max_len: Some(4), examples: vec!["".into(), "hi".into()] },
                ],
            }),
            max_len: Some(4),
        },
    );
    Schema::Struct { fields }
}

fn input_schema() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("before".to_string(), workspace());
    fields.insert("after".to_string(), workspace());
    Schema::Struct { fields }
}

/// @drt REQ-MIRROR.apply_reproduces
/// @tests REQ-MIRROR.apply_reproduces
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_mirroring() {
    let scratch = harness::scratch("mirror");
    let implementation = harness::rust_runner(
        "REQ-MIRROR",
        "apply_reproduces",
        "crates/core/src/observe/mirror.rs::mirrored",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Mirror",
        "TraceLean.Mirror.mirrored",
        OP,
        &["before", "after"],
        &scratch,
    );

    let result = run(
        OP,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 17, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The law must be exercised where it has content: runs where the two trees
/// are equal would agree without testing anything.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_real_differences_and_excluded_paths() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::Workspace;
    use tracelean_core::observe::mirror::mutations;

    let schema = input_schema();
    let mut rng = gen::Rng::new(17);
    let (mut differing, mut excluded) = (0, 0);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let before: Workspace = serde_json::from_value(v["before"].clone()).unwrap();
        let after: Workspace = serde_json::from_value(v["after"].clone()).unwrap();
        if before != after {
            differing += 1;
        }
        let touched_protected = before
            .files
            .keys()
            .chain(after.files.keys())
            .any(|p| !tracelean_core::observe::is_mirrored(p));
        if touched_protected && !mutations(before, after).is_empty() {
            excluded += 1;
        }
    }
    support::covered(
        "REQ-MIRROR.apply_reproduces",
        &[("the two trees differed", differing), ("the exclusion rule applied", excluded)],
    );
}

/// The mutation list itself, not only the state it reaches.
///
/// Agreement on `mirrored` is weaker than it looks: two different mutation
/// lists can drive the same tree to the same place, so a model that ordered
/// them differently, emitted a redundant one, or leaked a protected path would
/// still agree. Comparing the list is what makes `minimal`, `ordering_defined`
/// and `protected_excluded` checked rather than asserted.
///
/// @drt REQ-MIRROR.diff_is_pure
/// @tests REQ-MIRROR.diff_is_pure
/// @tests REQ-MIRROR.minimal
/// @tests REQ-MIRROR.ordering_defined
/// @tests REQ-MIRROR.protected_excluded
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_mutations_themselves() {
    let scratch = harness::scratch("mutations");
    let op = "REQ-MIRROR.diff_is_pure";
    let implementation = harness::rust_runner(
        "REQ-MIRROR",
        "diff_is_pure",
        "crates/core/src/observe/mirror.rs::mutations",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Mirror",
        "TraceLean.Mirror.mutationsOf",
        op,
        &["before", "after"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 64, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What the mirror does about a file the editor cannot represent.
///
/// Three states, not two: absent, text, and present-but-opaque. Collapsing the
/// third into either of the others is how a mirror either loses a change or
/// writes a binary's bytes into a text buffer and corrupts it on the way back.
///
/// @drt REQ-MIRROR.binary_handled
/// @tests REQ-MIRROR.binary_handled
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_happens_to_an_opaque_file() {
    let scratch = harness::scratch("opaque");
    let op = "REQ-MIRROR.binary_handled";
    let implementation = harness::rust_runner(
        "REQ-MIRROR",
        "binary_handled",
        "crates/core/src/observe/mirror.rs::change_at",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Mirror",
        "TraceLean.Mirror.changeAt",
        op,
        &["path", "before", "after"],
        &scratch,
    );

    let mut state: std::collections::BTreeMap<String, Option<Box<Schema>>> =
        std::collections::BTreeMap::new();
    state.insert("absent".into(), None);
    // With the hash its bytes are known by, two of them, so an unchanged
    // binary and a changed one both occur. It was once drawn without one,
    // which neither side could read — and two sides failing alike agree, so
    // no opaque file was ever compared (found by measuring lines).
    state.insert(
        "opaque".into(),
        Some(Box::new(Schema::Struct {
            fields: [(
                "hash".to_string(),
                Schema::Str { max_len: Some(0), examples: vec!["h1".into(), "h2".into()] },
            )]
            .into_iter()
            .collect(),
        })),
    );
    state.insert(
        "text".into(),
        Some(Box::new(Schema::Struct {
            fields: [(
                "content".to_string(),
                Schema::Str {
                    max_len: Some(1),
                    examples: vec!["".into(), "one".into(), "two".into()],
                },
            )]
            .into_iter()
            .collect(),
        })),
    );

    let schema = Schema::Struct {
        fields: [
            (
                "path".to_string(),
                Schema::Str {
                    max_len: Some(0),
                    examples: vec![
                        "src/a.rs".into(),
                        "logo.png".into(),
                        ".git/index".into(),
                        "target/out".into(),
                        "".into(),
                    ],
                },
            ),
            ("before".to_string(), Schema::Enum { variants: state.clone() }),
            ("after".to_string(), Schema::Enum { variants: state }),
        ]
        .into_iter()
        .collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 131, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What a sandbox offers as the agent's: a start known or not, paths the agent
/// left as they started (their hash in the start) or changed, and a project
/// that moved on meanwhile.
///
/// @drt REQ-OBS.only_what_the_tool_changed
/// @tests REQ-OBS.only_what_the_tool_changed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_the_agent_changed() {
    let op = "REQ-OBS.only_what_the_tool_changed";
    let scratch = harness::scratch("mirror-changed-since");
    let implementation = harness::rust_runner(
        "REQ-OBS",
        "only_what_the_tool_changed",
        "crates/core/src/observe/mirror.rs::changed_since_owned",
        &scratch,
    );
    let model = harness::lean_runner("TraceLean.Mirror", "TraceLean.Mirror.changedSince", op, &["base", "project", "agent"], &scratch);
    // A start lists hashes of the same few texts the trees hold, so a path
    // the agent left alone matches it and one it changed does not.
    let hashes = ["", "hi"].iter().map(|t| tracelean_core::trace::hash::text(t)).chain(["0".to_string()]).collect();
    let base = Schema::Option {
        inner: Box::new(Schema::List {
            inner: Box::new(Schema::Tuple { items: vec![path_name(), Schema::Str { max_len: Some(0), examples: hashes }] }),
            max_len: Some(4),
        }),
    };
    let schema = Schema::Struct {
        fields: [("base".to_string(), base), ("project".to_string(), workspace()), ("agent".to_string(), workspace())]
            .into_iter()
            .collect(),
    };
    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 211, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
