mod harness;

use harness::strukt;

use tracelean_core::drt::schema::Schema;

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
    let op = "REQ-REQDOC.refines_dag";

    harness::agree(op, &input_schema(), 71, 3_000);
}

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
    harness::covered(
        "REQ-REQDOC.refines_dag",
        &[
            ("a graph with a cycle", cyclic),
            ("a graph with a dangling parent", dangling),
            ("a clean graph", clean),
        ],
    );
}
