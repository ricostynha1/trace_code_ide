//! A session: what a sequence of keys does to the editor.
//!
//! The keymap suite asks one question at a time — this mode, this key, what
//! outcome. A person does not. They press Space, then `t`, then `f`, and each
//! key lands on wherever the last one left them. That fold is the arrow nothing
//! stated, and the editor was unusable for a reason that lived exactly there
//! (`REQ-DRIVE`).
//!
//! What is generated is a keymap and a sequence of keys, so the cases include
//! the ones that matter: a key that opens a menu, a key that closes one, a key
//! the mode does not bind at all, and a sequence long enough for a walk to
//! diverge in the middle rather than at the first press.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn name(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}

fn mode_name() -> Schema {
    name(&["Normal", "Leader", "File"])
}

/// The keys a session presses. `Space` and `Escape` are in here for the same
/// reason: both are keys a keymap spells rather than keys that spell
/// themselves, and both were wrong in a frontend at some point.
fn key_name() -> Schema {
    name(&["Space", "Escape", "f", "t", " "])
}

fn binding() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "enter".into(),
        Some(Box::new(strukt(&[("mode", mode_name()), ("description", name(&["leader"]))]))),
    );
    variants.insert(
        "dispatch".into(),
        Some(Box::new(strukt(&[
            ("action", name(&["trace.check", "file.open"])),
            ("description", name(&["check"])),
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
                inner: Box::new(Schema::Tuple { items: vec![key_name(), binding()] }),
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

fn session() -> Schema {
    strukt(&[
        ("keymap", keymap()),
        ("mode", mode_name()),
        ("keys", Schema::List { inner: Box::new(key_name()), max_len: Some(4) }),
    ])
}

/// @drt REQ-DRIVE.session_is_a_value
/// @tests REQ-DRIVE.session_is_a_value
/// @tests REQ-DRIVE.walk_follows_the_machine
/// @tests REQ-DRIVE.menu_is_the_bar_there
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_sequence_of_keys_does() {
    let scratch = harness::scratch("drive");
    let op = "REQ-DRIVE.session_is_a_value";
    let implementation = harness::rust_runner(
        "REQ-DRIVE",
        "session_is_a_value",
        "crates/core/src/surface/drive.rs::drive_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Drive",
        "TraceLean.Drive.drive",
        op,
        &["keymap", "mode", "keys"],
        &scratch,
    );

    let result = run(
        op,
        &session(),
        &model,
        &implementation,
        RunOptions { seed: 27, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Generation reaches every kind of key a session can contain, and the two laws
/// that are not about agreement are stated here directly.
///
/// A session of keys that all pass through agrees with an implementation that
/// never moves, so the floors are on each of the four things a key can do —
/// and separately on a session long enough for the *second* key to be applied
/// to where the first one landed, because a walk that ignores its own history
/// is right about every one-key session.
///
/// @tests REQ-DRIVE.walk_follows_the_machine
/// @tests REQ-DRIVE.menu_is_the_bar_there
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_kind_of_key_and_the_walk_is_the_machine() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::drive::{drive_of, modes_visited};
    use tracelean_core::surface::keymap::{next_mode, which_key, Keymap, Outcome, step};

    let schema = session();
    let mut rng = gen::Rng::new(27);
    let (mut empty, mut several) = (0u64, 0u64);
    let (mut entering, mut dispatching, mut leaving, mut passing) = (0u64, 0u64, 0u64, 0u64);
    let mut offering = 0u64;

    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let keymap: Keymap = serde_json::from_value(value["keymap"].clone()).unwrap();
        let start = value["mode"].as_str().unwrap_or_default().to_string();
        let keys: Vec<String> = serde_json::from_value(value["keys"].clone()).unwrap();

        let steps = drive_of(keymap.clone(), start.clone(), keys.clone());

        // `walk_follows_the_machine`: each key is applied to where the previous
        // one landed. Re-walked here by hand, which is the only way to say that
        // the fold is a fold rather than three answers to the same question.
        let mut at = start.clone();
        assert_eq!(steps.len(), keys.len(), "a session answered a different number of times");
        for (index, key) in keys.iter().enumerate() {
            match step(keymap.clone(), at.clone(), key.clone()) {
                Outcome::Enter { .. } => entering += 1,
                Outcome::Dispatch { .. } => dispatching += 1,
                Outcome::Leave { .. } => leaving += 1,
                Outcome::PassThrough => passing += 1,
            }
            at = next_mode(keymap.clone(), at.clone(), key.clone());
            assert_eq!(steps[index].mode, at, "step {index} is not where the machine goes");
            assert_eq!(steps[index].key, *key, "step {index} reports the wrong key");

            // `menu_is_the_bar_there`: the bar of the mode the key reached, not
            // of the mode it was pressed in.
            let bar = which_key(&keymap, &at);
            assert_eq!(steps[index].menu, bar, "step {index} shows another mode's menu");
            if !bar.is_empty() {
                offering += 1;
            }
        }
        assert_eq!(modes_visited(&steps).len(), keys.len());

        if keys.is_empty() {
            empty += 1;
        } else if keys.len() > 1 {
            several += 1;
        }
    }

    support::covered(
        "REQ-DRIVE.session_is_a_value",
        &[
            ("a session of no keys", empty),
            ("a session of more than one key", several),
            ("a key that entered a mode", entering),
            ("a key that dispatched an action", dispatching),
            ("a key that left a mode", leaving),
            ("a key nothing bound", passing),
            ("a step whose menu offered something", offering),
        ],
    );
}
