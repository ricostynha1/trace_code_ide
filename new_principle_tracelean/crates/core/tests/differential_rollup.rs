mod harness;

use std::collections::{BTreeMap, BTreeSet};

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

const OP: &str = "REQ-ROLLUP.every_part_holds";

fn input_schema() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "parts".to_string(),
        Schema::List { inner: Box::new(harness::marks()), max_len: Some(5) },
    );
    Schema::Struct { fields }
}

/// @drt REQ-ROLLUP.every_part_holds
/// @tests REQ-ROLLUP.every_part_holds
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_aggregation() {
    let scratch = harness::scratch("rollup");
    let implementation = harness::rust_runner(
        "REQ-ROLLUP",
        "every_part_holds",
        "crates/core/src/trace/rollup.rs::combine",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Rollup", "TraceLean.Rollup.combine", OP, &["parts"], &scratch);

    let result = run(
        OP,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 37, cases: 2_000 },
    )
    .expect("both runners answer");

    harness::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests ARCH-HONEST.weakest_link
#[test]
fn a_mark_is_held_only_when_every_part_holds_it() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Marks;
    use tracelean_core::trace::rollup::combine;

    let schema = input_schema();
    let mut rng = gen::Rng::new(37);
    let (mut mixed, mut uniform, mut empty, mut beside) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let parts: Vec<Marks> = serde_json::from_value(v["parts"].clone()).unwrap();
        let result = combine(parts.clone());
        if parts.is_empty() {
            assert_eq!(result, Marks::NONE, "nothing to aggregate holds no mark");
            empty += 1;
            continue;
        }
        assert_eq!(result.structural, parts.iter().all(|p| p.structural), "{result:?} from {parts:?}");
        for (i, &(mark, held, _)) in result.each().iter().enumerate() {
            let every = parts.iter().filter(|p| p.applies(mark)).all(|p| p.each()[i].1);
            assert_eq!(held, result.applies(mark) && every, "{result:?} from {parts:?}");
        }
        if parts.iter().any(|p| *p != parts[0]) {
            mixed += 1;
        } else {
            uniform += 1;
        }
        if parts.iter().any(|p| p.structural) && parts.iter().any(|p| !p.structural) {
            beside += 1;
        }
    }
    harness::covered(
        OP,
        &[
            ("parts that do not all hold the same marks", mixed),
            ("every part the same", uniform),
            ("nothing to aggregate", empty),
            ("a structural part beside one with a model", beside),
        ],
    );
}

fn strukt2(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn req_id() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec!["A".into(), "B".into(), "C".into(), "D".into()],
    }
}

fn clause_key() -> Schema {
    Schema::Option {
        inner: Box::new(Schema::Str {
            max_len: Some(0),
            examples: vec!["one".into(), "two".into()],
        }),
    }
}

fn graph_input() -> Schema {
    let node = strukt2(&[
        ("id", req_id()),
        ("complete", Schema::Bool),
        ("clauses", Schema::List { inner: Box::new(clause_key()), max_len: Some(2) }),
        ("exempt", Schema::List { inner: Box::new(clause_key()), max_len: Some(1) }),
        ("partialClauses", Schema::List { inner: Box::new(clause_key()), max_len: Some(1) }),
        ("refines", Schema::List { inner: Box::new(req_id()), max_len: Some(2) }),
    ]);
    strukt2(&[
        ("nodes", Schema::List { inner: Box::new(node), max_len: Some(4) }),
        (
            "held",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![
                        Schema::Tuple { items: vec![req_id(), clause_key()] },
                        harness::marks(),
                    ],
                }),
                max_len: Some(4),
            },
        ),
        ("root", req_id()),
        ("floor", harness::marks()),
    ])
}

/// @drt REQ-ROLLUP.open_is_lower_bound
/// @tests REQ-ROLLUP.open_is_lower_bound
/// @tests REQ-ROLLUP.never_complete_when_open
/// @tests REQ-ROLLUP.exempt_leaves_denominator
/// @tests REQ-ROLLUP.partial_capped
/// @tests REQ-ROLLUP.deterministic_order
/// @tests REQ-ROLLUP.counted_once
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_rolling_a_graph_up() {
    let op = "REQ-ROLLUP.open_is_lower_bound";

    harness::agree(op, &graph_input(), 91, 10_000);
}

fn figure_input() -> Schema {
    let count = Schema::Nat { max: Some(12), edges: vec![0, 1, 2, 3] };
    strukt2(&[(
        "figure",
        strukt2(&[("met", count.clone()), ("total", count), ("exact", Schema::Bool)]),
    )])
}

/// @drt ARCH-HONEST.lower_bound_marked
/// @tests ARCH-HONEST.lower_bound_marked
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_writing_a_figure() {
    let scratch = harness::scratch("render-figure");
    let op = "ARCH-HONEST.lower_bound_marked";
    let implementation = harness::rust_runner(
        "ARCH-HONEST",
        "lower_bound_marked",
        "crates/core/src/trace/rollup.rs::render_figure",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Rollup", "TraceLean.Rollup.render", op, &["figure"], &scratch);
    let result = run(
        op,
        &figure_input(),
        &model,
        &implementation,
        RunOptions { seed: 53, cases: 1_000 },
    )
    .expect("both runners answer");
    harness::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests ARCH-HONEST.lower_bound_marked
#[test]
fn an_open_figure_is_always_marked() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::rollup::{render_figure as render, Figure};

    let mut rng = gen::Rng::new(53);
    let (mut open, mut exact, mut empty) = (0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&figure_input(), &mut rng);
        let figure: Figure = serde_json::from_value(v["figure"].clone()).unwrap();
        let shown = render(figure);
        assert_eq!(shown.contains("provisional"), !figure.exact, "{figure:?} written as {shown}");
        if figure.total == 0 {
            empty += 1;
        }
        if figure.exact {
            exact += 1;
        } else {
            assert!(shown.starts_with('≤'), "an open figure does not say it can only fall: {shown}");
            open += 1;
        }
    }
    harness::covered(
        "ARCH-HONEST.lower_bound_marked",
        &[("an open figure", open), ("an exact figure", exact), ("nothing counted", empty)],
    );
}

/// @tests REQ-ROLLUP.never_complete_when_open
/// @tests REQ-ROLLUP.exempt_leaves_denominator
/// @tests REQ-ROLLUP.partial_capped
/// @tests REQ-ROLLUP.counted_once
/// @tests REQ-ROLLUP.deterministic_order
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_roll_up_is_a_function_of_the_graph_and_never_flatters_it() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Marks;
    use tracelean_core::trace::rollup::{roll_up, Node};

    let schema = graph_input();
    let mut rng = gen::Rng::new(91);
    let (mut inexact, mut exempted, mut partial, mut deep, mut diamond, mut cycle) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..10_000 {
        let v = gen::value(&schema, &mut rng);
        let nodes: Vec<Node> = serde_json::from_value(v["nodes"].clone()).unwrap();
        let held: Vec<((String, Option<String>), Marks)> =
            serde_json::from_value(v["held"].clone()).unwrap();
        let root = v["root"].as_str().unwrap().to_string();
        let floor: Marks = serde_json::from_value(v["floor"].clone()).unwrap();

        let rolled = roll_up(nodes.clone(), held.clone(), root.clone(), floor);

        let unique: BTreeSet<&String> = nodes.iter().map(|n| &n.id).collect();
        if unique.len() == nodes.len() {
            let mut reversed = nodes.clone();
            reversed.reverse();
            assert_eq!(
                rolled,
                roll_up(reversed, held.clone(), root.clone(), floor),
                "the roll-up depends on the order nodes arrive in"
            );
        }

        if !rolled.covered.exact {
            assert!(!rolled.covered.is_complete(), "an unclaimed decomposition read as finished");
            inexact += 1;
        }

        let graph: BTreeMap<String, Node> =
            nodes.iter().map(|n| (n.id.clone(), n.clone())).collect();
        let (reached, paths) = by_paths(&graph, &root);
        let kept: usize = reached
            .iter()
            .filter_map(|id| graph.get(id))
            .map(|n| n.clauses.iter().filter(|c| !n.exempt.contains(c)).count())
            .sum();
        assert_eq!(
            rolled.covered.total as usize, kept,
            "the denominator is not the unexempted clauses of the reachable set, each once"
        );
        if let Some(node) = graph.get(&rolled.id) {
            if node.clauses.iter().any(|c| node.exempt.contains(c)) {
                exempted += 1;
            }
            if node.clauses.iter().any(|c| node.partial_clauses.contains(c) && !node.exempt.contains(c)) {
                partial += 1;
                assert!(rolled.covered.met < rolled.covered.total || !(floor.tested || floor.proved));
            }
        }
        if paths.values().any(|n| *n > 1) {
            diamond += 1;
        }
        if reached.len() > 1 && cyclic(&graph, &root) {
            cycle += 1;
        }
        if !rolled.children.is_empty() {
            deep += 1;
        }
    }
    harness::covered(
        "REQ-ROLLUP.open_is_lower_bound",
        &[
            ("a roll-up that is a lower bound", inexact),
            ("a root with an exempted clause", exempted),
            ("a root with a partial clause", partial),
            ("a roll-up with children", deep),
            ("a diamond", diamond),
            ("a cycle", cycle),
        ],
    );
}

fn by_paths(
    graph: &BTreeMap<String, tracelean_core::trace::rollup::Node>,
    root: &str,
) -> (BTreeSet<String>, BTreeMap<String, usize>) {
    fn go(
        graph: &BTreeMap<String, tracelean_core::trace::rollup::Node>,
        at: &str,
        path: &mut Vec<String>,
        paths: &mut BTreeMap<String, usize>,
    ) {
        *paths.entry(at.to_string()).or_default() += 1;
        path.push(at.to_string());
        for (child, n) in graph {
            if n.refines.iter().any(|p| p == at) && !path.contains(child) {
                go(graph, child, path, paths);
            }
        }
        path.pop();
    }
    let mut paths = BTreeMap::new();
    go(graph, root, &mut Vec::new(), &mut paths);
    (paths.keys().cloned().collect(), paths)
}

fn cyclic(graph: &BTreeMap<String, tracelean_core::trace::rollup::Node>, root: &str) -> bool {
    let (reached, _) = by_paths(graph, root);
    reached.iter().any(|id| {
        let (below, _) = by_paths(graph, id);
        below.iter().any(|d| d != id && graph.get(d).is_some_and(|n| n.refines.contains(id)))
    })
}

fn file_coverage_input() -> Schema {
    let file = Schema::Str {
        max_len: Some(0),
        examples: vec!["a.rs".into(), "b.rs".into(), "c.lean".into(), "gone.rs".into()],
    };
    Schema::Struct {
        fields: [
            (
                "scanned".to_string(),
                Schema::List { inner: Box::new(file.clone()), max_len: Some(4) },
            ),
            ("claimed".to_string(), Schema::List { inner: Box::new(file), max_len: Some(4) }),
        ]
        .into_iter()
        .collect(),
    }
}

/// @drt REQ-ROLLUP.untraced_counted
/// @tests REQ-ROLLUP.untraced_counted
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_coverage_over_files() {
    let op = "REQ-ROLLUP.untraced_counted";

    harness::agree(op, &file_coverage_input(), 127, 2_000);
}

/// @tests REQ-ROLLUP.untraced_counted
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn a_claim_about_an_unscanned_file_never_raises_coverage() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::rollup::file_coverage;

    let schema = file_coverage_input();
    let mut rng = gen::Rng::new(127);
    let (mut phantom, mut untraced, mut all_claimed, mut nothing) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let scanned: Vec<String> = serde_json::from_value(v["scanned"].clone()).unwrap();
        let claimed: Vec<String> = serde_json::from_value(v["claimed"].clone()).unwrap();

        let figure = file_coverage(scanned.clone(), claimed.clone());
        assert!(figure.met <= figure.total, "more files claimed than were scanned");
        assert!(figure.exact, "the denominator is the set of files scanned, which is known");

        let mut inflated = claimed.clone();
        inflated.push("never-scanned.rs".to_string());
        assert_eq!(
            file_coverage(scanned.clone(), inflated),
            figure,
            "a claim about an unscanned file changed the figure"
        );

        let scanned_set: std::collections::BTreeSet<&String> = scanned.iter().collect();
        if claimed.iter().any(|file| !scanned_set.contains(file)) {
            phantom += 1;
        }
        if scanned.is_empty() {
            nothing += 1;
        } else if figure.met == figure.total {
            all_claimed += 1;
        } else {
            untraced += 1;
        }
    }
    harness::covered(
        "REQ-ROLLUP.untraced_counted",
        &[
            ("a claim about a file nothing scanned", phantom),
            ("a scanned file nothing claims", untraced),
            ("every scanned file claimed", all_claimed),
            ("nothing scanned at all", nothing),
        ],
    );
}
