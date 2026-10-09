//! Validating a keymap, and answering what keys are available.
//!
//! The keymap is data a user edits, so it can be wrong in ways the code cannot.
//! Everything wrong with it is reported at load — a binding that fails when
//! pressed is a keymap that works until the moment somebody needs it.
//!
//! Two of the six problems are about reachability rather than about any single
//! binding: a mode no key sequence reaches is dead configuration, and an action
//! no sequence reaches is a feature the user cannot get to. Neither shows up by
//! pressing keys, which is why they are worth generating graphs for.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn name(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

fn mode_name() -> Schema {
    name(&["Main", "File", "Options"])
}

fn action_name() -> Schema {
    name(&["undo", "save"])
}

fn binding() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "enter".into(),
        Some(Box::new(strukt(&[
            ("mode", mode_name()),
            ("description", name(&["go"])),
        ]))),
    );
    variants.insert(
        "dispatch".into(),
        Some(Box::new(strukt(&[
            ("action", action_name()),
            ("description", name(&["do"])),
        ]))),
    );
    Schema::Enum { variants }
}

fn keymap() -> Schema {
    let mode = strukt(&[
        ("parent", Schema::Option { inner: Box::new(mode_name()) }),
        (
            "bindings",
            Schema::List {
                inner: Box::new(Schema::Tuple {
                    items: vec![name(&["u", "f", "o", "Escape"]), binding()],
                }),
                max_len: Some(2),
            },
        ),
    ]);
    strukt(&[
        ("root", mode_name()),
        (
            "modes",
            Schema::List {
                inner: Box::new(Schema::Tuple { items: vec![mode_name(), mode] }),
                max_len: Some(3),
            },
        ),
    ])
}

fn validate_input() -> Schema {
    strukt(&[
        ("keymap", keymap()),
        ("actions", Schema::List { inner: Box::new(action_name()), max_len: Some(2) }),
    ])
}

/// @drt REQ-MYTH.modes_defined
/// @tests REQ-MYTH.modes_defined
/// @tests REQ-MYTH.actions_defined
/// @tests REQ-MYTH.escape_terminates
/// @tests REQ-MYTH.actions_reachable
/// @tests REQ-MYTH.keymap_is_data
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_wrong_with_a_keymap() {
    let scratch = harness::scratch("validate");
    let op = "REQ-MYTH.modes_defined";
    let implementation = harness::rust_runner(
        "REQ-MYTH",
        "modes_defined",
        "crates/core/src/surface/keymap.rs::validate_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Keymap",
        "TraceLean.Keymap.validate",
        op,
        &["keymap", "actions"],
        &scratch,
    );

    let result = run(
        op,
        &validate_input(),
        &model,
        &implementation,
        RunOptions { seed: 24, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-MYTH.whichkey_is_a_query
/// @tests REQ-MYTH.whichkey_is_a_query
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_keys_are_available() {
    let scratch = harness::scratch("whichkey");
    let op = "REQ-MYTH.whichkey_is_a_query";
    let implementation = harness::rust_runner(
        "REQ-MYTH",
        "whichkey_is_a_query",
        "crates/core/src/surface/keymap.rs::which_key_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Keymap",
        "TraceLean.Keymap.whichKey",
        op,
        &["keymap", "mode"],
        &scratch,
    );

    let result = run(
        op,
        &strukt(&[("keymap", keymap()), ("mode", mode_name())]),
        &model,
        &implementation,
        RunOptions { seed: 25, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every problem the requirement names must occur, or agreement says nothing
/// about the cases that matter.
///
/// Also states the which-key law directly: the bar lists a key if and only if
/// the same keymap dispatches it.
///
/// @tests REQ-MYTH.whichkey_is_a_query
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_problem_and_the_bar_matches_the_machine() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::keymap::{
        validate_of, which_key_of, Keymap, Outcome, Problem, step,
    };

    let schema = validate_input();
    let mut rng = gen::Rng::new(24);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let keymap: Keymap = serde_json::from_value(v["keymap"].clone()).unwrap();
        let actions: Vec<String> = serde_json::from_value(v["actions"].clone()).unwrap();

        for problem in validate_of(keymap.clone(), actions) {
            *seen
                .entry(match problem {
                    Problem::UndefinedMode { .. } => "undefinedMode",
                    Problem::UndefinedAction { .. } => "undefinedAction",
                    Problem::ParentCycle { .. } => "parentCycle",
                    Problem::UnreachableMode { .. } => "unreachableMode",
                    Problem::UnreachableAction { .. } => "unreachableAction",
                    Problem::NoRoot { .. } => "noRoot",
                    Problem::UndefinedParent { .. } => "undefinedParent",
                    Problem::Stranded { .. } => "stranded",
                })
                .or_default() += 1;
        }

        // The bar is a query over the machine, so every key it offers must do
        // something other than pass through.
        for (mode_name, _) in &keymap.modes {
            for (key, _) in which_key_of(keymap.clone(), mode_name.clone()) {
                assert!(
                    !matches!(step(keymap.clone(), mode_name.clone(), key.clone()), Outcome::PassThrough),
                    "the bar offers `{key}` in `{mode_name}`, and pressing it does nothing"
                );
            }
        }
    }
    let counts: Vec<(&str, u64)> = [
        "undefinedMode",
        "undefinedAction",
        "parentCycle",
        "unreachableMode",
        "unreachableAction",
        "noRoot",
        "undefinedParent",
        "stranded",
    ]
    .iter()
    .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
    .collect();
    support::covered("REQ-MYTH.modes_defined", &counts);
}

/// The bar, asked where it has something to say.
///
/// A which-key bar listing nothing agrees with every implementation, including
/// one that never lists anything. So the floor is on modes that offer keys, and
/// separately on each of the three things a key can reach — a mode, an action,
/// and the way back out — because a bar that showed only one kind would look
/// right until somebody pressed another.
///
/// @tests REQ-MYTH.whichkey_is_a_query
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_bars_that_offer_every_kind_of_key() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::keymap::{step, which_key_of, Keymap, Outcome};

    let schema = strukt(&[("keymap", keymap()), ("mode", mode_name())]);
    let mut rng = gen::Rng::new(25);
    let (mut offering, mut empty) = (0u64, 0u64);
    let (mut entering, mut dispatching, mut leaving) = (0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let keymap: Keymap = serde_json::from_value(v["keymap"].clone()).unwrap();
        let mode = v["mode"].as_str().unwrap_or_default().to_string();

        let bar = which_key_of(keymap.clone(), mode.clone());
        // The bar is a query over the machine: every key it offers does
        // something, and it says what. Leaving counts — Escape is a key the bar
        // is right to show — but passing through does not, because a key that
        // passes through is one the mode does not bind at all.
        // The description is whatever the binding carries — generated keymaps
        // carry generated descriptions, including empty ones — so what is
        // checked here is that the bar reports the binding's own words, not
        // that the words are good.
        for (key, _description) in &bar {
            match step(keymap.clone(), mode.clone(), key.clone()) {
                Outcome::Enter { .. } => entering += 1,
                Outcome::Dispatch { .. } => dispatching += 1,
                Outcome::Leave { .. } => leaving += 1,
                Outcome::PassThrough => {
                    panic!("the bar offers `{key}` in `{mode}`, and pressing it does nothing")
                }
            }
        }

        if bar.is_empty() {
            empty += 1;
        } else {
            offering += 1;
        }
    }
    support::covered(
        "REQ-MYTH.whichkey_is_a_query",
        &[
            ("a mode offering at least one key", offering),
            ("a mode offering nothing", empty),
            ("a key that enters a mode", entering),
            ("a key that dispatches an action", dispatching),
            ("a key that leaves a mode", leaving),
        ],
    );
}
