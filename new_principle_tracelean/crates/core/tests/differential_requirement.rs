//! Requirement documents, checked against the model.
//!
//! The interesting cases are not well-formed documents. They are fences that
//! do not close, indented lines with nothing above them, `id:` present and
//! empty, a list that is missing its bracket, and a colon inside a value. A
//! generator drawing from a line alphabet produces those combinations in an
//! order nobody would have thought to write down.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Lines a document might be made of.
fn line() -> Schema {
    Schema::Str {
        // Generated (non-example) lines are empty, which the parser skips.
        // With a one-character alphabet they would be junk that does nothing
        // either, and they would halve the chance of drawing a fence.
        max_len: Some(0),
        // `---` and `id:` are repeated so the generator draws them often. A
        // document is only a requirement if its first line is a fence, a later
        // line closes it, and an `id:` sits between — three coincidences that
        // a uniform alphabet produces perhaps once in a thousand documents,
        // which would leave every branch past the fence untested.
        examples: vec![
            // Whole documents, because an element is joined into the text with
            // a newline and may therefore be several lines. Drawing the fence,
            // the identifier and the closing fence as three independent lines
            // in the right order happens perhaps once in two hundred documents,
            // which leaves everything past the fence effectively untested.
            "---\nid: REQ-X\n---".into(),
            "---\nid: REQ-X\nstatus: approved\ndecomposition: complete\n---".into(),
            "---\nid: REQ-Y\nclauses:\n  one: does a thing\n  two: does another\n---".into(),
            "---\nid: REQ-Z\nrefines: [REQ-X, REQ-Y]\nclauses:\n  one: t\n---".into(),
            // Clauses written as blocks, and every way a block goes wrong.
            "---\nid: REQ-N\nclauses:\n  one: plain\n  two:\n    text: base\n    empty: none is L1\n---".into(),
            "---\nid: REQ-N\nclauses:\n  two:\n    empty: no text\n  one: t\n---".into(),
            "---\nid: REQ-N\nclauses:\n  two:\n    text: a\n    text: b\n    e: x\n    e: y\n---".into(),
            "---\nid: REQ-N\nclauses:\n  one: t\n  one: again\n---".into(),
            "---\nid: REQ-N\nclauses:\n  bad-key: t\n  two:\n    text: a\n    two words: n\n---".into(),
            "---\nid: REQ-N\niD: REQ-M\nclauess:\n---".into(),
            "---\niD: REQ-X\n---".into(),
            "---\nid: REQ-N\nclauses:".into(),
            "  two:".into(),
            "    text: base".into(),
            "    empty: none".into(),
            "\ttab: x".into(),
            "Title: cased".into(),
            "---\nid:\n---".into(),
            // `id:` with nothing after it opens a map rather than setting the
            // identifier; the empty identifier is the quoted one.
            "---\nid: \"\"\n---".into(),
            "---\nnot a key value\nid: REQ-X\n---".into(),
            "---\nclauses:\n  bare\nid: REQ-X\n---".into(),
            "---\n  orphan: indented\nid: REQ-X\n---".into(),
            "---\nid: REQ-X".into(),
            "---".into(),
            "---".into(),
            "id: REQ-X".into(),
            "clauses:".into(),
            "  one: does a thing".into(),
            "decomposition: complete".into(),
            "id: REQ-X".into(),
            "id:".into(),
            "id: ".into(),
            "title: A title".into(),
            "title: \"quoted\"".into(),
            "status: approved".into(),
            "status: nonsense".into(),
            "decomposition: complete".into(),
            "refines: [REQ-A, REQ-B]".into(),
            "refines: [".into(),
            "refines: []".into(),
            "clauses:".into(),
            "  one: does a thing".into(),
            "  two: does another: with a colon".into(),
            "  bare".into(),
            "  ".into(),
            "not a key value".into(),
            "# a comment".into(),
            "# A heading".into(),
            "".into(),
            "prose".into(),
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[("lines", Schema::List { inner: Box::new(line()), max_len: Some(8) })])
}

/// @drt REQ-REQDOC.id_is_identity
/// @tests REQ-REQDOC.id_is_identity
/// @tests REQ-REQDOC.clauseless_uniform
/// @tests REQ-REQDOC.decomposition_claimed
/// @tests REQ-REQDOC.malformed_reported
/// @tests REQ-REQDOC.narrowings_nest
/// @tests REQ-REQDOC.clause_unique
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_document_declares() {
    let scratch = harness::scratch("requirement");
    let op = "REQ-REQDOC.id_is_identity";
    let implementation = harness::rust_runner(
        "REQ-REQDOC",
        "id_is_identity",
        "crates/core/src/trace/requirement.rs::parse_lines",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Requirement",
        "TraceLean.Requirement.parseLines",
        op,
        &["lines"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 61, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A run in which nothing ever parsed as a requirement would agree about
/// nothing, and every named frontmatter fault must occur.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_requirements_and_every_fault() {
    use std::collections::BTreeSet;
    use tracelean_core::drt::gen;
    use tracelean_core::trace::requirement::parse_lines;

    let schema = input_schema();
    let mut rng = gen::Rng::new(61);
    let (mut requirements, mut clauseless, mut with_clauses, mut complete) = (0, 0, 0, 0);
    let mut narrowed = 0;
    let mut kinds = BTreeSet::new();
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let lines: Vec<String> = serde_json::from_value(value["lines"].clone()).unwrap();
        let parsed = parse_lines(lines);
        if parsed.is_requirement {
            requirements += 1;
            if parsed.clauses.is_empty() {
                clauseless += 1;
            } else {
                with_clauses += 1;
            }
            if parsed.decomposition
                == tracelean_core::trace::requirement::Decomposition::Complete
            {
                complete += 1;
            }
            if !parsed.narrowings.is_empty() {
                narrowed += 1;
            }
        }
        for (_, kind) in parsed.problems {
            kinds.insert(format!("{kind:?}"));
        }
    }

    support::covered(
        "REQ-REQDOC.id_is_identity",
        &[
            ("a document that was a requirement", requirements),
            ("a requirement declaring no clauses", clauseless),
            ("a requirement declaring clauses", with_clauses),
            ("a requirement claiming a complete decomposition", complete),
            ("a requirement with a narrowed clause", narrowed),
        ],
    );
    assert_eq!(kinds.len(), 9, "not every frontmatter fault occurred: {kinds:?}");
}

/// Which identifiers are redeclared, checked against the model.
///
/// @drt REQ-REQDOC.id_unique
/// @tests REQ-REQDOC.id_unique
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_identifiers_are_redeclared() {
    let scratch = harness::scratch("duplicate-ids");
    let op = "REQ-REQDOC.id_unique";
    let implementation = harness::rust_runner(
        "REQ-REQDOC",
        "id_unique",
        "crates/core/src/trace/requirement.rs::duplicate_ids",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Requirement",
        "TraceLean.Requirement.duplicateIds",
        op,
        &["declared"],
        &scratch,
    );

    let small = |examples: &[&str]| Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    };
    // Three identifiers over four files, so a redeclaration is the common case
    // rather than a coincidence; distinct random names never collide.
    let schema = strukt(&[(
        "declared",
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![
                    small(&["REQ-A", "REQ-B", "REQ-C"]),
                    small(&["a.md", "b.md", "nested/c.md", "d.md"]),
                ],
            }),
            max_len: Some(5),
        },
    )]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 109, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
