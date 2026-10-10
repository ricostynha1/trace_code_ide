//! Producing buffers, checked against the model.
//!
//! Seven bindings: the normaliser every producer ends with, and one producer per
//! kind of thing the editor shows. `tidy` is the one that matters most — every
//! other binding rests on it, and it is the only place where a mark a parser got
//! wrong is turned into something a renderer can draw.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn role() -> Schema {
    support::role()
}

/// Every role, tokens too: the affordance table has a row for them that the
/// shared roles never asked about (found by measuring lines).
fn any_role() -> Schema {
    let Schema::Enum { mut variants } = support::role() else { unreachable!() };
    let mut kind = BTreeMap::new();
    kind.insert("kind".to_string(), Schema::simple_enum(&["keyword", "comment"]));
    variants.insert("token".to_string(), Some(Box::new(Schema::Struct { fields: kind })));
    Schema::Enum { variants }
}

/// Deliberately generous: offsets past any plausible text, so the clamping in
/// `tidy` is exercised rather than assumed.
fn offset() -> Schema {
    Schema::Nat { max: Some(14), edges: vec![0, 1, 9] }
}

fn span() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("start".to_string(), offset());
    fields.insert("stop".to_string(), offset());
    fields.insert("role".to_string(), role());
    fields.insert(
        "actions".to_string(),
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: None,
                examples: vec!["file.open".into(), "trace.check".into()],
            }),
            max_len: Some(2),
        },
    );
    Schema::Struct { fields }
}

fn mark() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("start".to_string(), offset());
    fields.insert("stop".to_string(), offset());
    fields.insert("role".to_string(), role());
    Schema::Struct { fields }
}

/// Texts rather than letters: a file is lines, and a generator drawing
/// characters would almost never produce one.
fn text() -> Schema {
    Schema::Str {
        max_len: None,
        examples: vec![
            "".into(),
            "one".into(),
            "one\ntwo".into(),
            "one\ntwo\nthree".into(),
            "é\nü".into(),
            "a\n\nb\n".into(),
        ],
    }
}

fn lines() -> Schema {
    Schema::List {
        inner: Box::new(Schema::Str {
            max_len: None,
            examples: vec!["".into(), "one".into(), "two".into(), "three".into(), "é".into()],
        }),
        max_len: Some(4),
    }
}

fn name() -> Schema {
    Schema::Str { max_len: Some(4), examples: vec!["a.rs".into(), "src".into()] }
}

fn check(op: &str, function: &str, entry: &str, arguments: &[&str], schema: Schema, seed: u64) {
    check_in("TraceLean.Produce", op, function, entry, arguments, schema, seed)
}

/// Most producers live in `Produce`; the listing lives beside the
/// representation, because it is also how `REQ-VIEW` shows that one span type
/// has two producers.
fn check_in(
    import: &str,
    op: &str,
    function: &str,
    entry: &str,
    arguments: &[&str],
    schema: Schema,
    seed: u64,
) {
    let (req, clause) = op.split_once('.').expect("an op names a clause");
    let scratch = harness::scratch(&format!("produce-{clause}"));
    let implementation = harness::rust_runner(req, clause, entry, &scratch);
    let model = harness::lean_runner(import, function, op, arguments, &scratch);

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

/// The normaliser every producer ends with.
///
/// @drt REQ-SHOW.well_formed_by_construction
/// @drt REQ-SHOW.producers_are_total
/// @tests REQ-SHOW.well_formed_by_construction
/// @tests REQ-SHOW.producers_are_total
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_tidying_spans() {
    let mut fields = BTreeMap::new();
    fields.insert("size".to_string(), Schema::Nat { max: Some(12), edges: vec![0, 3] });
    fields.insert("spans".to_string(), Schema::List { inner: Box::new(span()), max_len: Some(4) });
    check(
        "REQ-SHOW.well_formed_by_construction",
        "TraceLean.Produce.tidy",
        "crates/core/src/surface/produce.rs::tidy",
        &["size", "spans"],
        Schema::Struct { fields },
        53,
    );
}

/// The role-to-affordance policy, which both sides must read the same way.
///
/// @drt REQ-SHOW.actions_by_role
/// @tests REQ-SHOW.actions_by_role
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_role_affords() {
    let mut fields = BTreeMap::new();
    fields.insert("role".to_string(), any_role());
    check(
        "REQ-SHOW.actions_by_role",
        "TraceLean.Produce.actionsFor",
        "crates/core/src/surface/produce.rs::actions_for",
        &["role"],
        Schema::Struct { fields },
        59,
    );
}

/// Affordances come from the core, so every role the representation has must be
/// asked — a role the run never generated is a row of the table nobody checked,
/// and the one role that affords *nothing* is the arm most easily written wrong.
///
/// @tests REQ-SHOW.actions_by_role
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_role_and_the_one_that_affords_nothing() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::actions_for;
    use tracelean_core::surface::view::Role;

    let mut fields = BTreeMap::new();
    fields.insert("role".to_string(), any_role());
    let schema = Schema::Struct { fields };

    let mut rng = gen::Rng::new(59);
    let (mut affords, mut nothing, mut several) = (0u64, 0u64, 0u64);
    let mut roles = std::collections::BTreeSet::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let role: Role = serde_json::from_value(v["role"].clone()).unwrap();
        let actions = actions_for(role);
        roles.insert(format!("{role:?}"));

        // An affordance is a name a frontend dispatches, so an empty one would
        // be a button that does nothing.
        assert!(actions.iter().all(|name| !name.is_empty()), "a role affords an unnamed action");

        match actions.len() {
            0 => nothing += 1,
            1 => affords += 1,
            _ => {
                several += 1;
                affords += 1;
            }
        }
    }
    // Nineteen and not eight: `level` carries a grade, so there are four of
    // it, `claim` its kind, six, and `token` two kinds; a run that reached only
    // one of them would say nothing about a frontend that colours L1 and L4
    // the same.
    assert_eq!(roles.len(), 19, "only {roles:?} of the nineteen roles were generated");
    support::covered(
        "REQ-SHOW.actions_by_role",
        &[
            ("a role that affords something", affords),
            ("a role that affords nothing", nothing),
            ("a role that affords more than one action", several),
        ],
    );
}

/// The producer for a directory. It lives beside the representation because it
/// is also how `REQ-VIEW` shows that one span type has two producers; this is
/// the same function under this requirement's own claim.
///
/// @drt REQ-SHOW.listing_from_entries
/// @tests REQ-SHOW.listing_from_entries
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_listing() {
    check_in(
        "TraceLean.View",
        "REQ-SHOW.listing_from_entries",
        "TraceLean.View.directoryBuffer",
        "crates/core/src/surface/view.rs::directory_buffer",
        &["path", "entries"],
        listing_input(),
        97,
    );
}

fn listing_input() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("path".to_string(), name());
    fields.insert(
        "entries".to_string(),
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![
                    Schema::Nat { max: Some(3), edges: vec![0, 1] },
                    Schema::Str {
                        max_len: None,
                        examples: vec!["main.rs".into(), "deep".into(), "é.rs".into()],
                    },
                ],
            }),
            max_len: Some(4),
        },
    );
    Schema::Struct { fields }
}

/// A listing is indentation and one span per entry, so the cases worth reaching
/// are the empty directory, a nested entry, and a name whose characters are not
/// bytes — the last because the spans are offsets into the joined text and a
/// producer counting bytes would put them in the wrong place.
///
/// @tests REQ-SHOW.listing_from_entries
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_listings_worth_drawing() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::view::{directory_buffer, faults};

    let schema = listing_input();
    let mut rng = gen::Rng::new(97);
    let (mut empty, mut nested, mut wide, mut many) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let path = v["path"].as_str().unwrap().to_string();
        let entries: Vec<(usize, String)> = serde_json::from_value(v["entries"].clone()).unwrap();

        let buffer = directory_buffer(path, entries.clone());
        assert_eq!(faults(buffer.clone()), vec![], "a listing came out faulty");
        assert_eq!(
            buffer.spans.len(),
            entries.len(),
            "a listing does not have one span per entry"
        );

        if entries.is_empty() {
            empty += 1;
        } else {
            if entries.len() > 2 {
                many += 1;
            }
            if entries.iter().any(|(depth, _)| *depth > 0) {
                nested += 1;
            }
            if entries.iter().any(|(_, name)| name.chars().count() != name.len()) {
                wide += 1;
            }
        }
    }
    support::covered(
        "REQ-SHOW.listing_from_entries",
        &[
            ("an empty directory", empty),
            ("a nested entry", nested),
            ("a name whose characters are not bytes", wide),
            ("more than two entries", many),
        ],
    );
}

/// @drt REQ-SHOW.file_from_text
/// @tests REQ-SHOW.file_from_text
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_file() {
    check(
        "REQ-SHOW.file_from_text",
        "TraceLean.Produce.fileBuffer",
        "crates/core/src/surface/produce.rs::file_buffer",
        &["path", "text", "marks"],
        file_input(),
        61,
    );
}

fn file_input() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("path".to_string(), name());
    fields.insert("text".to_string(), text());
    fields.insert("marks".to_string(), Schema::List { inner: Box::new(mark()), max_len: Some(4) });
    Schema::Struct { fields }
}

/// A file buffer has to be asked about the texts a file actually takes: empty,
/// several lines, one ending in a newline, and one whose characters are not
/// bytes. Marks reaching past the text matter too — the producer clamps them,
/// and a run whose marks all fit would never make it do so.
///
/// @tests REQ-SHOW.file_from_text
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_the_texts_a_file_buffer_has_to_survive() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::{file_buffer, Mark};
    use tracelean_core::surface::view::{faults, BufferKind};

    let schema = file_input();
    let mut rng = gen::Rng::new(61);
    let (mut empty, mut multiline, mut trailing, mut wide, mut overhanging) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let path = v["path"].as_str().unwrap().to_string();
        let text = v["text"].as_str().unwrap().to_string();
        let marks: Vec<Mark> = serde_json::from_value(v["marks"].clone()).unwrap();

        let buffer = file_buffer(path.clone(), text.clone(), marks.clone());
        // Well formed by construction: a producer never emits a buffer the
        // representation would report a fault in.
        assert_eq!(faults(buffer.clone()), vec![], "a file buffer came out faulty");
        assert!(
            matches!(buffer.kind, BufferKind::File { .. }),
            "a file buffer is not of the file kind"
        );

        if text.is_empty() {
            empty += 1;
        }
        if text.contains('\n') {
            multiline += 1;
        }
        if text.ends_with('\n') {
            trailing += 1;
        }
        if text.chars().count() != text.len() {
            wide += 1;
        }
        if marks.iter().any(|mark| mark.stop > text.chars().count()) {
            overhanging += 1;
        }
    }
    support::covered(
        "REQ-SHOW.file_from_text",
        &[
            ("no text at all", empty),
            ("more than one line", multiline),
            ("text ending in a newline", trailing),
            ("a character that is not one byte", wide),
            ("a mark reaching past the text", overhanging),
        ],
    );
}

/// @drt REQ-SHOW.review_from_change
/// @tests REQ-SHOW.review_from_change
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_changed_between_two_texts() {
    let mut fields = BTreeMap::new();
    fields.insert("before".to_string(), lines());
    fields.insert("after".to_string(), lines());
    check(
        "REQ-SHOW.review_from_change",
        "TraceLean.Produce.diffLines",
        "crates/core/src/surface/produce.rs::diff_lines",
        &["before", "after"],
        Schema::Struct { fields },
        67,
    );
}

/// @drt REQ-SHOW.producer_is_pure
/// @tests REQ-SHOW.producer_is_pure
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_review() {
    check(
        "REQ-SHOW.producer_is_pure",
        "TraceLean.Produce.reviewBuffer",
        "crates/core/src/surface/produce.rs::review_buffer",
        &["target", "before", "after"],
        review_input(),
        71,
    );
}

fn review_input() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("target".to_string(), name());
    fields.insert("before".to_string(), text());
    fields.insert("after".to_string(), text());
    Schema::Struct { fields }
}

/// Purity is a claim about *every* call, so the run has to make the same call
/// twice and compare — and it has to do so on inputs where the producer has
/// something to decide. Two identical texts and two unrelated ones are the ends;
/// the middle, where the texts share a prefix, is where a producer that cached
/// something would go wrong.
///
/// @tests REQ-SHOW.producer_is_pure
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn a_review_buffer_is_the_same_every_time_it_is_asked() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::review_buffer;
    use tracelean_core::surface::view::faults;

    let schema = review_input();
    let mut rng = gen::Rng::new(71);
    let (mut same, mut different, mut shared, mut empty) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let target = v["target"].as_str().unwrap().to_string();
        let before = v["before"].as_str().unwrap().to_string();
        let after = v["after"].as_str().unwrap().to_string();

        let once = review_buffer(target.clone(), before.clone(), after.clone());
        let twice = review_buffer(target.clone(), before.clone(), after.clone());
        assert_eq!(once, twice, "a producer answered two different things to one question");
        assert_eq!(faults(once.clone()), vec![], "a review buffer came out faulty");

        if before == after {
            same += 1;
        } else {
            different += 1;
            let prefix = before
                .lines()
                .zip(after.lines())
                .take_while(|(a, b)| a == b)
                .count();
            if prefix > 0 {
                shared += 1;
            }
        }
        if before.is_empty() || after.is_empty() {
            empty += 1;
        }
    }
    support::covered(
        "REQ-SHOW.producer_is_pure",
        &[
            ("two texts that are the same", same),
            ("two texts that differ", different),
            ("two texts sharing a first line", shared),
            ("a side with no text at all", empty),
        ],
    );
}

/// @drt REQ-SHOW.menu_from_keymap
/// @tests REQ-SHOW.menu_from_keymap
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_menu() {
    check(
        "REQ-SHOW.menu_from_keymap",
        "TraceLean.Produce.menuBuffer",
        "crates/core/src/surface/produce.rs::menu_buffer",
        &["title", "entries"],
        menu_input(),
        73,
    );
}

/// A menu on one row: widths around the menu's full length, so it fits with
/// its descriptions, just fails to, and falls back to the keys.
///
/// @drt REQ-LOOK.row_fits
/// @tests REQ-LOOK.row_fits
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_menu_row() {
    check("REQ-LOOK.row_fits", "TraceLean.Produce.menuRow", "crates/core/src/surface/produce.rs::menu_row", &["title", "entries", "width"], menu_row_input(), 75);

    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::{menu_row, MenuEntry};
    let mut rng = gen::Rng::new(75);
    let (mut described, mut keys) = (0u64, 0u64);
    for _ in 0..500 {
        let v = gen::value(&menu_row_input(), &mut rng);
        let entries: Vec<MenuEntry> = serde_json::from_value(v["entries"].clone()).unwrap();
        if entries.iter().all(|e| e.description.is_empty()) {
            continue;
        }
        let row = menu_row("t".into(), entries.clone(), v["width"].as_u64().unwrap());
        if entries.iter().all(|e| row.text.contains(&e.description)) {
            described += 1;
        } else {
            keys += 1;
        }
    }
    support::covered("REQ-LOOK.row_fits", &[("fits with its descriptions", described), ("only the keys fit", keys)]);
}

fn menu_row_input() -> Schema {
    let Schema::Struct { mut fields } = menu_input() else { unreachable!() };
    fields.insert("width".to_string(), Schema::Nat { max: Some(40), edges: vec![0, 7, 8, 9, 18, 19] });
    Schema::Struct { fields }
}

/// The welcome page: the starts as a menu, then each recent folder as a path
/// that opens it — none, one, several, and folders whose characters are not
/// bytes, so a span offset counted in bytes would show.
///
/// @drt REQ-SHOW.recent_reopens
/// @tests REQ-SHOW.recent_reopens
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_welcome_page() {
    let Schema::Struct { fields } = menu_input() else { unreachable!() };
    let mut input = BTreeMap::new();
    input.insert("starts".to_string(), fields["entries"].clone());
    input.insert(
        "recent".to_string(),
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: Some(0),
                examples: vec!["/home/u/one".into(), "/home/ü/dois".into(), "".into(), "C:\\proj".into()],
            }),
            max_len: Some(3),
        },
    );
    check_in(
        "TraceLean.Welcome",
        "REQ-SHOW.recent_reopens",
        "TraceLean.Welcome.welcome",
        "crates/core/src/surface/welcome.rs::welcome_owned",
        &["starts", "recent"],
        Schema::Struct { fields: input },
        74,
    );
}

fn menu_input() -> Schema {
    let mut entry = BTreeMap::new();
    entry.insert(
        "key".to_string(),
        Schema::Str { max_len: Some(1), examples: vec!["t".into(), "u".into()] },
    );
    entry.insert(
        "description".to_string(),
        Schema::Str { max_len: None, examples: vec!["trace".into(), "undo".into(), "é".into()] },
    );
    entry.insert(
        "action".to_string(),
        Schema::Option {
            inner: Box::new(Schema::Str {
                max_len: None,
                examples: vec!["history.undo".into(), "trace.check".into()],
            }),
        },
    );
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), name());
    fields.insert(
        "entries".to_string(),
        Schema::List { inner: Box::new(Schema::Struct { fields: entry }), max_len: Some(4) },
    );
    Schema::Struct { fields }
}

/// A menu row carries its entry's action, and a row for an entry that has none
/// carries nothing. Both must occur: a run of rows that all dispatch would
/// agree about a producer that invented an action for the ones that do not.
///
/// @tests REQ-SHOW.menu_from_keymap
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_menu_rows_with_and_without_an_action() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::{menu_buffer, MenuEntry};
    use tracelean_core::surface::view::faults;

    let schema = menu_input();
    let mut rng = gen::Rng::new(73);
    let (mut dispatching, mut inert, mut empty, mut wide) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let title = v["title"].as_str().unwrap().to_string();
        let entries: Vec<MenuEntry> = serde_json::from_value(v["entries"].clone()).unwrap();

        let buffer = menu_buffer(title, entries.clone());
        assert_eq!(faults(buffer.clone()), vec![], "a menu came out faulty");
        assert_eq!(buffer.spans.len(), entries.len(), "a menu does not have one row per entry");
        // A row offers exactly what its entry declared, and never more.
        for (span, entry) in buffer.spans.iter().zip(&entries) {
            assert_eq!(
                span.actions,
                entry.action.clone().into_iter().collect::<Vec<_>>(),
                "a menu row offers an action its entry does not have"
            );
        }

        if entries.is_empty() {
            empty += 1;
        }
        if entries.iter().any(|entry| entry.action.is_some()) {
            dispatching += 1;
        }
        if entries.iter().any(|entry| entry.action.is_none()) {
            inert += 1;
        }
        if entries.iter().any(|e| e.description.chars().count() != e.description.len()) {
            wide += 1;
        }
    }
    support::covered(
        "REQ-SHOW.menu_from_keymap",
        &[
            ("a menu with a row that dispatches", dispatching),
            ("a menu with a row that does not", inert),
            ("a menu with no rows at all", empty),
            ("a row whose characters are not bytes", wide),
        ],
    );
}

/// Scrolling, which is a producer like any other.
///
/// @drt REQ-SHOW.window_is_a_buffer
/// @tests REQ-SHOW.window_is_a_buffer
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_part_of_a_buffer() {
    check(
        "REQ-SHOW.window_is_a_buffer",
        "TraceLean.Produce.window",
        "crates/core/src/surface/produce.rs::window",
        &["buffer", "start", "count"],
        window_input(),
        83,
    );
}

fn window_input() -> Schema {
    let mut kinds = BTreeMap::new();
    let path = Schema::Str { max_len: Some(4), examples: vec!["a.rs".into()] };
    for (variant, field) in [("file", "path"), ("record", "title")] {
        let mut inner = BTreeMap::new();
        inner.insert(field.to_string(), path.clone());
        kinds.insert(variant.to_string(), Some(Box::new(Schema::Struct { fields: inner })));
    }
    let mut buffer = BTreeMap::new();
    buffer.insert("id".to_string(), name());
    buffer.insert("kind".to_string(), Schema::Enum { variants: kinds });
    buffer.insert("text".to_string(), text());
    buffer.insert("spans".to_string(), Schema::List { inner: Box::new(span()), max_len: Some(3) });

    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), Schema::Struct { fields: buffer });
    // Past the end as well as inside it: a window that falls off is the case a
    // frontend reaches by scrolling one line too far.
    fields.insert("start".to_string(), Schema::Nat { max: Some(5), edges: vec![0, 1] });
    fields.insert("count".to_string(), Schema::Nat { max: Some(5), edges: vec![0, 1, 2] });
    Schema::Struct { fields }
}

/// A window is where scrolling goes wrong, so the run has to scroll off both
/// ends. A window entirely past the text and a window of no lines are the two a
/// frontend reaches by scrolling one line too far, and they are the two that
/// would produce a buffer whose spans point at nothing.
///
/// @tests REQ-SHOW.window_is_a_buffer
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_windows_that_fall_off_the_text() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::window;
    use tracelean_core::surface::view::{faults, plain_text, Buffer};

    let schema = window_input();
    let mut rng = gen::Rng::new(83);
    let (mut inside, mut past, mut none, mut whole) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let buffer: Buffer = serde_json::from_value(v["buffer"].clone()).unwrap();
        let start = v["start"].as_u64().unwrap() as usize;
        let count = v["count"].as_u64().unwrap() as usize;

        // The same count `window` itself scrolls over, so a situation never
        // claims a window fell off text it was in fact inside.
        let lines = plain_text(buffer.clone()).len();
        let shown = window(buffer.clone(), start, count);
        // Scrolling is a producer like any other, so what comes back is a
        // buffer the representation accepts — including when the window fell
        // off the end and there is nothing to show.
        assert_eq!(faults(shown.clone()), vec![], "a window came out faulty");
        assert_eq!(shown.kind, buffer.kind, "a window changed what kind of thing it shows");

        if count == 0 {
            none += 1;
        } else if start >= lines {
            past += 1;
        } else if start == 0 && count >= lines {
            whole += 1;
        } else {
            inside += 1;
        }
    }
    support::covered(
        "REQ-SHOW.window_is_a_buffer",
        &[
            ("a window inside the text", inside),
            ("a window past the end of the text", past),
            ("a window of no lines", none),
            ("a window showing the whole text", whole),
        ],
    );
}

/// @drt REQ-SHOW.record_from_events
/// @tests REQ-SHOW.record_from_events
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_record() {
    check(
        "REQ-SHOW.record_from_events",
        "TraceLean.Produce.recordBuffer",
        "crates/core/src/surface/produce.rs::record_buffer",
        &["title", "events"],
        record_input(),
        79,
    );
}

fn record_input() -> Schema {
    let mut event = BTreeMap::new();
    event.insert(
        "kind".to_string(),
        Schema::Str { max_len: None, examples: vec!["wrote".into(), "ran".into()] },
    );
    event.insert(
        "text".to_string(),
        Schema::Str { max_len: None, examples: vec!["a.rs".into(), "cargo test".into(), "é".into()] },
    );
    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), name());
    fields.insert(
        "events".to_string(),
        Schema::List { inner: Box::new(Schema::Struct { fields: event }), max_len: Some(4) },
    );
    Schema::Struct { fields }
}

/// A record is what a sandboxed agent's transcript looks like on screen: one
/// row per event, each offering the diff. An empty record has to occur — a
/// session that did nothing is the ordinary case at the start — and so does a
/// text whose characters are not bytes, because the row offsets are character
/// offsets and a producer counting bytes would place them wrong.
///
/// @tests REQ-SHOW.record_from_events
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_records_from_nothing_to_a_full_session() {
    use tracelean_core::drt::gen;
    use tracelean_core::observe::transcript::Event;
    use tracelean_core::surface::produce::record_buffer;
    use tracelean_core::surface::view::faults;

    let schema = record_input();
    let mut rng = gen::Rng::new(79);
    let (mut empty, mut some, mut many, mut wide) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let title = v["title"].as_str().unwrap().to_string();
        let events: Vec<Event> = serde_json::from_value(v["events"].clone()).unwrap();

        let buffer = record_buffer(title, events.clone());
        assert_eq!(faults(buffer.clone()), vec![], "a record came out faulty");
        assert_eq!(buffer.spans.len(), events.len(), "a record does not have one row per event");
        // Every row offers the same thing, because every row is the same kind
        // of thing: an event whose change a person can look at.
        for span in &buffer.spans {
            assert_eq!(span.actions, vec!["observe.diff".to_string()]);
        }

        match events.len() {
            0 => empty += 1,
            1 | 2 => some += 1,
            _ => many += 1,
        }
        if events.iter().any(|e| e.text.chars().count() != e.text.len()) {
            wide += 1;
        }
    }
    support::covered(
        "REQ-SHOW.record_from_events",
        &[
            ("a record of nothing", empty),
            ("a record of one or two events", some),
            ("a record of more than two events", many),
            ("an event whose characters are not bytes", wide),
        ],
    );
}

/// The laws must be asked where they have content. A run in which `tidy` never
/// dropped anything would agree without establishing that it drops the right
/// things, and a diff of two unrelated texts never shows that common ends are
/// kept.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_marks_worth_tidying_and_diffs_worth_reading() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::produce::{diff_lines, tidy, DiffLine, Mark};
    use tracelean_core::surface::view::{Role, Span};

    let mut rng = gen::Rng::new(53);
    let span_schema = Schema::List { inner: Box::new(span()), max_len: Some(4) };
    let (mut clamped, mut backwards, mut overlapped, mut kept_all, mut emptied) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let value = gen::value(&span_schema, &mut rng);
        let spans: Vec<Span> = serde_json::from_value(value).expect("the schema generates spans");
        if spans.iter().any(|span| span.stop > 6) {
            clamped += 1;
        }
        if spans.iter().any(|span| span.stop < span.start) {
            backwards += 1;
        }
        let out = tidy(6, spans.clone());
        if out.len() < spans.iter().filter(|s| s.start < s.stop).count() {
            overlapped += 1;
        }
        if !spans.is_empty() && out.len() == spans.len() {
            kept_all += 1;
        }
        if !spans.is_empty() && out.is_empty() {
            emptied += 1;
        }
    }

    let line_schema = lines();
    let (mut context, mut removed, mut added, mut unchanged) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let before: Vec<String> =
            serde_json::from_value(gen::value(&line_schema, &mut rng)).expect("lines");
        let after: Vec<String> =
            serde_json::from_value(gen::value(&line_schema, &mut rng)).expect("lines");
        let same = before == after;
        let out: Vec<DiffLine> = diff_lines(before, after);
        if same {
            unchanged += 1;
        }
        if out.iter().any(|line| line.role == Role::Plain) {
            context += 1;
        }
        if out.iter().any(|line| line.role == Role::Removed) {
            removed += 1;
        }
        if out.iter().any(|line| line.role == Role::Added) {
            added += 1;
        }
    }

    // A mark is a span without actions; the two travel the same path through
    // `tidy`, so reaching the shapes above reaches them for marks too.
    let _ = Mark { start: 0, stop: 1, role: Role::Plain };

    // Two bindings, two calls: `tidy` and `diff_lines` are separate ops, and a
    // count reported against the wrong one would say a law was exercised where
    // it never ran.
    support::covered(
        "REQ-SHOW.well_formed_by_construction",
        &[
            ("a span reaching past the text", clamped),
            ("a span that runs backwards", backwards),
            ("a span dropped for overlapping", overlapped),
            // The rarest of these: four spans that are each non-empty, in order
            // and disjoint over six characters is a narrow target for a
            // generator drawing offsets freely.
            ("spans that all survive", kept_all),
            ("spans of which none survive", emptied),
        ],
    );
    support::covered(
        "REQ-SHOW.review_from_change",
        &[
            // Context needs two random line lists to share an end, which is
            // rarer than either of them differing.
            ("a diff that keeps context", context),
            ("a diff with a removal", removed),
            ("a diff with an addition", added),
            ("two texts that are the same", unchanged),
        ],
    );
}
