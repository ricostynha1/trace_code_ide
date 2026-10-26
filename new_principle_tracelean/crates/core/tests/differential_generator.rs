mod harness;

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
    let op = "REQ-DRT-GEN.fixed_stream";

    harness::agree(op, &strukt(&[("seed", seed()), ("count", count())]), 3, 2_000);
}

/// @drt REQ-DRT-GEN.edges_sampled
/// @tests REQ-DRT-GEN.edges_sampled
/// @tests REQ-DRT-GEN.seed_reproduces
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_generated_numbers() {
    let op = "REQ-DRT-GEN.edges_sampled";

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

    harness::agree(op, &schema, 4, 2_000);
}

/// @drt REQ-DRT-GEN.seed_reproduces
/// @tests REQ-DRT-GEN.seed_reproduces
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_every_shape_of_generated_value() {
    let op = "REQ-DRT-GEN.seed_reproduces";

    let schema = strukt(&[
        ("seed", seed()),
        ("shape", Schema::Nat { max: Some(9), edges: vec![0, 9] }),
        ("count", count()),
    ]);

    harness::agree(op, &schema, 8, 2_000);
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
