//! The representation, checked against the model.
//!
//! Five bindings over one type: what is wrong with a buffer's spans, what can be
//! done at a position, the rendering every frontend must agree with, the spans a
//! synthetic buffer gets without a grammar, and the delta between two states.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn role() -> Schema {
    support::role()
}

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

fn span() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("start".to_string(), Schema::Nat { max: Some(12), edges: vec![0, 1] });
    fields.insert("stop".to_string(), Schema::Nat { max: Some(12), edges: vec![0, 3] });
    fields.insert("role".to_string(), role());
    fields.insert(
        "actions".to_string(),
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: None,
                examples: vec!["file.open".into(), "history.undo".into(), "trace.check".into()],
            }),
            max_len: Some(2),
        },
    );
    Schema::Struct { fields }
}

fn buffer() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "id".to_string(),
        Schema::Str { max_len: None, examples: vec!["dir:src".into(), "file:a.rs".into()] },
    );
    fields.insert("kind".to_string(), kind());
    fields.insert(
        "text".to_string(),
        Schema::Str {
            max_len: None,
            // Whole documents rather than letters: a buffer's text is lines, and
            // a generator drawing characters would almost never produce one.
            examples: vec![
                "".into(),
                "one".into(),
                "one\ntwo".into(),
                "main.rs\n  deep.rs\nlib.rs".into(),
                "a\n\nb\n".into(),
            ],
        },
    );
    fields.insert(
        "spans".to_string(),
        Schema::List { inner: Box::new(span()), max_len: Some(3) },
    );
    Schema::Struct { fields }
}

fn one_buffer() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), buffer());
    Schema::Struct { fields }
}

fn check(op: &str, function: &str, entry: &str, arguments: &[&str], schema: Schema, seed: u64) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("view-{clause}"));
    let implementation = harness::rust_runner("REQ-VIEW", clause, entry, &scratch);
    let model = harness::lean_runner("TraceLean.View", function, op, arguments, &scratch);

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

/// @drt REQ-VIEW.text_is_the_content
/// @drt REQ-VIEW.structure_over_text
/// @tests REQ-VIEW.text_is_the_content
/// @tests REQ-VIEW.structure_over_text
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_is_wrong_with_a_buffer() {
    check(
        "REQ-VIEW.text_is_the_content",
        "TraceLean.View.faults",
        "crates/core/src/surface/view.rs::faults",
        &["buffer"],
        one_buffer(),
        23,
    );
}

/// @drt REQ-VIEW.affordances_named
/// @tests REQ-VIEW.affordances_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_can_be_done_at_a_position() {
    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), buffer());
    fields.insert("offset".to_string(), Schema::Nat { max: Some(12), edges: vec![0, 1, 7] });
    check(
        "REQ-VIEW.affordances_named",
        "TraceLean.View.actionsAt",
        "crates/core/src/surface/view.rs::actions_at",
        &["buffer", "offset"],
        Schema::Struct { fields },
        29,
    );
}

/// @drt REQ-VIEW.rendering_is_total
/// @tests REQ-VIEW.rendering_is_total
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_reference_rendering() {
    check(
        "REQ-VIEW.rendering_is_total",
        "TraceLean.View.plainText",
        "crates/core/src/surface/view.rs::plain_text",
        &["buffer"],
        one_buffer(),
        31,
    );
}

/// @drt REQ-VIEW.structure_has_one_type
/// @tests REQ-VIEW.structure_has_one_type
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_listing_nobody_parsed() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "path".to_string(),
        Schema::Str { max_len: None, examples: vec!["src".into(), "reqs/trace".into()] },
    );
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
    check(
        "REQ-VIEW.structure_has_one_type",
        "TraceLean.View.directoryBuffer",
        "crates/core/src/surface/view.rs::directory_buffer",
        &["path", "entries"],
        Schema::Struct { fields },
        37,
    );
}

/// @drt REQ-VIEW.changes_are_deltas
/// @tests REQ-VIEW.changes_are_deltas
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_changed() {
    let mut fields = BTreeMap::new();
    fields.insert("before".to_string(), buffer());
    fields.insert("after".to_string(), buffer());
    check(
        "REQ-VIEW.changes_are_deltas",
        "TraceLean.View.delta",
        "crates/core/src/surface/view.rs::delta",
        &["before", "after"],
        Schema::Struct { fields },
        41,
    );
}

/// The laws must be asked where they have content: buffers whose spans are all
/// well formed would agree without testing the faults, and two identical
/// buffers would agree without testing the delta.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_faulty_spans_and_real_changes() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::view::{delta, faults, Buffer, Fault};

    let schema = buffer();
    let mut rng = gen::Rng::new(23);
    let (mut past, mut back, mut overlap, mut unordered, mut clean) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut text_changed, mut spans_changed, mut nothing) = (0u64, 0u64, 0u64);
    let mut previous: Option<Buffer> = None;
    for n in 0..3_000 {
        let value = gen::value(&schema, &mut rng);
        let mut buffer: Buffer = serde_json::from_value(value).expect("the schema generates a buffer");
        // One in fifty is the buffer before it again: two equal buffers drawn
        // at random are too rare to leave "nothing changed" to chance.
        if n % 50 == 49 {
            if let Some(before) = &previous {
                buffer = before.clone();
            }
        }
        let found = faults(buffer.clone());
        if found.is_empty() {
            clean += 1;
        }
        for fault in found {
            match fault {
                Fault::PastTheEnd { .. } => past += 1,
                Fault::Backwards { .. } => back += 1,
                Fault::Overlap { .. } => overlap += 1,
                Fault::Unordered { .. } => unordered += 1,
            }
        }
        if let Some(before) = previous.replace(buffer.clone()) {
            let d = delta(before, buffer);
            if d.text.is_some() {
                text_changed += 1;
            }
            if d.spans.is_some() {
                spans_changed += 1;
            }
            if d.text.is_none() && d.spans.is_none() && d.kind.is_none() {
                nothing += 1;
            }
        }
    }
    // Two ops share the loop but not the claim: the fault counts say what
    // `faults` was asked, and the delta counts say what `delta` was asked.
    support::covered(
        "REQ-VIEW.text_is_the_content",
        &[
            ("a span past the end of the text", past),
            ("a span that runs backwards", back),
            ("two spans overlapping", overlap),
            ("two spans out of order", unordered),
            ("a buffer whose spans are all well formed", clean),
        ],
    );
    support::covered(
        "REQ-VIEW.changes_are_deltas",
        &[
            ("a change to the text", text_changed),
            ("a change to the spans", spans_changed),
            ("nothing changed at all", nothing),
        ],
    );
}

/// A frontend's answer, checked against the buffer it was given.
///
/// @drt REQ-VIEW.frontend_adds_nothing
/// @drt REQ-VIEW.one_representation
/// @tests REQ-VIEW.frontend_adds_nothing
/// @tests REQ-VIEW.one_representation
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_whether_a_frontend_drew_the_buffer() {
    let mut rendering = BTreeMap::new();
    rendering.insert(
        "lines".to_string(),
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: None,
                examples: vec![
                    "".into(),
                    "one".into(),
                    "main.rs".into(),
                    "  deep.rs".into(),
                    "lib.rs".into(),
                ],
            }),
            max_len: Some(3),
        },
    );
    rendering.insert(
        "offered".to_string(),
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![
                    Schema::Nat { max: Some(10), edges: vec![0, 1] },
                    Schema::Str {
                        max_len: None,
                        examples: vec!["file.open".into(), "history.undo".into()],
                    },
                ],
            }),
            max_len: Some(3),
        },
    );
    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), buffer());
    fields.insert("rendering".to_string(), Schema::Struct { fields: rendering });
    check(
        "REQ-VIEW.frontend_adds_nothing",
        "TraceLean.View.conformance",
        "crates/core/src/surface/view.rs::conformance",
        &["buffer", "rendering"],
        Schema::Struct { fields },
        43,
    );
}

/// A row of presented regions, as a frontend with glyphs would report it.
///
/// The two fields are drawn from overlapping alphabets on purpose, so that a
/// run holds rows where the painting is the name, rows where it is an emblem,
/// and rows that mix the two — the last being what a station bar actually is.
fn presented_row() -> Schema {
    let mut fields = BTreeMap::new();
    // Small alphabets, and `max_len: Some(0)` so that the half of the time the
    // generator does not reach for an example it produces the empty string
    // rather than a fresh random word. Two independently drawn words are almost
    // never equal, and a run in which they never are would never reach the
    // ordinary case: a region with no emblem, painted as the word it is. The
    // names are drawn from a subset of the paintings for the same reason.
    let words = |examples: Vec<String>| Schema::Str { max_len: Some(0), examples };
    fields.insert(
        "painted".to_string(),
        words(vec!["📁".into(), "main.rs".into(), "é".into()]),
    );
    fields.insert("name".to_string(), words(vec!["main.rs".into(), "é".into()]));
    Schema::List { inner: Box::new(Schema::Struct { fields }), max_len: Some(3) }
}

/// What a row of presented regions reads as.
///
/// The clause the icons rest on: a frontend may paint what its medium affords
/// so long as each painted region carries the buffer's own text as its name,
/// and what is read off a screen is those names.
///
/// @drt REQ-VIEW.presentation_may_be_symbolic
/// @tests REQ-VIEW.presentation_may_be_symbolic
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_presented_row_reads_as() {
    let mut fields = BTreeMap::new();
    fields.insert("row".to_string(), presented_row());
    check(
        "REQ-VIEW.presentation_may_be_symbolic",
        "TraceLean.View.accessible",
        "crates/core/src/surface/view.rs::accessible",
        &["row"],
        Schema::Struct { fields },
        47,
    );
}

/// The symbolic case has to be reached, or the clause is answered by rows that
/// were never painted as anything.
///
/// A run made only of rows whose painting is their name would agree perfectly
/// and would say nothing about icons at all — which is the shape of vacuity
/// this project names rather than tolerates.
///
/// @tests REQ-VIEW.presentation_may_be_symbolic
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_rows_painted_as_themselves_and_as_symbols() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::view::{accessible, symbolic, Presented};

    let schema = presented_row();
    let mut rng = gen::Rng::new(47);
    let (mut none, mut some, mut all, mut empty, mut named) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let value = gen::value(&schema, &mut rng);
        let row: Vec<Presented> =
            serde_json::from_value(value).expect("the schema generates a row");

        // The law, over the same cases: what a row reads as never depends on
        // what was painted. Repaint every region with one glyph and the reading
        // is unchanged — which is `a_symbol_is_read_as_its_name`, asked of the
        // implementation rather than of the model.
        let repainted: Vec<Presented> = row
            .iter()
            .map(|piece| Presented { painted: "🙂".into(), name: piece.name.clone() })
            .collect();
        assert_eq!(
            accessible(repainted),
            accessible(row.clone()),
            "the painting changed what the row reads as"
        );

        let marked = symbolic(row.clone()).len();
        if row.is_empty() {
            empty += 1;
        } else if marked == 0 {
            none += 1;
        } else if marked == row.len() {
            all += 1;
        } else {
            some += 1;
        }
        if !accessible(row).is_empty() {
            named += 1;
        }
    }
    support::covered(
        "REQ-VIEW.presentation_may_be_symbolic",
        &[
            ("a row with nothing in it", empty),
            ("a row painted entirely as its own names", none),
            ("a row where some regions are painted as symbols", some),
            ("a row where every region is painted as a symbol", all),
            ("a row that reads as something", named),
        ],
    );
}

/// Asking what can be done at a position, where the answer is not obvious.
///
/// A position no span covers affords nothing, and a position two spans cover
/// affords the union without repeats. Both are cases a frontend reaches by
/// moving the cursor one character, and a run made of positions inside exactly
/// one span would agree about neither.
///
/// @tests REQ-VIEW.affordances_named
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_positions_covered_by_none_one_and_several_spans() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::view::{actions_at, Buffer};

    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), buffer());
    fields.insert("offset".to_string(), Schema::Nat { max: Some(12), edges: vec![0, 1, 7] });
    let schema = Schema::Struct { fields };

    let mut rng = gen::Rng::new(29);
    let (mut none, mut one, mut several, mut offered) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let buffer: Buffer = serde_json::from_value(v["buffer"].clone()).unwrap();
        let offset = v["offset"].as_u64().unwrap() as usize;

        let actions = actions_at(buffer.clone(), offset);
        // No repeats, whatever the spans said: a frontend that drew this list
        // would otherwise show the same button twice.
        let unique: std::collections::BTreeSet<&String> = actions.iter().collect();
        assert_eq!(unique.len(), actions.len(), "an affordance was offered twice");
        // Nothing is invented: every action offered comes from a span that
        // covers the position.
        for action in &actions {
            assert!(
                buffer.spans.iter().any(|span| span.start <= offset
                    && offset < span.stop
                    && span.actions.contains(action)),
                "`{action}` was offered at a position no span naming it covers"
            );
        }

        let covering = buffer
            .spans
            .iter()
            .filter(|span| span.start <= offset && offset < span.stop)
            .count();
        match covering {
            0 => {
                assert!(actions.is_empty(), "a position no span covers afforded something");
                none += 1;
            }
            1 => one += 1,
            _ => several += 1,
        }
        if !actions.is_empty() {
            offered += 1;
        }
    }
    support::covered(
        "REQ-VIEW.affordances_named",
        &[
            ("a position no span covers", none),
            ("a position exactly one span covers", one),
            ("a position more than one span covers", several),
            ("a position that affords something", offered),
        ],
    );
}
