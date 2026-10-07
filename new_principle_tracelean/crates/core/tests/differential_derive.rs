//! Deriving a schema from two signatures, checked against the model.
//!
//! The failure this guards is a pair of types turned into a generator neither
//! side has — a float approximated, an `Int` tested against a `u8`, a
//! structure paired with one that spells a field differently — and so a run
//! reporting agreement about inputs nobody passes.

mod harness;
mod support;

use std::collections::BTreeMap;

use tracelean_core::drt::derive::{pair, Paired, Structs, Ty};
use tracelean_core::drt::gen;
use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn small(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

/// Lean's integers are unbounded (`bits` 0) and Rust's are not; each side is
/// generated as its reader would write it, with the odd exception.
fn bits(lean: bool) -> Schema {
    if lean {
        Schema::Nat { max: Some(0), edges: vec![] }
    } else {
        Schema::Nat { max: Some(64), edges: vec![8, 64] }
    }
}

/// Types `depth` levels deep, as one side writes them.
fn side(depth: u32, lean: bool) -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    let field = |name: &str, schema: Schema| {
        Some(Box::new(Schema::Struct { fields: [(name.to_string(), schema)].into_iter().collect() }))
    };
    variants.insert("int".into(), field("bits", bits(lean)));
    variants.insert("nat".into(), field("bits", bits(lean)));
    for nullary in ["bool", "str", "float"] {
        variants.insert(nullary.into(), None);
    }
    variants.insert("named".into(), field("name", small(&["P"])));
    variants.insert("other".into(), field("text", small(&["&str"])));
    if depth > 0 {
        variants.insert("list".into(), field("inner", side(depth - 1, lean)));
        variants.insert("option".into(), field("inner", side(depth - 1, lean)));
    }
    Schema::Enum { variants }
}

/// Structures named `P` (sometimes `Q`), of a field or two.
fn structs(lean: bool) -> Schema {
    let mut simple: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    simple.insert("int".into(), Some(Box::new(Schema::Struct { fields: [("bits".to_string(), bits(lean))].into_iter().collect() })));
    simple.insert("bool".into(), None);
    let field = Schema::Tuple { items: vec![small(&["x"]), Schema::Enum { variants: simple }] };
    // Fixed lengths: one structure of one field. An empty list would make most
    // lookups fail before the fields are compared at all.
    let one = Schema::Tuple { items: vec![small(&["P"]), Schema::Tuple { items: vec![field] }] };
    Schema::Tuple { items: vec![one] }
}

fn input() -> Schema {
    Schema::Struct {
        fields: [
            ("lean".to_string(), side(1, true)),
            ("rust".to_string(), side(1, false)),
            ("leanStructs".to_string(), structs(true)),
            ("rustStructs".to_string(), structs(false)),
            ("fuel".to_string(), Schema::Nat { max: Some(4), edges: vec![] }),
        ]
        .into_iter()
        .collect(),
    }
}

/// @drt REQ-DRT-SCHEMA.derived_from_both
/// @tests REQ-DRT-SCHEMA.derived_from_both
/// @tests REQ-DRT-SCHEMA.outside_is_error
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_how_two_types_pair() {
    let op = "REQ-DRT-SCHEMA.derived_from_both";
    let scratch = harness::scratch("derive");
    let implementation = harness::rust_runner_with_params(
        "REQ-DRT-SCHEMA",
        "derived_from_both",
        "crates/core/src/drt/derive.rs::pair",
        &[("leanStructs", "lean_structs"), ("rustStructs", "rust_structs")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Derive",
        "TraceLean.Derive.pair",
        op,
        &["lean", "rust", "leanStructs", "rustStructs", "fuel"],
        &scratch,
    );
    let result = run(op, &input(), &model, &implementation, RunOptions { seed: 321, cases: 20_000, shrink_rounds: 100 })
        .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The generator reaches pairs that give a schema, pairs of structures, and
/// pairs that are refused.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_pairs_structures_and_mismatches() {
    let mut rng = gen::Rng::new(321);
    let (mut paired, mut structure, mut refused) = (0u64, 0u64, 0u64);
    for _ in 0..20_000 {
        let v = gen::value(&input(), &mut rng);
        let lean: Ty = serde_json::from_value(v["lean"].clone()).unwrap();
        let rust: Ty = serde_json::from_value(v["rust"].clone()).unwrap();
        let theirs: Structs = serde_json::from_value(v["leanStructs"].clone()).unwrap();
        let ours: Structs = serde_json::from_value(v["rustStructs"].clone()).unwrap();
        let fuel = v["fuel"].as_u64().unwrap() as u32;
        match pair(lean, rust, theirs, ours, fuel) {
            Paired::Ok(Schema::Struct { .. }) => {
                paired += 1;
                structure += 1;
            }
            Paired::Ok(_) => paired += 1,
            Paired::Err(_) => refused += 1,
        }
    }
    support::covered(
        "REQ-DRT-SCHEMA.derived_from_both",
        &[("paired", paired), ("a structure paired", structure), ("a mismatch reported", refused)],
    );
}
