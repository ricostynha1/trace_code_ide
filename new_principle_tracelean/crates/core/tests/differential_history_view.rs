//! The undo tree's drawing and its filters, checked against the model: which
//! row each node is drawn on and in which column, and which nodes a filter
//! keeps under which ancestor.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::surface::history_view::{history_graph, shown_points, Filter, Point};

mod support;

/// Four node numbers, so branches, nodes named twice and parents that name
/// each other all occur often enough to be tested rather than hoped for.
fn number() -> Schema {
    Schema::Nat { max: Some(3), edges: vec![0, 1, 2, 3] }
}

fn points() -> Schema {
    let mut point = BTreeMap::new();
    point.insert("node".to_string(), number());
    point.insert("parent".to_string(), Schema::Option { inner: Box::new(number()) });
    point.insert("here".to_string(), Schema::Bool);
    point.insert("said".to_string(), Schema::Str { max_len: Some(0), examples: vec!["typed".into()] });
    point.insert(
        "file".to_string(),
        Schema::Option { inner: Box::new(Schema::Str { max_len: Some(0), examples: vec!["a.rs".into(), "b.rs".into()] }) },
    );
    point.insert("saved".to_string(), Schema::Bool);
    Schema::List { inner: Box::new(Schema::Struct { fields: point }), max_len: Some(5) }
}

fn graph() -> BTreeMap<String, Schema> {
    let mut fields = BTreeMap::new();
    fields.insert("points".to_string(), points());
    fields
}

fn filtered() -> BTreeMap<String, Schema> {
    let mut fields = graph();
    fields.insert("filter".to_string(), Schema::simple_enum(&["all", "file", "saved"]));
    fields.insert("file".to_string(), Schema::Str { max_len: Some(0), examples: vec!["a.rs".into(), "b.rs".into()] });
    fields
}

fn check(op: &str, function: &str, entry: &str, arguments: &[&str], fields: BTreeMap<String, Schema>, seed: u64) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("history-{clause}-{seed}"));
    let implementation = harness::rust_runner("REQ-UNDO", clause, entry, &scratch);
    let model = harness::lean_runner("TraceLean.HistoryView", function, op, arguments, &scratch);
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

/// @drt REQ-UNDO.tree_is_drawn
/// @tests REQ-UNDO.tree_is_drawn
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_the_tree_is_drawn() {
    check(
        "REQ-UNDO.tree_is_drawn",
        "TraceLean.HistoryView.historyGraph",
        "crates/core/src/surface/history_view.rs::history_graph",
        &["points"],
        graph(),
        301,
    );
}

/// @drt REQ-UNDO.filtered_view
/// @tests REQ-UNDO.filtered_view
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_filter_keeps() {
    check(
        "REQ-UNDO.filtered_view",
        "TraceLean.HistoryView.shownPoints",
        "crates/core/src/surface/history_view.rs::shown_points",
        &["points", "filter", "file"],
        filtered(),
        302,
    );
}

/// @drt REQ-UNDO.tree_is_shown
/// @tests REQ-UNDO.tree_is_shown
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_history_a_person_reads() {
    let mut fields = filtered();
    fields.insert(
        "file".to_string(),
        Schema::Option { inner: Box::new(Schema::Str { max_len: Some(0), examples: vec!["a.rs".into(), "b.rs".into()] }) },
    );
    check(
        "REQ-UNDO.tree_is_shown",
        "TraceLean.HistoryView.historyView",
        "crates/core/src/surface/history_view.rs::history_view_owned",
        &["points", "filter", "file"],
        fields,
        303,
    );
}

/// Texts that differ at the front, the back, the middle and not at all, long
/// enough that lines far from a change are left out.
fn text() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec![
            "".into(),
            "a".into(),
            "a\nb\nc\nd\ne\nf\ng".into(),
            "a\nb\nc\nX\ne\nf\ng".into(),
            "Y\nb\nc\nd\ne\nf\ng\nh".into(),
            "a\nb\n\nd\né".into(),
        ],
    }
}

/// @drt REQ-UNDO.hover_shows_change
/// @tests REQ-UNDO.hover_shows_change
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_change_a_node_shows() {
    let mut changed = BTreeMap::new();
    changed.insert("path".to_string(), Schema::Str { max_len: Some(0), examples: vec!["a.rs".into(), "b.rs".into()] });
    changed.insert("before".to_string(), text());
    changed.insert("after".to_string(), text());
    let mut fields = BTreeMap::new();
    fields.insert("node".to_string(), number());
    fields.insert("said".to_string(), Schema::Str { max_len: Some(0), examples: vec!["typed".into(), "".into()] });
    fields.insert(
        "changed".to_string(),
        Schema::List { inner: Box::new(Schema::Struct { fields: changed }), max_len: Some(2) },
    );
    check(
        "REQ-UNDO.hover_shows_change",
        "TraceLean.HistoryView.changeView",
        "crates/core/src/surface/history_view.rs::change_view_owned",
        &["node", "said", "changed"],
        fields,
        304,
    );
}

/// The generators reach what the clauses are about.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_branches_loops_and_hidden_positions() {
    let mut rng = gen::Rng::new(301);
    let (mut branch, mut two, mut looped, mut short) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: graph() }, &mut rng);
        let ps: Vec<Point> = serde_json::from_value(value["points"].clone()).expect("points");
        let rows = history_graph(ps.clone());
        if rows.iter().any(|r| r.cells.contains('╮')) {
            branch += 1;
        }
        if rows.iter().any(|r| r.cells.contains('┬')) {
            two += 1;
        }
        if ps.iter().any(|p| p.parent == Some(p.node)) {
            looped += 1;
        }
        if rows.len() < ps.len() + 1 {
            short += 1;
        }
    }
    support::covered(
        "REQ-UNDO.tree_is_drawn",
        &[("a branch", branch), ("two branches from one node", two), ("a node made after itself", looped), ("a node no row reaches", short)],
    );

    let mut rng = gen::Rng::new(302);
    let (mut hidden, mut lifted, mut none) = (0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&Schema::Struct { fields: filtered() }, &mut rng);
        let ps: Vec<Point> = serde_json::from_value(value["points"].clone()).expect("points");
        let filter: Filter = serde_json::from_value(value["filter"].clone()).expect("a filter");
        let file = value["file"].as_str().expect("a file").to_string();
        let shown = shown_points(ps.clone(), filter, file);
        let current_hidden = ps.iter().find(|p| p.here).is_some_and(|c| !shown.iter().any(|s| s.node == c.node));
        if current_hidden && shown.iter().any(|s| s.here) {
            hidden += 1;
        }
        if shown.iter().any(|s| ps.iter().find(|p| p.node == s.node).is_some_and(|p| p.parent != s.parent)) {
            lifted += 1;
        }
        if shown.is_empty() {
            none += 1;
        }
    }
    support::covered(
        "REQ-UNDO.filtered_view",
        &[("the position hidden and an ancestor marked", hidden), ("a node lifted to a further ancestor", lifted), ("nothing kept", none)],
    );
}
