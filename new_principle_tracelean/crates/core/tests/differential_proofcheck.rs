mod harness;

use harness::{name, strukt};

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

fn text(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(3), examples: examples.iter().map(|s| s.to_string()).collect() }
}

fn tags(variants: &[&str]) -> Schema {
    Schema::Enum { variants: variants.iter().map(|v| (v.to_string(), None)).collect::<BTreeMap<_, _>>() }
}

fn list(inner: Schema, max_len: usize) -> Schema {
    Schema::List { inner: Box::new(inner), max_len: Some(max_len) }
}

fn option(inner: Schema) -> Schema {
    Schema::Option { inner: Box::new(inner) }
}

fn axioms() -> Schema {
    let axiom = name(&[
        "sorryAx",
        "propext",
        "Classical.choice",
        "Quot.sound",
        "Lean.ofReduceBool",
        "Fix.oracle",
        "Fix.wishful",
    ]);
    list(Schema::Tuple { items: vec![axiom, Schema::Bool] }, 3)
}

fn facts() -> Schema {
    strukt(&[
        ("found", Schema::Bool),
        ("isType", Schema::Bool),
        ("mentions", Schema::Bool),
        ("general", Schema::Bool),
        ("leak", option(name(&["Fix.quad", "Fix.wrap"]))),
        ("ignores", Schema::Bool),
        ("closes", Schema::Bool),
        ("axioms", axioms()),
    ])
}

/// @drt REQ-STRENGTH.proof_states_the_model
/// @tests REQ-STRENGTH.proof_states_the_model
/// @tests REQ-STRENGTH.proof_is_general
/// @tests REQ-STRENGTH.proof_is_direct
/// @tests REQ-STRENGTH.proof_not_vacuous
/// @tests REQ-STRENGTH.model_is_a_function
/// @tests REQ-STRENGTH.axioms_declared
/// @tests REQ-STRENGTH.assumptions_shown
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_theorem_earns() {
    harness::agree("REQ-STRENGTH.proof_states_the_model", &strukt(&[("facts", facts())]), 71, 3_000);
}

/// @drt REQ-STRENGTH.pinned_by_theorem
/// @tests REQ-STRENGTH.pinned_by_theorem
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_pin_earns() {
    let pin = strukt(&[("found", Schema::Bool), ("statement", Schema::Bool), ("axioms", axioms())]);
    harness::agree("REQ-STRENGTH.pinned_by_theorem", &strukt(&[("facts", pin)]), 72, 2_000);
}

/// @drt REQ-STRENGTH.qualifies_proof
/// @tests REQ-STRENGTH.qualifies_proof
/// @tests REQ-STRENGTH.open_is_the_default
/// @tests REQ-STRENGTH.attempted_distinguished
/// @tests REQ-STRENGTH.nondeterministic_declared
/// @tests REQ-STRENGTH.verdict_kept
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_models_strength() {
    let pin = name(&["p1", "p2", "p3"]);
    let key = name(&["k1", "k2"]);
    let schema = strukt(&[
        ("held", option(Schema::Tuple { items: vec![key.clone(), list(pin.clone(), 3)] })),
        ("key_now", key),
        ("pins_now", list(pin, 3)),
        ("nondeterministic", option(text(&["io"]))),
    ]);
    harness::agree("REQ-STRENGTH.qualifies_proof", &schema, 73, 3_000);
}

/// @drt REQ-STRENGTH.obligation_generated
/// @tests REQ-STRENGTH.obligation_generated
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_statements_enter_the_obligation() {
    let both = strukt(&[
        ("found", Schema::Bool),
        ("isType", Schema::Bool),
        ("mentions", Schema::Bool),
        ("general", Schema::Bool),
        ("leak", option(name(&["Fix.quad"]))),
        ("ignores", Schema::Bool),
        ("closes", Schema::Bool),
        ("statement", Schema::Bool),
        ("axioms", axioms()),
    ]);
    let heard = strukt(&[
        ("heard", tags(&["facts", "obligation", "pinFacts", "missing"])),
        ("theorem", name(&["t1", "t2"])),
        ("model", name(&["m1", "m2"])),
        ("statement", text(&["∀ g"])),
        ("facts", both),
    ]);
    harness::agree("REQ-STRENGTH.obligation_generated", &strukt(&[("heard", list(heard, 5))]), 74, 2_000);
}

fn role() -> Schema {
    tags(&["models", "specifies", "implements", "tests", "drt", "proves", "pins"])
}

/// @drt REQ-STRENGTH.strength_per_model
/// @tests REQ-STRENGTH.strength_per_model
/// @tests ARCH-DETERMINISM.stable_ordering
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_plan_of_each_model() {
    let link = strukt(&[
        ("role", role()),
        ("reqId", name(&["REQ-A", "REQ-B"])),
        ("clause", option(name(&["a", "b"]))),
        ("symbol", name(&["M.f", "M.g", "M.t1", "M.t2"])),
        ("file", name(&["formal/M.lean", "src/m.rs"])),
        ("line", Schema::Nat { max: Some(9), edges: vec![] }),
        ("bodyHash", name(&["h1", "h2"])),
    ]);
    harness::agree("REQ-STRENGTH.strength_per_model", &strukt(&[("links", list(link, 8))]), 75, 2_000);
}
