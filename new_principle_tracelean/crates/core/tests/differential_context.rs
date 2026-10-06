//! An agent's context, checked against the model: the neighbourhood of a
//! requirement in the refinement graph, which parts a person's choice puts in
//! the copied text, and which part a label names.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::surface::context::{ancestors, descendants, part_named, parts_shown, Node, Part, ALL_PARTS};

mod support;

/// Four names, so chains, shared parents, cycles and a name declared twice
/// all occur often enough to be tested rather than hoped for.
fn name() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["A".into(), "B".into(), "C".into(), "D".into()] }
}

fn graph() -> BTreeMap<String, Schema> {
    let mut node = BTreeMap::new();
    node.insert("id".to_string(), name());
    node.insert("refines".to_string(), Schema::List { inner: Box::new(name()), max_len: Some(2) });
    let mut fields = BTreeMap::new();
    fields.insert("nodes".to_string(), Schema::List { inner: Box::new(Schema::Struct { fields: node }), max_len: Some(4) });
    fields.insert("id".to_string(), name());
    fields
}

fn parts() -> Schema {
    let names = ["requirement", "refines", "refinedBy", "code", "tests", "models", "affected"];
    Schema::List { inner: Box::new(Schema::simple_enum(&names)), max_len: Some(5) }
}

fn choice() -> BTreeMap<String, Schema> {
    let mut fields = BTreeMap::new();
    fields.insert("included".to_string(), parts());
    fields.insert("filled".to_string(), parts());
    fields
}

fn label() -> BTreeMap<String, Schema> {
    let mut examples: Vec<String> = ALL_PARTS.iter().map(|p| p.label().to_string()).collect();
    examples.extend(["refinedBy".to_string(), "Code".to_string(), "everything".to_string(), String::new()]);
    let mut fields = BTreeMap::new();
    fields.insert("name".to_string(), Schema::Str { max_len: Some(0), examples });
    fields
}

#[allow(clippy::too_many_arguments)]
fn check(req: &str, op: &str, function: &str, entry: &str, arguments: &[&str], fields: BTreeMap<String, Schema>, seed: u64) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("context-{clause}-{seed}"));
    let implementation = harness::rust_runner(req, clause, entry, &scratch);
    let model = harness::lean_runner("TraceLean.Context", function, op, arguments, &scratch);
    let result = run(
        op,
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-CONTEXT.neighbourhood_is_closed
/// @tests REQ-CONTEXT.neighbourhood_is_closed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_requirement_refines() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.neighbourhood_is_closed",
        "TraceLean.Context.ancestors",
        "crates/core/src/surface/context.rs::ancestors",
        &["nodes", "id"],
        graph(),
        97,
    );
}

/// @drt REQ-CONTEXT.neighbourhood_is_closed
/// @tests REQ-CONTEXT.neighbourhood_is_closed
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_refines_a_requirement() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.neighbourhood_is_closed",
        "TraceLean.Context.descendants",
        "crates/core/src/surface/context.rs::descendants",
        &["nodes", "id"],
        graph(),
        98,
    );
}

/// @drt REQ-CONTEXT.person_chooses
/// @tests REQ-CONTEXT.person_chooses
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_copied() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.person_chooses",
        "TraceLean.Context.partsShown",
        "crates/core/src/surface/context.rs::parts_shown",
        &["included", "filled"],
        choice(),
        99,
    );
}

/// @drt REQ-CONTEXT.part_named
/// @tests REQ-CONTEXT.part_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_label_names() {
    check(
        "REQ-CONTEXT",
        "REQ-CONTEXT.part_named",
        "TraceLean.Context.partNamed",
        "crates/core/src/surface/context.rs::part_named",
        &["name"],
        label(),
        100,
    );
}

/// The generators reach what the clauses are about.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_chains_cycles_and_every_kind_of_choice() {
    let mut rng = gen::Rng::new(97);
    let (mut none, mut deep, mut cycle, mut twice) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: graph() }, &mut rng);
        let nodes: Vec<Node> = serde_json::from_value(value["nodes"].clone()).expect("nodes");
        let id = value["id"].as_str().expect("an id").to_string();
        let up = ancestors(nodes.clone(), id.clone());
        let down = descendants(nodes.clone(), id.clone());
        if up.is_empty() {
            none += 1;
        }
        let direct: Vec<String> = nodes.iter().filter(|n| n.id == id).flat_map(|n| n.refines.clone()).collect();
        if up.iter().any(|a| !direct.contains(a)) {
            deep += 1;
        }
        if up.iter().any(|a| down.contains(a)) || direct.contains(&id) {
            cycle += 1;
        }
        if nodes.iter().filter(|n| n.id == id).count() > 1 {
            twice += 1;
        }
    }
    support::covered(
        "REQ-CONTEXT.neighbourhood_is_closed",
        &[
            ("nothing above", none),
            ("two or more steps up", deep),
            ("a cycle back to the target", cycle),
            ("a name declared twice", twice),
        ],
    );

    let mut rng = gen::Rng::new(99);
    let (mut nothing, mut empty, mut disorder, mut repeated) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: choice() }, &mut rng);
        let included: Vec<Part> = serde_json::from_value(value["included"].clone()).expect("parts");
        let filled: Vec<Part> = serde_json::from_value(value["filled"].clone()).expect("parts");
        if included.is_empty() {
            nothing += 1;
        }
        if included.iter().any(|p| !filled.contains(p)) {
            empty += 1;
        }
        if included.windows(2).any(|w| w[0] > w[1]) {
            disorder += 1;
        }
        if included.iter().enumerate().any(|(n, p)| included[..n].contains(p)) {
            repeated += 1;
        }
        let shown = parts_shown(included.clone(), filled.clone());
        assert!(shown.iter().all(|p| included.contains(p) && filled.contains(p)));
    }
    support::covered(
        "REQ-CONTEXT.person_chooses",
        &[
            ("nothing chosen", nothing),
            ("a part chosen with nothing in it", empty),
            ("chosen out of order", disorder),
            ("a part chosen twice", repeated),
        ],
    );

    let mut rng = gen::Rng::new(100);
    let (mut named, mut unnamed) = (0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: label() }, &mut rng);
        match part_named(value["name"].as_str().expect("a name").to_string()) {
            Some(_) => named += 1,
            None => unnamed += 1,
        }
    }
    support::covered(
        "REQ-CONTEXT.part_named",
        &[("a label that names a part", named), ("a name that is no part", unnamed)],
    );
}
