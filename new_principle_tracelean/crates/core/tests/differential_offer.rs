mod harness;

use harness::{strukt, name};

use std::collections::BTreeMap;

use tracelean_core::drt::schema::Schema;

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

fn declared() -> Schema {
    name(&[
        "file.open",
        "observe.diff",
        "observe.accept_file",
        "screen.show file:a.rs",
        "trace.check",
        "file.save",
        "file.explode",
        "design.toggle REQ-A",
    ])
}

fn buffer() -> Schema {
    let span = strukt(&[
        ("start", Schema::Nat { max: Some(8), edges: vec![0] }),
        ("stop", Schema::Nat { max: Some(12), edges: vec![0, 20] }),
        ("role", harness::role()),
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
    let schema = strukt(&[
        ("buffer", buffer()),
        ("offset", Schema::Nat { max: Some(10), edges: vec![0] }),
        ("keymap", keymap()),
    ]);
    harness::agree(op, &schema, 402, 1_500);
}
