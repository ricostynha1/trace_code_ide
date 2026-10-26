mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

const OP: &str = "REQ-MIRROR.apply_reproduces";

fn path_name() -> Schema {
    Schema::Str {
        max_len: None,
        examples: vec![
            "a.rs".into(),
            "b.rs".into(),
            "c.rs".into(),
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

    harness::agree(OP, &input_schema(), 17, 2_000);
}

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
    harness::covered(
        "REQ-MIRROR.apply_reproduces",
        &[("the two trees differed", differing), ("the exclusion rule applied", excluded)],
    );
}

/// @drt REQ-MIRROR.diff_is_pure
/// @tests REQ-MIRROR.diff_is_pure
/// @tests REQ-MIRROR.minimal
/// @tests REQ-MIRROR.ordering_defined
/// @tests REQ-MIRROR.protected_excluded
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_mutations_themselves() {
    let op = "REQ-MIRROR.diff_is_pure";

    harness::agree(op, &input_schema(), 64, 2_000);
}

/// @drt REQ-MIRROR.binary_handled
/// @tests REQ-MIRROR.binary_handled
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_happens_to_an_opaque_file() {
    let op = "REQ-MIRROR.binary_handled";

    let mut state: std::collections::BTreeMap<String, Option<Box<Schema>>> =
        std::collections::BTreeMap::new();
    state.insert("absent".into(), None);
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

    harness::agree(op, &schema, 131, 2_000);
}

/// @drt REQ-OBS.only_what_the_tool_changed
/// @tests REQ-OBS.only_what_the_tool_changed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_the_agent_changed() {
    let op = "REQ-OBS.only_what_the_tool_changed";
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
    harness::agree(op, &schema, 211, 2_000);
}
