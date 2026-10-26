mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn small(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

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
    let op = "REQ-STRENGTH.qualifies_proof";

    harness::agree(op, &input_schema(), 33, 3_000);
}

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
    harness::covered("REQ-STRENGTH.qualifies_proof", &counts);
}

/// @drt REQ-STRENGTH.obligation_generated
/// @tests REQ-STRENGTH.obligation_generated
/// @tests REQ-STRENGTH.not_proved_by_us
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_obligation_text() {
    let op = "REQ-STRENGTH.obligation_generated";

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

    harness::agree(op, &schema, 103, 2_000);
}
