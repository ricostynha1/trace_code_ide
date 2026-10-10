//! Normalised hashing, checked against the model.
//!
//! This is the identity of everything the project claims. A body hash that
//! moves when the body did not makes people stop annotating; one that does not
//! move when the body did leaves stale evidence looking valid, which is the
//! failure the whole system exists to prevent.
//!
//! Both the normalised text and the digest are compared. The digest alone would
//! tell you the two sides disagree; the text tells you where.
//!
//! Sources are ASCII. Offsets are byte offsets and the model indexes by
//! character, so the two coincide here; a non-ASCII body reaches the hash
//! through the protected-range path, which is a copy on both sides.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Short ASCII bodies with the features that matter: runs of whitespace,
/// tabs, newlines, and text that could sit inside a literal.
fn source() -> Schema {
    Schema::Str {
        max_len: Some(2),
        examples: vec![
            "fn f() { let x = 1; }".into(),
            "fn f() {\n    let x = 1;\n}".into(),
            "fn f() {\n\t\tlet x = 1;\n}".into(),
            "  leading and trailing  ".into(),
            "a\t\t b\n\nc".into(),
            "let s = \"two  spaces\";".into(),
            "".into(),
            " ".into(),
        ],
    }
}

/// Ranges are unconstrained on purpose: inverted, overlapping and past the end
/// are all things a scanner bug would produce, and neither side may panic or
/// disagree about them.
fn range() -> Schema {
    Schema::Tuple {
        items: vec![
            Schema::Nat { max: Some(12), edges: vec![0] },
            Schema::Nat { max: Some(12), edges: vec![0] },
        ],
    }
}

fn input_schema() -> Schema {
    strukt(&[
        ("source", source()),
        ("removed", Schema::List { inner: Box::new(range()), max_len: Some(2) }),
        ("protected", Schema::List { inner: Box::new(range()), max_len: Some(2) }),
    ])
}

/// @drt REQ-ANCHOR.whitespace_normalised
/// @tests REQ-ANCHOR.whitespace_normalised
/// @tests REQ-ANCHOR.comments_excluded
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_normalised_body() {
    let scratch = harness::scratch("normalize");
    let op = "REQ-ANCHOR.whitespace_normalised";
    let implementation = harness::rust_runner(
        "REQ-ANCHOR",
        "whitespace_normalised",
        "crates/core/src/trace/hash.rs::normalize_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Hash",
        "TraceLean.Hash.normalize",
        op,
        &["source", "removed", "protected"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 55, cases: 3_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The digest itself. A hash function reimplemented in two languages is a
/// place where an off-by-one in the mixing constant or the byte order shows up
/// as nothing at all until two runs disagree.
///
/// @drt REQ-ANCHOR.hash_tracks_body
/// @tests REQ-ANCHOR.hash_tracks_body
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_body_hash() {
    let scratch = harness::scratch("bodyhash");
    let op = "REQ-ANCHOR.hash_tracks_body";
    let implementation = harness::rust_runner(
        "REQ-ANCHOR",
        "hash_tracks_body",
        "crates/core/src/trace/hash.rs::body_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Hash",
        "TraceLean.Hash.bodyHash",
        op,
        &["source", "removed", "protected"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 56, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A clause's hash is keyed by its name and covers its narrowings, so renaming
/// or narrowing a clause moves it; without a key it is the body's.
///
/// @drt REQ-REQDOC.clause_addressable
/// @tests REQ-REQDOC.clause_addressable
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_clause_hash() {
    clause_hash_agrees("REQ-REQDOC", "clause_addressable");
}

/// The hash records carry as the clause's text moves with that text and its
/// narrowings, so rewording or narrowing it re-opens what rests on it.
///
/// @drt REQ-STALE.requirement_reopens_all
/// @tests REQ-STALE.requirement_reopens_all
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_hash_records_carry() {
    clause_hash_agrees("REQ-STALE", "requirement_reopens_all");
}

fn clause_hash_agrees(req: &str, clause: &str) {
    let scratch = harness::scratch(&format!("clausehash-{clause}"));
    let op = format!("{req}.{clause}");
    let op = op.as_str();
    let implementation = harness::rust_runner(
        req,
        clause,
        "crates/core/src/trace/hash.rs::clause_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Hash",
        "TraceLean.Hash.clauseHash",
        op,
        &["key", "text", "narrowings"],
        &scratch,
    );

    let word = Schema::Str {
        max_len: Some(1),
        examples: vec![
            "ladder".into(),
            "weakest_link".into(),
            "empty".into(),
            " shall be  ordered ".into(),
        ],
    };
    let schema = strukt(&[
        ("key", Schema::Option { inner: Box::new(word.clone()) }),
        ("text", word.clone()),
        (
            "narrowings",
            Schema::List {
                inner: Box::new(Schema::Tuple { items: vec![word.clone(), word] }),
                max_len: Some(3),
            },
        ),
    ]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 57, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Both directions of `hash_tracks_body`, asserted directly: reindenting never
/// moves the hash, and adding a character always does.
///
/// @tests REQ-ANCHOR.hash_tracks_body
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_hash_moves_exactly_when_the_normalised_body_moves() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::hash::{body_of, normalize_of};

    let schema = input_schema();
    let mut rng = gen::Rng::new(55);
    let (mut held, mut moved) = (0, 0);
    for _ in 0..3_000 {
        let v = gen::value(&schema, &mut rng);
        let source = v["source"].as_str().unwrap().to_string();
        if !v["removed"].as_array().unwrap().is_empty()
            || !v["protected"].as_array().unwrap().is_empty()
        {
            continue;
        }

        let normalised = normalize_of(source.clone(), vec![], vec![]);
        let hash = body_of(source.clone(), vec![], vec![]);

        // Reindenting outside a protected range cannot change the normalised
        // text, so it cannot move the hash.
        let reindented = source.replace("    ", "\t\t").replace('\n', "\n  ");
        assert_eq!(
            normalize_of(reindented.clone(), vec![], vec![]),
            normalised,
            "reindenting changed the normalised body of {source:?}"
        );
        assert_eq!(body_of(reindented, vec![], vec![]), hash);
        held += 1;

        // Adding a character does change it, in both.
        let extended = format!("{source}X");
        assert_ne!(normalize_of(extended.clone(), vec![], vec![]), normalised);
        assert_ne!(body_of(extended, vec![], vec![]), hash, "a real change left the hash alone");
        moved += 1;
    }
    support::covered(
        "REQ-ANCHOR.hash_tracks_body",
        &[("a body reindented without a range", held), ("a body extended", moved)],
    );
}

/// A file name and one anchor kind of each of the three shapes.
fn anchor_ident_input() -> Schema {
    let mut variants: std::collections::BTreeMap<String, Option<Box<Schema>>> =
        std::collections::BTreeMap::new();
    variants.insert(
        "decl".into(),
        Some(Box::new(Schema::Struct {
            fields: [(
                "symbolPath".to_string(),
                Schema::Str {
                    max_len: Some(1),
                    examples: vec!["Foo".into(), "Foo::bar".into(), "TraceLean::assurance".into()],
                },
            )]
            .into_iter()
            .collect(),
        })),
    );
    variants.insert(
        "region".into(),
        Some(Box::new(Schema::Struct {
            fields: [
                ("start".to_string(), Schema::Nat { max: Some(99), edges: vec![0, 1] }),
                ("end".to_string(), Schema::Nat { max: Some(99), edges: vec![0, 1] }),
            ]
            .into_iter()
            .collect(),
        })),
    );
    variants.insert("file".into(), None);

    Schema::Struct {
        fields: [
            (
                "file".to_string(),
                Schema::Str {
                    max_len: Some(1),
                    examples: vec!["a.rs".into(), "src/b/c.lean".into(), "".into()],
                },
            ),
            ("kind".to_string(), Schema::Enum { variants }),
        ]
        .into_iter()
        .collect(),
    }
}

/// An anchor's identity, checked against the model.
///
/// @drt REQ-ANCHOR.symbol_not_line
/// @tests REQ-ANCHOR.symbol_not_line
/// @tests REQ-ANCHOR.stable_under_move
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_an_anchors_identity() {
    let scratch = harness::scratch("anchor-ident");
    let op = "REQ-ANCHOR.symbol_not_line";
    let implementation = harness::rust_runner(
        "REQ-ANCHOR",
        "symbol_not_line",
        "crates/core/src/trace/anchor.rs::anchor_ident",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Anchor",
        "TraceLean.Anchor.anchorIdent",
        op,
        &["file", "kind"],
        &scratch,
    );

    let result = run(
        op,
        &anchor_ident_input(),
        &model,
        &implementation,
        RunOptions { seed: 73, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What a link through an imprecise anchor may claim.
///
/// @drt REQ-ANCHOR.imprecise_capped
/// @tests REQ-ANCHOR.imprecise_capped
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_cap_for_an_imprecise_anchor() {
    let scratch = harness::scratch("anchor-cap");
    let op = "REQ-ANCHOR.imprecise_capped";
    let implementation = harness::rust_runner(
        "REQ-ANCHOR",
        "imprecise_capped",
        "crates/core/src/trace/anchor.rs::anchor_ceiling",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Anchor",
        "TraceLean.Anchor.anchorCeiling",
        op,
        &["precise", "claimed"],
        &scratch,
    );

    let schema = Schema::Struct {
        fields: [
            ("precise".to_string(), Schema::Bool),
            ("claimed".to_string(), Schema::simple_enum(&["L1", "L2", "L3", "L4"])),
        ]
        .into_iter()
        .collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 79, cases: 500, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The cap has two branches and four levels; a run that only ever asked the
/// precise branch would agree about a function it never made do anything.
///
/// @tests REQ-ANCHOR.imprecise_capped
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_both_branches_of_the_cap() {
    use tracelean_core::drt::gen;
    use tracelean_core::evidence::Level;
    use tracelean_core::trace::anchor::anchor_ceiling;

    let schema = Schema::Struct {
        fields: [
            ("precise".to_string(), Schema::Bool),
            ("claimed".to_string(), Schema::simple_enum(&["L1", "L2", "L3", "L4"])),
        ]
        .into_iter()
        .collect(),
    };
    let mut rng = gen::Rng::new(79);
    // `lowered` is the case with content: an imprecise anchor claiming more
    // than L1. Imprecise-and-already-L1 is indistinguishable from the precise
    // branch, so it cannot stand in for it.
    let (mut precise, mut lowered, mut already_low) = (0u64, 0u64, 0u64);
    for _ in 0..500 {
        let v = gen::value(&schema, &mut rng);
        let is_precise = v["precise"].as_bool().unwrap();
        let claimed: Level = serde_json::from_value(v["claimed"].clone()).unwrap();

        let ceiling = anchor_ceiling(is_precise, claimed);
        assert!(ceiling <= claimed, "a cap raised what was claimed");
        if is_precise {
            assert_eq!(ceiling, claimed, "a precise anchor lowered a claim");
            precise += 1;
        } else {
            assert_eq!(ceiling, Level::L1, "an imprecise anchor carried more than L1");
            if claimed == Level::L1 {
                already_low += 1;
            } else {
                lowered += 1;
            }
        }
    }
    support::covered(
        "REQ-ANCHOR.imprecise_capped",
        &[
            ("a precise anchor", precise),
            ("an imprecise anchor claiming more than L1", lowered),
            ("an imprecise anchor claiming L1", already_low),
        ],
    );
}

/// An anchor's identity has three shapes, and two of them would collide if the
/// separator were dropped.
///
/// @tests REQ-ANCHOR.symbol_not_line
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_shape_an_anchor_can_take() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::anchor::{anchor_ident, AnchorKind};

    let schema = anchor_ident_input();
    let mut rng = gen::Rng::new(73);
    let (mut decl, mut region, mut whole) = (0u64, 0u64, 0u64);
    let mut seen: std::collections::BTreeMap<String, (String, AnchorKind)> = Default::default();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let file = v["file"].as_str().unwrap().to_string();
        let kind: AnchorKind = serde_json::from_value(v["kind"].clone()).unwrap();

        let ident = anchor_ident(file.clone(), kind.clone());
        // Identity: two different anchors never share an identifier. This is
        // the whole content of `symbol_not_line` — an identifier that collided
        // would let one claim be read as another's.
        if let Some((seen_file, seen_kind)) = seen.get(&ident) {
            assert_eq!(
                (seen_file, seen_kind),
                (&file, &kind),
                "two different anchors share the identifier `{ident}`"
            );
        } else {
            seen.insert(ident, (file, kind.clone()));
        }

        match kind {
            AnchorKind::Decl { .. } => decl += 1,
            AnchorKind::Region { .. } => region += 1,
            AnchorKind::File => whole += 1,
        }
    }
    support::covered(
        "REQ-ANCHOR.symbol_not_line",
        &[
            ("an anchor on a declaration", decl),
            ("an anchor on a region", region),
            ("an anchor on a whole file", whole),
        ],
    );
}
