//! Spec strength, checked against the model.
//!
//! `@proves` says a model has a property; it says nothing about how much that
//! property rules out. The four strength states exist to keep that distinction
//! visible, and the failure mode is that they quietly collapse — an obligation
//! nobody has attempted reported the same as one that is finished, or a pinning
//! theorem that merely *exists* reported as one that is *proved*.
//!
//! Generated link sets are the right input because the decision is about how
//! claims relate: which theorem is about which model, which pin belongs to
//! which declaration, which declaration was declared unpinnable. Those
//! relations are what a hand-written fixture fixes by accident.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn small(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

/// Only the roles this decision looks at, plus one it must ignore. A wider
/// vocabulary would mean most generated links were irrelevant, and a theorem
/// would almost never find the model it is about.
fn role() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for name in ["models", "proves", "pins", "implements"] {
        variants.insert(name.into(), None);
    }
    Schema::Enum { variants }
}

fn link() -> Schema {
    strukt(&[
        ("role", role()),
        ("reqId", small(&["REQ-A"])),
        ("clause", Schema::Option { inner: Box::new(small(&["one"])) }),
        ("anchor", small(&["M::f", "M::g", "M::thm"])),
        (
            "nondeterministic",
            Schema::Option { inner: Box::new(small(&["reads a clock"])) },
        ),
    ])
}

fn input_schema() -> Schema {
    strukt(&[("links", Schema::List { inner: Box::new(link()), max_len: Some(5) })])
}

/// @drt REQ-STRENGTH.qualifies_proof
/// @tests REQ-STRENGTH.qualifies_proof
/// @tests REQ-STRENGTH.open_is_the_default
/// @tests REQ-STRENGTH.attempted_distinguished
/// @tests REQ-STRENGTH.nondeterministic_declared
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_proof_is_worth() {
    let scratch = harness::scratch("strength");
    let op = "REQ-STRENGTH.qualifies_proof";
    let implementation = harness::rust_runner(
        "REQ-STRENGTH",
        "qualifies_proof",
        "crates/core/src/trace/strength.rs::obligations_from",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Strength",
        "TraceLean.Strength.obligationsFrom",
        op,
        &["links"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 33, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Three of the four states must occur — `pinned` cannot, and that is the
/// requirement: only the kernel may say a pinning theorem is finished, so
/// nothing derived from links alone is allowed to produce it.
///
/// @tests REQ-STRENGTH.not_proved_by_us
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_state_that_links_alone_may_produce() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::strength::{obligations_from, Strength, StrengthLink};

    let schema = input_schema();
    let mut rng = gen::Rng::new(33);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    let mut with_theorems = 0;
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let links: Vec<StrengthLink> = serde_json::from_value(v["links"].clone()).unwrap();
        for obligation in obligations_from(links) {
            *seen
                .entry(match obligation.strength {
                    Strength::Pinned { .. } => "pinned",
                    Strength::Attempted { .. } => "attempted",
                    Strength::Open => "open",
                    Strength::Nondeterministic { .. } => "nondeterministic",
                })
                .or_default() += 1;
            if !obligation.theorems.is_empty() {
                with_theorems += 1;
            }
        }
    }
    assert!(
        !seen.contains_key("pinned"),
        "links alone produced `pinned`; only the kernel may say a pin is finished"
    );
    let mut counts: Vec<(&str, u64)> = ["attempted", "open", "nondeterministic"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    counts.push(("an obligation carrying a theorem", with_theorems));
    support::covered("REQ-STRENGTH.qualifies_proof", &counts);
}

/// The obligation text, checked against the model.
///
/// The generated proof ends in `sorry` whatever the obligation, so the kernel
/// itself agrees it is unproved until somebody does the work. That is the
/// difference between a tool that helps you find out and one that tells you
/// what you wanted to hear, and it is worth a differential test because it
/// would be so easy to make the text look finished.
///
/// @drt REQ-STRENGTH.obligation_generated
/// @tests REQ-STRENGTH.obligation_generated
/// @tests REQ-STRENGTH.not_proved_by_us
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_obligation_text() {
    let scratch = harness::scratch("obligation");
    let op = "REQ-STRENGTH.obligation_generated";
    let implementation = harness::rust_runner(
        "REQ-STRENGTH",
        "obligation_generated",
        "crates/core/src/trace/strength.rs::obligation_source",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Strength",
        "TraceLean.Strength.obligationSource",
        op,
        &["obligation"],
        &scratch,
    );

    let mut strength: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    strength.insert("open".into(), None);
    strength.insert(
        "pinned".into(),
        Some(Box::new(strukt(&[("theoremName", small(&["M::thm"]))]))),
    );
    strength.insert(
        "attempted".into(),
        Some(Box::new(strukt(&[("theoremName", small(&["M::thm"]))]))),
    );
    strength.insert(
        "nondeterministic".into(),
        Some(Box::new(strukt(&[("reason", small(&["reads a clock"]))]))),
    );

    let schema = strukt(&[(
        "obligation",
        strukt(&[
            ("symbol", small(&["M::f", "TraceLean::assurance", "a.b.c"])),
            (
                "clauses",
                Schema::List {
                    inner: Box::new(Schema::Tuple {
                        items: vec![
                            small(&["REQ-A", "REQ-B"]),
                            Schema::Option { inner: Box::new(small(&["one", "two"])) },
                        ],
                    }),
                    max_len: Some(2),
                },
            ),
            (
                "theorems",
                Schema::List { inner: Box::new(small(&["t1", "t2"])), max_len: Some(2) },
            ),
            ("strength", Schema::Enum { variants: strength }),
        ]),
    )]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 103, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
