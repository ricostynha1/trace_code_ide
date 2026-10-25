mod harness;
mod support;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn seed() -> Schema {
    Schema::Nat {
        max: Some(u64::MAX),
        edges: vec![0, 1, 7, 2, u64::MAX, u64::MAX - 1, 0x9E37_79B9_7F4A_7C15],
    }
}

fn count() -> Schema {
    Schema::Nat { max: Some(8), edges: vec![0, 1] }
}

/// @drt REQ-DRT-GEN.fixed_stream
/// @tests REQ-DRT-GEN.fixed_stream
/// @tests ARCH-DETERMINISM.seeded_generation
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_raw_stream() {
    let scratch = harness::scratch("rawstream");
    let op = "REQ-DRT-GEN.fixed_stream";
    let implementation = harness::rust_runner(
        "REQ-DRT-GEN",
        "fixed_stream",
        "crates/core/src/drt/gen.rs::raw_stream",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Generator",
        "TraceLean.Generator.rawStream",
        op,
        &["seed", "count"],
        &scratch,
    );

    let result = run(
        op,
        &strukt(&[("seed", seed()), ("count", count())]),
        &model,
        &implementation,
        RunOptions { seed: 3, cases: 2_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-DRT-GEN.edges_sampled
/// @tests REQ-DRT-GEN.edges_sampled
/// @tests REQ-DRT-GEN.seed_reproduces
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_generated_numbers() {
    let scratch = harness::scratch("natstream");
    let op = "REQ-DRT-GEN.edges_sampled";
    let implementation = harness::rust_runner(
        "REQ-DRT-GEN",
        "edges_sampled",
        "crates/core/src/drt/gen.rs::nat_stream",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Generator",
        "TraceLean.Generator.natStream",
        op,
        &["seed", "max", "edges", "count"],
        &scratch,
    );

    let schema = strukt(&[
        ("seed", seed()),
        (
            "max",
            Schema::Option {
                inner: Box::new(Schema::Nat { max: Some(u64::MAX), edges: vec![0, 1, 9, u64::MAX] }),
            },
        ),
        (
            "edges",
            Schema::List {
                inner: Box::new(Schema::Nat { max: Some(100), edges: vec![0, 5000] }),
                max_len: Some(3),
            },
        ),
        ("count", count()),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 4, cases: 2_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-DRT-GEN.seed_reproduces
/// @tests REQ-DRT-GEN.seed_reproduces
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_every_shape_of_generated_value() {
    let scratch = harness::scratch("shapestream");
    let op = "REQ-DRT-GEN.seed_reproduces";
    let implementation = harness::rust_runner(
        "REQ-DRT-GEN",
        "seed_reproduces",
        "crates/core/src/drt/gen.rs::shape_stream",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Generator",
        "TraceLean.Generator.shapeStream",
        op,
        &["seed", "shape", "count"],
        &scratch,
    );

    let schema = strukt(&[
        ("seed", seed()),
        ("shape", Schema::Nat { max: Some(9), edges: vec![0, 9] }),
        ("count", count()),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 8, cases: 2_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-GEN.edges_sampled
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn a_declared_edge_is_drawn_far_more_often_than_chance_would_give_it() {
    use tracelean_core::drt::gen::nat_stream;

    let drawn = nat_stream(7, Some(1_000), vec![500], 10_000);
    let hits = drawn.iter().filter(|v| **v == 500).count();
    assert!(hits > 3_000, "the declared edge was drawn only {hits} times in 10000");

    let distinct: std::collections::BTreeSet<u64> = drawn.into_iter().collect();
    assert!(distinct.len() > 500, "only {} distinct values were generated", distinct.len());

    let plain = nat_stream(7, Some(1_000), vec![], 10_000);
    let five_hundreds = plain.iter().filter(|v| **v == 500).count();
    assert!(five_hundreds < 100, "an undeclared value was favoured {five_hundreds} times");
}
