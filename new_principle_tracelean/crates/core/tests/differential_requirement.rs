mod harness;

use harness::strukt;

use tracelean_core::drt::schema::Schema;

fn line() -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: vec![
            "---\nid: REQ-X\n---".into(),
            "---\nid: REQ-X\nstatus: approved\ndecomposition: complete\n---".into(),
            "---\nid: REQ-Y\nclauses:\n  one: does a thing\n  two: does another\n---".into(),
            "---\nid: REQ-Z\nrefines: [REQ-X, REQ-Y]\nclauses:\n  one: t\n---".into(),
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
    let op = "REQ-REQDOC.id_is_identity";

    harness::agree(op, &input_schema(), 61, 2_000);
}

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

    harness::covered(
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

/// @drt REQ-REQDOC.id_unique
/// @tests REQ-REQDOC.id_unique
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_identifiers_are_redeclared() {
    let op = "REQ-REQDOC.id_unique";

    let small = |examples: &[&str]| Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    };
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

    harness::agree(op, &schema, 109, 2_000);
}
