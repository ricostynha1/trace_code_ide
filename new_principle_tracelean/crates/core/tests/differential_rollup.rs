//! Roll-up, checked against the model.
//!
//! The rule is that an aggregate is the minimum and never an average. A mean
//! would report a level nothing established, and this is the one output people
//! quote without reading what produced it.

mod harness;
mod support;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

const OP: &str = "REQ-ROLLUP.min_not_mean";

fn input_schema() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for name in ["L1", "L2", "L3", "L4"] {
        variants.insert(name.to_string(), None);
    }
    let mut fields = BTreeMap::new();
    fields.insert(
        "levels".to_string(),
        Schema::List { inner: Box::new(Schema::Enum { variants }), max_len: Some(5) },
    );
    Schema::Struct { fields }
}

/// @drt REQ-ROLLUP.min_not_mean
/// @tests REQ-ROLLUP.min_not_mean
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_aggregation() {
    let scratch = harness::scratch("rollup");
    let implementation = harness::rust_runner(
        "REQ-ROLLUP",
        "min_not_mean",
        "crates/core/src/trace/rollup.rs::combine",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Rollup", "TraceLean.Rollup.combine", OP, &["levels"], &scratch);

    let result = run(
        OP,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 37, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The property that rules out an average: the result is always one of the
/// inputs. Checked directly, because it is the whole content of the rule.
///
/// @tests ARCH-HONEST.weakest_link
#[test]
fn the_result_is_always_one_of_the_inputs() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::trace::rollup::combine;

    let schema = input_schema();
    let mut rng = gen::Rng::new(37);
    let (mut mixed, mut uniform, mut empty) = (0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let levels: Vec<Level> = serde_json::from_value(v["levels"].clone()).unwrap();
        let result = combine(levels.clone());
        if levels.is_empty() {
            assert_eq!(result, Level::L1, "nothing to aggregate is the lowest");
            empty += 1;
        } else {
            assert!(levels.contains(&result), "{result:?} is not one of {levels:?}");
            if levels.iter().any(|l| *l != levels[0]) {
                mixed += 1;
            } else {
                uniform += 1;
            }
        }
    }
    // Mixed levels are the cases with content: a run where every input was the
    // same level cannot tell a minimum from a mean, from a maximum, or from
    // returning the first thing it was given.
    support::covered(
        OP,
        &[
            ("levels that are not all the same", mixed),
            ("every level the same", uniform),
            ("nothing to aggregate", empty),
        ],
    );
}

// ─── Rolling a declared graph up ─────────────────────────────────────────────

fn strukt2(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn req_id() -> Schema {
    Schema::Str { max_len: Some(0), examples: vec!["A".into(), "B".into(), "C".into()] }
}

fn clause_key() -> Schema {
    Schema::Option {
        inner: Box::new(Schema::Str {
            max_len: Some(0),
            examples: vec!["one".into(), "two".into()],
        }),
    }
}

fn level_schema() -> Schema {
    Schema::simple_enum(&["L1", "L2", "L3", "L4"])
}

fn graph_input() -> Schema {
    let node = strukt2(&[
        ("id", req_id()),
        ("complete", Schema::Bool),
        ("clauses", Schema::List { inner: Box::new(clause_key()), max_len: Some(2) }),
        ("exempt", Schema::List { inner: Box::new(clause_key()), max_len: Some(1) }),
        ("refines", Schema::List { inner: Box::new(req_id()), max_len: Some(2) }),
    ]);
    strukt2(&[
        ("nodes", Schema::List { inner: Box::new(node), max_len: Some(3) }),
        (
            "levels",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![
                        Schema::Tuple { items: vec![req_id(), clause_key()] },
                        level_schema(),
                    ],
                }),
                max_len: Some(3),
            },
        ),
        ("root", req_id()),
        ("floor", level_schema()),
    ])
}

/// @drt REQ-ROLLUP.open_is_lower_bound
/// @tests REQ-ROLLUP.open_is_lower_bound
/// @tests REQ-ROLLUP.never_complete_when_open
/// @tests REQ-ROLLUP.exempt_leaves_denominator
/// @tests REQ-ROLLUP.deterministic_order
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_rolling_a_graph_up() {
    let scratch = harness::scratch("rollgraph");
    let op = "REQ-ROLLUP.open_is_lower_bound";
    let implementation = harness::rust_runner(
        "REQ-ROLLUP",
        "open_is_lower_bound",
        "crates/core/src/trace/rollup.rs::roll_up",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Rollup",
        "TraceLean.Rollup.rollUp",
        op,
        &["nodes", "levels", "root", "floor"],
        &scratch,
    );

    let result = run(
        op,
        &graph_input(),
        &model,
        &implementation,
        RunOptions { seed: 91, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The three properties stated directly against the implementation: an
/// unclaimed decomposition never reads as finished, an exemption leaves the
/// denominator, and the result does not depend on the order nodes arrive in.
///
/// @tests REQ-ROLLUP.never_complete_when_open
/// @tests REQ-ROLLUP.exempt_leaves_denominator
/// @tests REQ-ROLLUP.deterministic_order
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_roll_up_is_a_function_of_the_graph_and_never_flatters_it() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::trace::rollup::{roll_up, Node};

    let schema = graph_input();
    let mut rng = gen::Rng::new(91);
    let (mut inexact, mut exempted, mut deep) = (0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let nodes: Vec<Node> = serde_json::from_value(v["nodes"].clone()).unwrap();
        let levels: Vec<((String, Option<String>), Level)> =
            serde_json::from_value(v["levels"].clone()).unwrap();
        let root = v["root"].as_str().unwrap().to_string();
        let floor: Level = serde_json::from_value(v["floor"].clone()).unwrap();

        let rolled = roll_up(nodes.clone(), levels.clone(), root.clone(), floor);

        // Order independence, where the graph is well formed. Two nodes
        // declaring the same identifier is a `DuplicateId` fault reported by
        // the checker; here the later one wins, so reversing the list is a
        // different graph rather than the same one shuffled.
        let unique: std::collections::BTreeSet<&String> = nodes.iter().map(|n| &n.id).collect();
        if unique.len() == nodes.len() {
            let mut reversed = nodes.clone();
            reversed.reverse();
            assert_eq!(
                rolled,
                roll_up(reversed, levels, root, floor),
                "the roll-up depends on the order nodes arrive in"
            );
        }

        if !rolled.covered.exact {
            assert!(!rolled.covered.is_complete(), "an unclaimed decomposition read as finished");
            inexact += 1;
        }
        // The last declaration of an identifier is the one the map keeps.
        if let Some(node) = nodes.iter().rev().find(|n| n.id == rolled.id) {
            let kept = node.clauses.iter().filter(|c| !node.exempt.contains(c)).count();
            assert_eq!(
                rolled.covered.total as usize, kept,
                "the denominator does not match the unexempted clauses"
            );
            if node.clauses.iter().any(|c| node.exempt.contains(c)) {
                exempted += 1;
            }
        }
        if !rolled.children.is_empty() {
            deep += 1;
        }
    }
    support::covered(
        "REQ-ROLLUP.open_is_lower_bound",
        &[
            ("a roll-up that is a lower bound", inexact),
            ("a root with an exempted clause", exempted),
            ("a roll-up with children", deep),
        ],
    );
}

/// A four-name alphabet, so a claim about an unscanned file and a repeated file
/// are both common rather than coincidental.
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

/// Coverage over files, checked against the model.
///
/// The failure this pins is the one that looks like progress: a figure that
/// counts only annotated files rises when somebody deletes an annotation. A
/// coverage number that improves when you do less work is worse than no number
/// at all.
///
/// @drt REQ-ROLLUP.untraced_counted
/// @tests REQ-ROLLUP.untraced_counted
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_coverage_over_files() {
    let scratch = harness::scratch("file-coverage");
    let op = "REQ-ROLLUP.untraced_counted";
    let implementation = harness::rust_runner(
        "REQ-ROLLUP",
        "untraced_counted",
        "crates/core/src/trace/rollup.rs::file_coverage",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Rollup",
        "TraceLean.Rollup.fileCoverage",
        op,
        &["scanned", "claimed"],
        &scratch,
    );

    let result = run(
        op,
        &file_coverage_input(),
        &model,
        &implementation,
        RunOptions { seed: 127, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The four file shapes the rule distinguishes, and the property that gives the
/// rule its name: a claim about a file nothing scanned must not raise the
/// figure.
///
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

        // Adding a claim about a file nothing scanned cannot move the figure.
        // This is the whole content of the rule: the numerator is an
        // intersection, not a count of claims.
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
    support::covered(
        "REQ-ROLLUP.untraced_counted",
        &[
            ("a claim about a file nothing scanned", phantom),
            ("a scanned file nothing claims", untraced),
            ("every scanned file claimed", all_claimed),
            ("nothing scanned at all", nothing),
        ],
    );
}
