//! The refinement graph, checked against the model.
//!
//! Two questions that must not be confused. A parent that does not exist is a
//! typo; a cycle is a contradiction in what the project claims. Report the
//! first as the second and a spelling mistake fails the build; report the
//! second as the first and a requirement is allowed to justify itself.
//!
//! The cycle returned is also part of the answer, not just its existence: it is
//! shown to a person, so it has to be the same cycle on every run. Generated
//! graphs with several cycles are what check that.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// A four-identifier alphabet, one of which nothing ever declares. Small
/// enough that cycles occur by chance rather than by construction.
fn id() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec!["A".into(), "B".into(), "C".into(), "GONE".into()],
    }
}

fn declared() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["A".into(), "B".into(), "C".into()] }
}

fn input_schema() -> Schema {
    strukt(&[(
        "edges",
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![
                    declared(),
                    Schema::List { inner: Box::new(id()), max_len: Some(2) },
                ],
            }),
            max_len: Some(4),
        },
    )])
}

/// @drt REQ-REQDOC.refines_dag
/// @tests REQ-REQDOC.refines_dag
/// @tests REQ-REQDOC.refines_resolves
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_refinement_graph() {
    let scratch = harness::scratch("refinement");
    let op = "REQ-REQDOC.refines_dag";
    let implementation = harness::rust_runner(
        "REQ-REQDOC",
        "refines_dag",
        "crates/core/src/trace/requirement.rs::graph_report",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Refinement",
        "TraceLean.Refinement.graphReport",
        op,
        &["edges"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 71, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    // The agreeing half of what this binding needs; the coverage half comes
    // from the generation test, and whichever lands second writes the record.
    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Cycles, dangling parents and clean graphs must all occur — and a reported
/// cycle must really be one.
///
/// @tests REQ-REQDOC.refines_dag
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_cycles_dangling_parents_and_clean_graphs() {
    use std::collections::BTreeMap;
    use tracelean_core::drt::gen;
    use tracelean_core::trace::requirement::graph_report;

    let schema = input_schema();
    let mut rng = gen::Rng::new(71);
    let (mut cyclic, mut dangling, mut clean) = (0, 0, 0);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let edges: Vec<(String, Vec<String>)> = serde_json::from_value(v["edges"].clone()).unwrap();
        let graph: BTreeMap<String, Vec<String>> = edges.clone().into_iter().collect();

        let report = graph_report(edges);
        match (&report.cycle, report.dangling.is_empty()) {
            (Some(cycle), _) => {
                cyclic += 1;
                // Every step of a reported cycle must be a real edge, and it
                // must close.
                assert!(cycle.len() >= 2, "a cycle of one node: {cycle:?}");
                assert_eq!(cycle.first(), cycle.last(), "the reported cycle does not close");
                for pair in cycle.windows(2) {
                    assert!(
                        graph.get(&pair[0]).is_some_and(|p| p.contains(&pair[1])),
                        "{} -> {} is not an edge",
                        pair[0],
                        pair[1]
                    );
                }
            }
            (None, false) => dangling += 1,
            (None, true) => clean += 1,
        }
        for (id, parent) in &report.dangling {
            assert!(!graph.contains_key(parent), "{parent} is declared, so it is not dangling");
            assert!(graph.contains_key(id), "{id} is not a declared requirement");
        }
    }
    support::covered(
        "REQ-REQDOC.refines_dag",
        &[
            ("a graph with a cycle", cyclic),
            ("a graph with a dangling parent", dangling),
            ("a clean graph", clean),
        ],
    );
}
