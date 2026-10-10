//! What a pointer is offered at a position, checked against the model: every
//! action declared there, the buffer's own, the panes and the places, each
//! with the shortest keys that reach it.

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

/// Every kind of buffer, with the titles the menu treats specially.
fn kind() -> Schema {
    let mut variants = BTreeMap::new();
    for (variant, field, examples) in [
        ("file", "path", &["a.rs"][..]),
        ("directory", "path", &["src"][..]),
        ("review", "target", &["a.rs"][..]),
        ("menu", "title", &["requirements", "design", "welcome"][..]),
        ("record", "title", &["sandbox", "observed", "findings"][..]),
    ] {
        variants.insert(variant.to_string(), Some(Box::new(strukt(&[(field, name(examples))]))));
    }
    Schema::Enum { variants }
}

/// Actions a span may declare: ones the menu words specially, one carrying a
/// target, and a diff, which brings an accept and a reject for its file.
fn declared() -> Schema {
    name(&["file.open", "observe.diff", "observe.accept_file", "screen.show file:a.rs", "trace.check", "file.save"])
}

fn buffer() -> Schema {
    let span = strukt(&[
        ("start", Schema::Nat { max: Some(8), edges: vec![0] }),
        ("stop", Schema::Nat { max: Some(12), edges: vec![0, 20] }),
        ("role", support::role()),
        ("actions", Schema::List { inner: Box::new(declared()), max_len: Some(2) }),
    ]);
    strukt(&[
        ("id", name(&["dir:src"])),
        ("kind", kind()),
        ("text", name(&["", "src/a.rs", "a.rs\nb.rs", "é path\nx"])),
        ("spans", Schema::List { inner: Box::new(span), max_len: Some(3) }),
    ])
}

fn mode_name() -> Schema {
    name(&["Main", "File", "Window"])
}

/// Keymaps where an action is bound twice at different depths, or not at all,
/// and modes enter each other in cycles.
fn keymap() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert(
        "enter".into(),
        Some(Box::new(strukt(&[("mode", mode_name()), ("description", name(&["go"]))]))),
    );
    variants.insert(
        "dispatch".into(),
        Some(Box::new(strukt(&[
            ("action", name(&["file.save", "file.open", "trace.check", "observe.accept_file", "screen.close"])),
            ("description", name(&["do"])),
        ]))),
    );
    let mode = strukt(&[
        ("parent", Schema::Option { inner: Box::new(mode_name()) }),
        (
            "bindings",
            Schema::List {
                inner: Box::new(Schema::Tuple { items: vec![name(&["Space", "f", "s"]), Schema::Enum { variants }] }),
                max_len: Some(3),
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

/// @drt REQ-ACT.everything_is_offered
/// @tests REQ-ACT.everything_is_offered
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_position_offers() {
    let op = "REQ-ACT.everything_is_offered";
    let scratch = harness::scratch("offer");
    let implementation = harness::rust_runner(
        "REQ-ACT",
        "everything_is_offered",
        "crates/core/src/surface/offer.rs::offered_owned",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Offer",
        "TraceLean.Offer.offers",
        op,
        &["buffer", "offset", "keymap"],
        &scratch,
    );
    let schema = strukt(&[
        ("buffer", buffer()),
        ("offset", Schema::Nat { max: Some(10), edges: vec![0] }),
        ("keymap", keymap()),
    ]);
    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 402, cases: 1_500, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}
