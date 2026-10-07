//! Dispatch, checked against the model.
//!
//! One binding carries most of it: `dispatch` is the whole middle of the editor,
//! and a disagreement about what an action means is a key and a button doing
//! different things. The generator draws action names from the registry *and*
//! from outside it, so the refusal path is checked as hard as the rest.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn kind() -> Schema {
    let mut variants = BTreeMap::new();
    let path = Schema::Str { max_len: Some(4), examples: vec!["src".into(), "a.rs".into()] };
    for (name, field) in [
        ("file", "path"),
        ("directory", "path"),
        ("review", "target"),
        ("menu", "title"),
        ("record", "title"),
    ] {
        let mut fields = BTreeMap::new();
        fields.insert(field.to_string(), path.clone());
        variants.insert(name.to_string(), Some(Box::new(Schema::Struct { fields })));
    }
    Schema::Enum { variants }
}

fn focus() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("kind".to_string(), kind());
    fields.insert("offset".to_string(), Schema::Nat { max: Some(8), edges: vec![0, 1] });
    fields.insert(
        "under".to_string(),
        Schema::Option {
            inner: Box::new(Schema::Str {
                max_len: None,
                // A part's label and a history switch, so the switches that
                // name what they act on are reached too.
                examples: vec!["a.rs".into(), "src/lib.rs".into(), "".into(), "code".into(), "Saved".into(), "base".into()],
            }),
        },
    );
    Schema::Struct { fields }
}

/// Names from the registry, and names that are not — a dispatch that only ever
/// saw real actions would never establish that the rest are refused.
fn action() -> Schema {
    let mut examples: Vec<String> =
        tracelean_core::surface::keymap::ACTIONS.iter().map(|a| a.to_string()).collect();
    examples.push("file.explode".into());
    examples.push("".into());
    examples.push("trace".into());
    Schema::Str { max_len: None, examples }
}

fn workspace() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "files".to_string(),
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![
                    Schema::Str { max_len: None, examples: vec!["a.rs".into(), "src".into()] },
                    Schema::Str { max_len: None, examples: vec!["x".into(), "fn f".into()] },
                ],
            }),
            max_len: Some(3),
        },
    );
    Schema::Struct { fields }
}

fn check(op: &str, function: &str, entry: &str, arguments: &[&str], schema: Schema, seed: u64) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("act-{clause}"));
    let implementation = harness::rust_runner("REQ-ACT", clause, entry, &scratch);
    let model = harness::lean_runner("TraceLean.Act", function, op, arguments, &scratch);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @drt REQ-ACT.action_to_intent
/// @tests REQ-ACT.action_to_intent
/// @tests REQ-ACT.focus_is_carried
/// @tests REQ-ACT.edits_are_commands
/// @tests REQ-ACT.missing_target_is_refused
/// @tests REQ-ACT.dispatch_is_pure
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_an_action_means() {
    let mut fields = BTreeMap::new();
    fields.insert("action".to_string(), action());
    fields.insert("focus".to_string(), focus());
    fields.insert("w".to_string(), workspace());
    check(
        "REQ-ACT.action_to_intent",
        "TraceLean.Act.dispatch",
        "crates/core/src/surface/act.rs::dispatch",
        &["action", "focus", "w"],
        Schema::Struct { fields },
        83,
    );
}

/// @drt REQ-ACT.unknown_is_refused
/// @tests REQ-ACT.unknown_is_refused
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_whether_anything_happens() {
    let mut travel = BTreeMap::new();
    travel.insert("move".to_string(), Schema::simple_enum(&["back", "forward", "branch"]));
    let mut observe = BTreeMap::new();
    observe.insert("watch".to_string(), Schema::simple_enum(&["start", "accept", "reject"]));
    let mut display = BTreeMap::new();
    display.insert("what".to_string(), kind());
    let mut unknown = BTreeMap::new();
    unknown.insert(
        "action".to_string(),
        Schema::Str { max_len: None, examples: vec!["file.explode".into()] },
    );
    let mut blocked = BTreeMap::new();
    blocked.insert("unknownAction".to_string(), Some(Box::new(Schema::Struct { fields: unknown })));
    let mut why = BTreeMap::new();
    why.insert("why".to_string(), Schema::Enum { variants: blocked });

    let mut variants = BTreeMap::new();
    variants.insert("display".to_string(), Some(Box::new(Schema::Struct { fields: display })));
    variants.insert("travel".to_string(), Some(Box::new(Schema::Struct { fields: travel })));
    variants.insert("observe".to_string(), Some(Box::new(Schema::Struct { fields: observe })));
    variants.insert("persist".to_string(), None);
    variants.insert("refuse".to_string(), Some(Box::new(Schema::Struct { fields: why })));

    let mut fields = BTreeMap::new();
    fields.insert("intent".to_string(), Schema::Enum { variants });
    check(
        "REQ-ACT.unknown_is_refused",
        "TraceLean.Act.acts",
        "crates/core/src/surface/act.rs::acts",
        &["intent"],
        Schema::Struct { fields },
        89,
    );
}

/// A key and a rendered affordance carrying the same action reach the same
/// function with the same argument, so they cannot mean different things.
///
/// This is not a differential test but a structural one: the claim is about
/// there being one call, which no pair of values expresses.
///
/// @tests REQ-ACT.one_path
/// @structural REQ-ACT.one_path reason="a claim that two callers reach one function, which is a property of the code rather than of any value it computes"
#[test]
fn a_key_and_a_button_reach_the_same_dispatch() {
    use tracelean_core::history::command::Workspace;
    use tracelean_core::surface::act::{dispatch, Focus};
    use tracelean_core::surface::keymap::{self, Binding, Outcome};
    use tracelean_core::surface::produce::{menu_buffer, MenuEntry};
    use tracelean_core::surface::view::{actions_at, BufferKind};

    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("assets/keymap.json"),
    )
    .expect("the shipped keymap");
    let keymap = keymap::load(&text, &keymap::actions()).expect("it loads");

    let mut pairs = 0;
    for (mode_name, mode) in &keymap.modes {
        // What a key in this mode dispatches.
        let mut entries = Vec::new();
        for (key, binding) in &mode.bindings {
            let action = match binding {
                Binding::Dispatch { action, .. } => Some(action.clone()),
                Binding::Enter { .. } => None,
            };
            entries.push(MenuEntry {
                key: key.clone(),
                description: binding.description().to_string(),
                action: action.clone(),
            });
            let Some(action) = action else { continue };

            // The same mode as a buffer: what a click on that row would carry.
            let buffer = menu_buffer(mode_name.clone(), entries.clone());
            let row_start = buffer.spans.last().expect("the row just added").start;
            let offered = actions_at(buffer, row_start);
            assert_eq!(offered, vec![action.clone()], "the row does not carry what the key does");

            // And pressing the key says the same name.
            match keymap::step(keymap.clone(), mode_name.clone(), key.clone()) {
                Outcome::Dispatch { action: pressed } => {
                    assert_eq!(pressed, action, "key `{key}` in `{mode_name}`")
                }
                other => panic!("`{key}` in `{mode_name}` did not dispatch: {other:?}"),
            }

            // One function, one answer, whichever of the two arrived.
            let focus = Focus {
                kind: BufferKind::Menu { title: mode_name.clone() },
                offset: row_start,
                under: Some("a.rs".into()),
            };
            assert_eq!(
                dispatch(action.clone(), focus.clone(), Workspace::default()),
                dispatch(offered[0].clone(), focus, Workspace::default())
            );
            pairs += 1;
        }
    }
    support::each_occurred(&[("a key and a row carrying the same action", pairs)]);
    assert!(pairs > 15, "only {pairs} actions were reachable by key and by row");
}
