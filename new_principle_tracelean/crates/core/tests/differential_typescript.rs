//! The web frontend's reading of a buffer, checked against the model.
//!
//! The same three questions the Rust core answers, asked of the TypeScript that
//! the browser runs, over the same generated cases — and answered against the
//! *model*, not against the Rust. Two implementations agree perfectly when both
//! are wrong; the model is the only thing either of them is trying to be.
//!
//! This is the difference between a second frontend and a second liability.

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
            // A character outside the basic plane is the case that separates
            // characters from code units, which is the mistake a JavaScript
            // implementation makes and a Rust one cannot.
            examples: vec![
                "".into(),
                "one".into(),
                "one\ntwo".into(),
                "é\nü".into(),
                "𝄞\nb".into(),
                "a\n\nb\n".into(),
            ],
        },
    );
    fields.insert("spans".to_string(), Schema::List { inner: Box::new(span()), max_len: Some(3) });
    Schema::Struct { fields }
}

fn check(op: &str, function: &str, entry: &str, arguments: &[&str], schema: Schema, seed: u64) {
    let clause = op.split_once('.').expect("an op names a clause").1;
    let scratch = harness::scratch(&format!("ts-{clause}"));
    let implementation = harness::ts_runner("REQ-VIEW", clause, entry, &scratch);
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

/// @drt REQ-VIEW.rendering_is_total
/// @tests REQ-DRT-TS.second_implementation_same_oracle
/// @tests REQ-DRT-TS.runs_the_shipped_source
#[test]
#[ignore = "runs the web frontend under node and builds a Lean package; run with --ignored"]
fn the_typescript_and_the_model_agree_on_the_reference_rendering() {
    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), buffer());
    check(
        "REQ-VIEW.rendering_is_total",
        "TraceLean.View.plainText",
        "web/src/view.ts::plainText",
        &["buffer"],
        Schema::Struct { fields },
        101,
    );
}

/// @drt REQ-VIEW.affordances_named
/// @tests REQ-DRT-TS.second_implementation_same_oracle
#[test]
#[ignore = "runs the web frontend under node and builds a Lean package; run with --ignored"]
fn the_typescript_and_the_model_agree_on_what_can_be_done_at_a_position() {
    let mut fields = BTreeMap::new();
    fields.insert("buffer".to_string(), buffer());
    fields.insert("offset".to_string(), Schema::Nat { max: Some(12), edges: vec![0, 1, 7] });
    check(
        "REQ-VIEW.affordances_named",
        "TraceLean.View.actionsAt",
        "web/src/view.ts::actionsAt",
        &["buffer", "offset"],
        Schema::Struct { fields },
        103,
    );
}

/// @drt REQ-VIEW.frontend_adds_nothing
/// @tests REQ-DRT-TS.second_implementation_same_oracle
#[test]
#[ignore = "runs the web frontend under node and builds a Lean package; run with --ignored"]
fn the_typescript_and_the_model_agree_on_whether_a_frontend_drew_the_buffer() {
    let mut rendering = BTreeMap::new();
    rendering.insert(
        "lines".to_string(),
        Schema::List {
            inner: Box::new(Schema::Str {
                max_len: None,
                examples: vec!["".into(), "one".into(), "é".into(), "main.rs".into()],
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
        "web/src/view.ts::conformance",
        &["buffer", "rendering"],
        Schema::Struct { fields },
        107,
    );
}

/// What a row of presented regions reads as.
///
/// The clause the window's icons rest on, asked of the language that paints
/// them. The names are read; the glyphs are not. A JavaScript implementation
/// joining the wrong field would draw a bar of emblems that a harness read back
/// as emblems, and the whole permission would be worthless.
///
/// @drt REQ-VIEW.presentation_may_be_symbolic
/// @tests REQ-DRT-TS.second_implementation_same_oracle
#[test]
#[ignore = "runs the web frontend under node and builds a Lean package; run with --ignored"]
fn the_typescript_and_the_model_agree_on_what_a_presented_row_reads_as() {
    let mut piece = BTreeMap::new();
    let words = |examples: Vec<String>| Schema::Str { max_len: Some(0), examples };
    piece.insert("painted".to_string(), words(vec!["📁".into(), "main.rs".into(), "é".into()]));
    piece.insert("name".to_string(), words(vec!["main.rs".into(), "é".into()]));
    let mut fields = BTreeMap::new();
    fields.insert(
        "row".to_string(),
        Schema::List { inner: Box::new(Schema::Struct { fields: piece }), max_len: Some(3) },
    );
    check(
        "REQ-VIEW.presentation_may_be_symbolic",
        "TraceLean.View.accessible",
        "web/src/view.ts::accessible",
        &["row"],
        Schema::Struct { fields },
        109,
    );
}

/// TypeScript sources for the parameter reader: the plain shapes, and the three
/// that made the first textual match the wrong one — a signature in a comment
/// or a string, the type-only `this`, and two declarations of one name.
fn ts_signature_input() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "source".to_string(),
        Schema::Str {
            max_len: None,
            examples: vec![
                "export function f(a: number, b: string) {}".into(),
                "export function f() {}".into(),
                "function g() {}".into(),
                "export function f(this: Window) {}".into(),
                "function g(x: Map<string, number>): void {}".into(),
                "function g<T>(xs: T[], n: number) {}".into(),
                "export function f(a: number, b?: number) {}".into(),
                "export function f(a = 1) {}".into(),
                "const f = (a) => a".into(),
                "// was: function f(b, a)\nexport function f(a: number, b: number) {}".into(),
                "/* function f(b, a) */ export function f(a, b) {}".into(),
                "const s = 'function f(b, a)';\nexport function f(a, b) {}".into(),
                "const t = `function g(y)`;\nfunction g(x) {}".into(),
                "export function f(this: Window, a: number) {}".into(),
                "function f(a) {}\nexport function f(a, b) {}".into(),
                "function g(x) {}\n// function g(z)\nfunction g(y) {}".into(),
                "function f(a) {}\nfunction g(a) {}\nfunction f(b) {}\nfunction g(b) {}".into(),
                "".into(),
            ],
        },
    );
    fields.insert(
        "symbol".to_string(),
        Schema::Str { max_len: None, examples: vec!["f".into(), "g".into(), "absent".into()] },
    );
    Schema::Struct { fields }
}

/// Every answer the TypeScript parameter reader can give, and each of the
/// three ways the first textual match was wrong, must occur.
///
/// @tests REQ-DRT-TS.params_from_source
#[test]
fn generation_reaches_every_way_a_typescript_signature_can_mislead() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::ts_runner::{ts_declarations, ts_parameters_of};

    let schema = ts_signature_input();
    let mut rng = gen::Rng::new(109);
    let (mut named, mut empty, mut refused, mut absent) = (0u64, 0u64, 0u64, 0u64);
    let (mut past_comment, mut this, mut ambiguous) = (0u64, 0u64, 0u64);
    // As many cases as the differential run asks, from the same seed.
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let source = v["source"].as_str().unwrap();
        let symbol = v["symbol"].as_str().unwrap();
        let answer = ts_parameters_of(source, symbol);
        let textual = source.matches(&format!("function {symbol}(")).count();
        let declared = ts_declarations(source, symbol);
        if textual > declared && declared == 1 && answer.is_some() {
            past_comment += 1;
        }
        if source.contains(&format!("function {symbol}(this")) && answer.is_some() {
            this += 1;
        }
        if declared > 1 && answer.is_none() {
            ambiguous += 1;
        }
        match answer {
            Some(names) if !names.is_empty() => named += 1,
            Some(_) => empty += 1,
            None if declared > 0 => refused += 1,
            None => absent += 1,
        }
    }
    support::covered(
        "REQ-DRT-TS.params_from_source",
        &[
            ("a signature yielding names", named),
            ("a signature taking no parameters", empty),
            ("a declared signature refused", refused),
            ("an absent symbol", absent),
            ("a signature in a comment or string passed over", past_comment),
            ("a this parameter dropped", this),
            ("two declarations refused", ambiguous),
        ],
    );
}

/// The parameter reader, which is what lets a binding name a TypeScript
/// function without also naming its argument order.
///
/// @drt REQ-DRT-TS.params_from_source
/// @tests REQ-DRT-TS.params_from_source
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_typescript_signature() {
    let scratch = harness::scratch("ts-params");
    let implementation = harness::rust_runner(
        "REQ-DRT-TS",
        "params_from_source",
        "crates/core/src/drt/ts_runner.rs::ts_parameters_owned",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Signature",
        "TraceLean.Signature.tsParameters",
        "REQ-DRT-TS.params_from_source",
        &["source", "symbol"],
        &scratch,
    );

    let result = run(
        "REQ-DRT-TS.params_from_source",
        &ts_signature_input(),
        &model,
        &implementation,
        RunOptions { seed: 109, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");
    support::agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
}

/// The cases must reach where a JavaScript implementation differs from a Rust
/// one, or agreement says nothing about the thing most likely to be wrong.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_text_where_characters_and_code_units_differ() {
    use tracelean_core::drt::gen;
    use tracelean_core::surface::view::Buffer;

    let schema = buffer();
    let mut rng = gen::Rng::new(101);
    let (mut astral, mut wide, mut empty, mut multiline) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..3_000 {
        let value = gen::value(&schema, &mut rng);
        let buffer: Buffer = serde_json::from_value(value).expect("the schema generates a buffer");
        // A character outside the basic plane is two UTF-16 code units, so a
        // frontend counting `String.length` is a position out of step here.
        if buffer.text.chars().any(|c| c as u32 > 0xFFFF) {
            astral += 1;
        }
        if buffer.text.chars().count() != buffer.text.len() {
            wide += 1;
        }
        if buffer.text.is_empty() {
            empty += 1;
        }
        if buffer.text.contains('\n') {
            multiline += 1;
        }
    }
    support::covered(
        "REQ-VIEW.rendering_is_total",
        &[
            ("text outside the basic plane", astral),
            ("a character that is not one byte", wide),
            ("no text at all", empty),
            ("more than one line", multiline),
        ],
    );
}
