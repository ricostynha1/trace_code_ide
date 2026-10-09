//! What a passing run is worth, checked against the model.
//!
//! The case this exists for: a run that agreed, over a law whose situation no
//! case reached. It is the most comfortable possible result and it means
//! nothing, so the verdict has to distinguish it from a run that met its floor
//! — and from one that reached the situation but not often enough, which is a
//! different problem with a different fix.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn situation() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec!["deletes".into(), "creates".into(), "empty".into()],
    }
}

fn count() -> Schema {
    Schema::Nat { max: Some(4), edges: vec![0, 1, 2] }
}

fn verdict_input() -> Schema {
    strukt(&[
        (
            "floors",
            Schema::List {
                inner: Box::new(strukt(&[("situation", situation()), ("atLeast", count())])),
                max_len: Some(3),
            },
        ),
        (
            "observed",
            Schema::List {
                inner: Box::new(strukt(&[("situation", situation()), ("reached", count())])),
                max_len: Some(3),
            },
        ),
        (
            "waivers",
            Schema::List {
                inner: Box::new(strukt(&[
                    (
                        "situations",
                        Schema::List { inner: Box::new(situation()), max_len: Some(2) },
                    ),
                    (
                        "reason",
                        Schema::Str {
                            max_len: Some(0),
                            examples: vec!["cannot be reached".into(), "".into(), "  ".into()],
                        },
                    ),
                ])),
                max_len: Some(2),
            },
        ),
    ])
}

/// @drt REQ-DRT-COVER.floor_stated
/// @tests REQ-DRT-COVER.floor_stated
/// @tests REQ-DRT-COVER.law_coverage
/// @tests REQ-DRT-COVER.vacuous_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_run_covered() {
    let scratch = harness::scratch("coverage");
    let op = "REQ-DRT-COVER.floor_stated";
    let implementation = harness::rust_runner(
        "REQ-DRT-COVER",
        "floor_stated",
        "crates/core/src/drt/coverage.rs::verdict",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Coverage",
        "TraceLean.Coverage.coverageVerdict",
        op,
        &["floors", "observed", "waivers"],
        &scratch,
    );

    let result = run(
        op,
        &verdict_input(),
        &model,
        &implementation,
        RunOptions { seed: 97, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What a run establishes, checked against the model.
///
/// @drt REQ-DRT-COVER.floor_unmet_is_not_pass
/// @tests REQ-DRT-COVER.floor_unmet_is_not_pass
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_that_an_uncovered_run_is_not_evidence() {
    let scratch = harness::scratch("coverage-level");
    let op = "REQ-DRT-COVER.floor_unmet_is_not_pass";
    let implementation = harness::rust_runner(
        "REQ-DRT-COVER",
        "floor_unmet_is_not_pass",
        "crates/core/src/drt/coverage.rs::level",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Coverage",
        "TraceLean.Coverage.coverageLevel",
        op,
        &["agreed", "verdict"],
        &scratch,
    );

    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    let mut gap: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    gap.insert("vacuous".into(), Some(Box::new(strukt(&[("situation", situation())]))));
    gap.insert(
        "short".into(),
        Some(Box::new(strukt(&[
            ("situation", situation()),
            ("reached", count()),
            ("atLeast", count()),
        ]))),
    );
    variants.insert("met".into(), None);
    variants.insert("undeclared".into(), None);
    variants.insert(
        "unmet".into(),
        Some(Box::new(strukt(&[(
            "gaps",
            Schema::List { inner: Box::new(Schema::Enum { variants: gap }), max_len: Some(2) },
        )]))),
    );

    let schema = strukt(&[
        ("agreed", Schema::Bool),
        ("verdict", Schema::Enum { variants }),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 101, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every verdict must occur, or the agreement above is about one branch.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_coverage_verdict() {
    use tracelean_core::drt::coverage::{verdict, Floor, Gap, Observed, Verdict};
    use tracelean_core::drt::gen;

    let schema = verdict_input();
    let mut rng = gen::Rng::new(97);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let floors: Vec<Floor> = serde_json::from_value(value["floors"].clone()).unwrap();
        let observed: Vec<Observed> = serde_json::from_value(value["observed"].clone()).unwrap();
        // An unmet run is counted once for each kind of gap it has.
        match verdict(floors, observed, Vec::new()) {
            Verdict::Met => *seen.entry("met").or_default() += 1,
            Verdict::Undeclared => *seen.entry("undeclared").or_default() += 1,
            Verdict::Unmet { gaps } => {
                if gaps.iter().any(|g| matches!(g, Gap::Vacuous { .. })) {
                    *seen.entry("vacuous").or_default() += 1;
                }
                if gaps.iter().any(|g| matches!(g, Gap::Short { .. })) {
                    *seen.entry("short").or_default() += 1;
                }
            }
        }
    }
    let counts: Vec<(&str, u64)> = ["met", "vacuous", "short", "undeclared"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-DRT-COVER.floor_stated", &counts);
}

/// Which waivers excuse nothing, checked against the model.
///
/// @drt REQ-DRT-COVER.waiver_unused_reported
/// @tests REQ-DRT-COVER.waiver_unused_reported
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_waivers_are_unused() {
    let scratch = harness::scratch("coverage-waivers");
    let op = "REQ-DRT-COVER.waiver_unused_reported";
    let implementation = harness::rust_runner(
        "REQ-DRT-COVER",
        "waiver_unused_reported",
        "crates/core/src/drt/coverage.rs::unused_waivers",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Coverage",
        "TraceLean.Coverage.unusedWaivers",
        op,
        &["floors", "observed", "waivers"],
        &scratch,
    );
    let result = run(
        op,
        &verdict_input(),
        &model,
        &implementation,
        RunOptions { seed: 131, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// How often the cases ran each line, checked against the model.
///
/// @drt REQ-DRT-COVER.lines_run
/// @tests REQ-DRT-COVER.lines_run
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_often_each_line_ran() {
    let scratch = harness::scratch("coverage-lines");
    let op = "REQ-DRT-COVER.lines_run";
    let implementation = harness::rust_runner(
        "REQ-DRT-COVER",
        "lines_run",
        "crates/core/src/drt/coverage.rs::line_reach",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Coverage",
        "TraceLean.Coverage.lineReach",
        op,
        &["hits", "item"],
        &scratch,
    );
    let line = || Schema::Nat { max: Some(5), edges: vec![0, 1] };
    let schema = strukt(&[
        (
            "hits",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![line(), Schema::Nat { max: Some(3), edges: vec![0] }],
                }),
                max_len: Some(5),
            },
        ),
        (
            "item",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![
                        line(),
                        Schema::Str {
                            max_len: Some(0),
                            examples: vec!["let a".into(), "}".into(), "  x ".into()],
                        },
                    ],
                }),
                max_len: Some(5),
            },
        ),
    ]);
    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 137, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The classes a seeded stream reaches, over every shape of schema.
///
/// @drt REQ-DRT-COVER.classes_reached
/// @tests REQ-DRT-COVER.classes_reached
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_classes_a_stream_reaches() {
    let scratch = harness::scratch("coverage-classes");
    let op = "REQ-DRT-COVER.classes_reached";
    let implementation = harness::rust_runner(
        "REQ-DRT-COVER",
        "classes_reached",
        "crates/core/src/drt/classes.rs::reached_shape",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Coverage",
        "TraceLean.Coverage.reachedShape",
        op,
        &["shape", "seed", "cases"],
        &scratch,
    );
    let schema = strukt(&[
        ("shape", Schema::Nat { max: Some(9), edges: vec![0, 9] }),
        ("seed", Schema::Nat { max: Some(1_000), edges: vec![0, 1] }),
        ("cases", Schema::Nat { max: Some(30), edges: vec![0, 1] }),
    ]);
    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 139, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}
