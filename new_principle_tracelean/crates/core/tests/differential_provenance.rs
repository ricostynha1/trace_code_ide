mod harness;

use std::collections::BTreeMap;

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

fn offset() -> Schema {
    Schema::Nat { max: Some(3), edges: vec![0] }
}

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
        "renameFile".into(),
        Some(Box::new(strukt(&[("from_", path_name()), ("to", path_name())]))),
    );
    Schema::Enum { variants }
}

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

fn movement() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert("undo".into(), None);
    variants.insert("redo".into(), None);
    variants.insert(
        "jump".into(),
        Some(Box::new(strukt(&[("node", Schema::Nat { max: Some(4), edges: vec![0] })]))),
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
        ("script", Schema::List { inner: Box::new(step()), max_len: Some(8) }),
        ("file", path_name()),
        ("position", Schema::Nat { max: Some(5), edges: vec![0] }),
    ])
}

/// @drt REQ-PROV.position_question
/// @tests REQ-PROV.position_question
/// @tests REQ-PROV.mapping_before
/// @tests REQ-PROV.mapping_after
/// @tests REQ-PROV.mapping_inside
/// @tests REQ-PROV.exact_not_heuristic
/// @tests REQ-PROV.base_is_honest
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_where_text_came_from() {
    let scratch = harness::scratch("provenance");
    let op = "REQ-PROV.position_question";
    let implementation = harness::rust_runner(
        "REQ-PROV",
        "position_question",
        "crates/core/src/history/provenance.rs::origins",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Provenance",
        "TraceLean.Provenance.origins",
        op,
        &["base", "script", "file", "position"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 88, cases: 3_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_both_a_written_position_and_an_inherited_one() {
    use tracelean_core::drt::gen;
    use tracelean_core::history::command::Workspace;
    use tracelean_core::history::provenance::{origins, Origin};
    use tracelean_core::history::tree::Step;

    let schema = input_schema();
    let mut rng = gen::Rng::new(88);
    let (mut written, mut inherited) = (0, 0);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let base: Workspace = serde_json::from_value(v["base"].clone()).unwrap();
        let script: Vec<Step> = serde_json::from_value(v["script"].clone()).unwrap();
        let answers = origins(
            base,
            script,
            v["file"].as_str().unwrap().to_string(),
            v["position"].as_u64().unwrap() as usize,
        );
        for answer in answers {
            match answer {
                Origin::Node { .. } => written += 1,
                Origin::Base => inherited += 1,
            }
        }
    }
    support::covered(
        "REQ-PROV.position_question",
        &[("attributed to a recorded edit", written), ("attributed to the base", inherited)],
    );
}
