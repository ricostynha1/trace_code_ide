//! The annotation grammar, checked against its model.
//!
//! Totality is the property that matters: every `@word` must yield a directive
//! or a named problem. It is also the property a hand-written test suite is
//! worst at, because the cases that get dropped are the ones nobody thought of
//! — a bare `@`, an email address, an unterminated quote, an identifier
//! starting with a digit, a qualifier with no annotation above it. Generating
//! comment lines out of fragments produces those without anyone naming them.

mod harness;
mod support;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Fragments a comment body is joined out of. Every one of them is a case
/// somebody would have to think to write down.
fn line() -> Schema {
    Schema::Str {
        max_len: Some(3),
        examples: vec![
            "@implements REQ-X.clause".into(),
            "@models REQ-EVID".into(),
            "@tests REQ-A.b @drt REQ-A.b".into(),
            "@proves REQ-A begin".into(),
            "@end".into(),
            "@partial reason=\"only the happy path\"".into(),
            "@exempt REQ-A.b reason=platform by=ana until=2027".into(),
            "@nondeterministic".into(),
            // Things that must be named rather than dropped.
            "@banana REQ-X".into(),
            "@implements".into(),
            "@implements lowercase".into(),
            "@implementsREQ-X".into(),
            // Things that must not parse as directives at all.
            "write to a@example.com".into(),
            "an @ on its own".into(),
            "email@ REQ-X".into(),
            // Attribute edge cases.
            "@implements REQ-A.b reason=\"unterminated".into(),
            "@implements REQ-A.b =novalue key= k=v".into(),
            "@implements REQ-A.b b=2 a=1".into(),
            // Identifier edge cases.
            "@implements 9REQ".into(),
            "@implements REQ-A.".into(),
            "@implements REQ_A_B.c_d".into(),
            "".into(),
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[
        ("lines", Schema::List { inner: Box::new(line()), max_len: Some(3) }),
        ("firstLine", Schema::Nat { max: Some(20), edges: vec![0] }),
    ])
}

/// @drt REQ-ANNOT.totality
/// @tests REQ-ANNOT.totality
/// @tests REQ-ANNOT.role_vocabulary
/// @tests REQ-ANNOT.qualifiers
/// @tests REQ-ANNOT.unknown_role_named
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_grammar() {
    let scratch = harness::scratch("annotation");
    let op = "REQ-ANNOT.totality";
    let implementation = harness::rust_runner_with_params(
        "REQ-ANNOT",
        "totality",
        "crates/core/src/trace/annotation.rs::parse_comment_lines",
        &[("firstLine", "first_line")],
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Annotation",
        "TraceLean.Annotation.parseCommentLines",
        op,
        &["lines", "firstLine"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 44, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Totality, asserted directly: an `@` that starts a word either becomes a
/// directive or becomes a problem, and never vanishes.
///
/// @tests REQ-ANNOT.totality
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn every_directive_shaped_token_is_accounted_for() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::annotation::{parse_comment_lines, Directive, ProblemKind};

    let schema = input_schema();
    let mut rng = gen::Rng::new(44);
    let mut kinds = std::collections::BTreeSet::new();
    let mut problems = std::collections::BTreeSet::new();
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let lines: Vec<String> = serde_json::from_value(v["lines"].clone()).unwrap();
        let first = v["firstLine"].as_u64().unwrap() as u32;

        // Every at-sign that starts a word must be answered by something.
        let expected: usize = lines
            .iter()
            .map(|line| {
                line.match_indices('@')
                    .filter(|(at, _)| *at == 0 || line.as_bytes()[at - 1].is_ascii_whitespace())
                    .filter(|(at, _)| {
                        line[at + 1..].starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                    })
                    .count()
            })
            .sum();

        let parsed = parse_comment_lines(lines.clone(), first);
        assert_eq!(
            parsed.directives.len() + parsed.problems.len(),
            expected,
            "an at-sign was neither parsed nor reported: {lines:?}"
        );

        for d in &parsed.directives {
            kinds.insert(match d {
                Directive::Annotation { .. } => "annotation",
                Directive::Qualified { .. } => "qualified",
                Directive::End { .. } => "end",
            });
        }
        for p in &parsed.problems {
            problems.insert(match p.kind {
                ProblemKind::UnknownRole => "unknownRole",
                ProblemKind::MissingId => "missingId",
                ProblemKind::OrphanQualifier => "orphanQualifier",
                ProblemKind::UnclosedRegion => "unclosedRegion",
                ProblemKind::StrayEnd => "strayEnd",
            });
        }
    }
    for kind in ["annotation", "qualified", "end"] {
        assert!(kinds.contains(kind), "never generated a {kind} directive");
    }
    for kind in ["unknownRole", "missingId"] {
        assert!(problems.contains(kind), "never generated a {kind} problem");
    }
}

/// Region balance, checked against the model.
///
/// Both halves are reported, and they are different mistakes: an unclosed
/// region silently extends a claim over code nobody meant to claim, while a
/// stray `@end` means somebody thought they were closing something and were
/// not. A generator drawing `begin` and `@end` independently produces the
/// nesting no test author would think to write.
///
/// @drt REQ-ANNOT.region_balanced
/// @tests REQ-ANNOT.region_balanced
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_region_balance() {
    let scratch = harness::scratch("regions");
    let op = "REQ-ANNOT.region_balanced";
    let implementation = harness::rust_runner(
        "REQ-ANNOT",
        "region_balanced",
        "crates/core/src/trace/anchor.rs::region_balance",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Annotation",
        "TraceLean.Annotation.regionBalance",
        op,
        &["directives"],
        &scratch,
    );

    let small = |examples: &[&str]| Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    };
    let line_no = Schema::Nat { max: Some(9), edges: vec![0, 1, 2] };

    let mut qualifier: std::collections::BTreeMap<String, Option<Box<Schema>>> =
        std::collections::BTreeMap::new();
    qualifier.insert(
        "partial".into(),
        Some(Box::new(strukt(&[(
            "reason",
            Schema::Option { inner: Box::new(small(&["half"])) },
        )]))),
    );

    let mut directive: std::collections::BTreeMap<String, Option<Box<Schema>>> =
        std::collections::BTreeMap::new();
    directive.insert(
        "annotation".into(),
        Some(Box::new(strukt(&[(
            "annotation",
            strukt(&[
                ("role", Schema::simple_enum(&["models", "implements", "tests"])),
                ("reqId", small(&["REQ-A"])),
                ("clause", Schema::Option { inner: Box::new(small(&["one"])) }),
                (
                    "attrs",
                    Schema::List {
                        inner: Box::new(Schema::Tuple {
                            items: vec![small(&["reason"]), small(&["x"])],
                        }),
                        max_len: Some(0),
                    },
                ),
                // The whole question: half the annotations open a region.
                ("opensRegion", Schema::Bool),
                ("line", line_no.clone()),
            ]),
        )]))),
    );
    directive.insert(
        "qualified".into(),
        Some(Box::new(strukt(&[
            ("qualifier", Schema::Enum { variants: qualifier }),
            ("reqId", Schema::Option { inner: Box::new(small(&["REQ-A"])) }),
            ("clause", Schema::Option { inner: Box::new(small(&["one"])) }),
            ("line", line_no.clone()),
        ]))),
    );
    directive.insert("end".into(), Some(Box::new(strukt(&[("line", line_no)]))));

    let schema = strukt(&[(
        "directives",
        Schema::List { inner: Box::new(Schema::Enum { variants: directive }), max_len: Some(5) },
    )]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 107, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
