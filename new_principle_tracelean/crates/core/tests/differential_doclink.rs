//! Documentation drift, checked against the model.
//!
//! Three states and one lookup — small enough that a unit test looks like
//! enough, which is exactly why it is worth generating: the interesting cases
//! are a target absent from the map, a recorded hash that is empty, and a hash
//! that matches by accident, and all three are easy to forget to write down.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn hash() -> Schema {
    Schema::Str { max_len: None, examples: vec!["".into(), "h1".into(), "h2".into()] }
}

fn target() -> Schema {
    Schema::Str {
        max_len: None,
        examples: vec!["REQ-EVID".into(), "REQ-CMD".into(), "src/a.rs::f".into()],
    }
}

fn input_schema() -> Schema {
    strukt(&[
        (
            "link",
            strukt(&[
                ("file", Schema::Str { max_len: None, examples: vec!["docs/x.md".into()] }),
                ("target", target()),
                ("recordedHash", hash()),
            ]),
        ),
        (
            "current",
            Schema::List {
                inner: Box::new(Schema::Tuple { items: vec![target(), hash()] }),
                max_len: Some(3),
            },
        ),
    ])
}

/// @drt REQ-DOCLINK.hash_moves_review
/// @tests REQ-DOCLINK.hash_moves_review
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_document_state() {
    let scratch = harness::scratch("doclink");
    let op = "REQ-DOCLINK.hash_moves_review";
    let implementation = harness::rust_runner(
        "REQ-DOCLINK",
        "hash_moves_review",
        "crates/core/src/trace/doclink.rs::state",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.DocLink",
        "TraceLean.DocLink.docState",
        op,
        &["link", "current"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 5, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// All three states must occur, or agreement says nothing about the ones that
/// matter.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_all_three_states() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::doclink::{state, DocLink, State};

    let schema = input_schema();
    let mut rng = gen::Rng::new(5);
    let mut seen: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let link: DocLink = serde_json::from_value(v["link"].clone()).unwrap();
        let current: Vec<(String, String)> = serde_json::from_value(v["current"].clone()).unwrap();
        *seen
            .entry(match state(link, current) {
                State::Current => "current",
                State::InReview { .. } => "inReview",
                State::Dangling => "dangling",
            })
            .or_default() += 1;
    }
    let counts: Vec<(&str, u64)> = ["current", "inReview", "dangling"]
        .iter()
        .map(|name| (*name, seen.get(name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-DOCLINK.hash_moves_review", &counts);
}

/// Only a dangling target blocks.
///
/// Two states out of three must not block, and the one that must is the one a
/// tempting simplification would collapse into its neighbours.
///
/// @drt REQ-DOCLINK.review_is_not_error
/// @tests REQ-DOCLINK.review_is_not_error
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_blocks() {
    let scratch = harness::scratch("doclink-blocks");
    let op = "REQ-DOCLINK.review_is_not_error";
    let implementation = harness::rust_runner(
        "REQ-DOCLINK",
        "review_is_not_error",
        "crates/core/src/trace/doclink.rs::state_blocks",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.DocLink",
        "TraceLean.DocLink.docBlocks",
        op,
        &["state"],
        &scratch,
    );

    let mut variants: std::collections::BTreeMap<String, Option<Box<Schema>>> = Default::default();
    variants.insert("current".into(), None);
    variants.insert("dangling".into(), None);
    variants.insert(
        "inReview".into(),
        Some(Box::new(strukt(&[("was", hash()), ("now", hash())]))),
    );
    let schema = strukt(&[("state", Schema::Enum { variants })]);

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 6, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What a document declares, checked against the model.
///
/// @drt REQ-DOCLINK.declares_target
/// @tests REQ-DOCLINK.declares_target
/// @tests REQ-DOCLINK.records_hash
/// @tests REQ-DOCLINK.decisions_exempt
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_document_declares() {
    let scratch = harness::scratch("declared");
    let op = "REQ-DOCLINK.declares_target";
    let implementation = harness::rust_runner(
        "REQ-DOCLINK",
        "declares_target",
        "crates/core/src/trace/doclink.rs::declared_in",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.DocLink",
        "TraceLean.DocLink.declaredIn",
        op,
        &["lines"],
        &scratch,
    );

    let line = Schema::Str {
        // Whole documents, because the fence has to open and close for anything
        // to be declared at all. Single lines drawn independently produce a
        // well-formed document about once in a thousand draws.
        max_len: Some(0),
        examples: vec![
            "---\ndescribes: a.rs::f\ndescribed_hash: h1\n---".into(),
            "---\ndescribes: [a.rs::f, b.rs::g]\ndescribed_hash:\n  a.rs::f: h1\n  b.rs::g: h2\n---"
                .into(),
            "---\ndescribes: [a.rs::f, b.rs::g]\ndescribed_hash: h1\n---".into(),
            "---\ndescribes: [REQ-X]\n---".into(),
            "---\ndescribes:\ndescribed_hash: h1\n---".into(),
            "---\nadr: 11\naffects: [REQ-X]\n---".into(),
            "---\ndescribes: \"a.rs::f\"\ndescribed_hash:\n  a.rs::f: h1\n---".into(),
            "---\ndescribed_hash:\n  a.rs::f: h1\ndescribes: a.rs::f\n---".into(),
            "---\ndescribes: []\n---".into(),
            "---\ndescribes: a.rs::f".into(),
            "---".into(),
            "describes: a.rs::f".into(),
            "  a.rs::f: h1".into(),
            "# prose".into(),
            "".into(),
        ],
    };

    let schema = Schema::Struct {
        fields: [(
            "lines".to_string(),
            Schema::List { inner: Box::new(line), max_len: Some(3) },
        )]
        .into_iter()
        .collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 43, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The frontmatter a person would re-record, checked against the model.
///
/// @drt REQ-DOCLINK.confirmation_is_human
/// @tests REQ-DOCLINK.confirmation_is_human
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_confirming_would_write() {
    let scratch = harness::scratch("recorded");
    let op = "REQ-DOCLINK.confirmation_is_human";
    let implementation = harness::rust_runner(
        "REQ-DOCLINK",
        "confirmation_is_human",
        "crates/core/src/trace/doclink.rs::recorded_of",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.DocLink",
        "TraceLean.DocLink.recordedFrontmatter",
        op,
        &["targets", "current"],
        &scratch,
    );

    let target = Schema::Str {
        max_len: Some(1),
        examples: vec!["a.rs::f".into(), "b.rs::g".into(), "REQ-X".into()],
    };
    let schema = Schema::Struct {
        fields: [
            (
                "targets".to_string(),
                Schema::List { inner: Box::new(target.clone()), max_len: Some(3) },
            ),
            (
                "current".to_string(),
                Schema::List {
                    inner: Box::new(Schema::Tuple {
                        items: vec![
                            target,
                            Schema::Str {
                                max_len: Some(1),
                                examples: vec!["h1".into(), "h2".into()],
                            },
                        ],
                    }),
                    max_len: Some(3),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 47, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Only a dangling target blocks, and the run has to reach all three states to
/// say so. Two of them not blocking is the whole content of the clause: a
/// document in review is a document somebody has to re-read, not a build
/// failure, and collapsing it into either neighbour would be invisible in a run
/// that never produced it.
///
/// @tests REQ-DOCLINK.review_is_not_error
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_state_and_only_one_of_them_blocks() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::doclink::{state_blocks, State};

    let mut variants: std::collections::BTreeMap<String, Option<Box<Schema>>> = Default::default();
    variants.insert("current".into(), None);
    variants.insert("dangling".into(), None);
    variants.insert(
        "inReview".into(),
        Some(Box::new(strukt(&[("was", hash()), ("now", hash())]))),
    );
    let schema = strukt(&[("state", Schema::Enum { variants })]);

    let mut rng = gen::Rng::new(6);
    let (mut current, mut dangling, mut review, mut moved) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&schema, &mut rng);
        let state: State = serde_json::from_value(v["state"].clone()).unwrap();
        let blocks = state_blocks(state.clone());
        match state {
            State::Current => {
                assert!(!blocks, "a current document blocked");
                current += 1;
            }
            State::Dangling => {
                assert!(blocks, "a document describing nothing did not block");
                dangling += 1;
            }
            State::InReview { was, now } => {
                assert!(!blocks, "a document in review blocked");
                review += 1;
                // The case worth separating: a review where the hash really
                // moved, rather than one where both halves happen to be equal.
                if was != now {
                    moved += 1;
                }
            }
        }
    }
    support::covered(
        "REQ-DOCLINK.review_is_not_error",
        &[
            ("a current document", current),
            ("a dangling document", dangling),
            ("a document in review", review),
            ("a document whose hash actually moved", moved),
        ],
    );
}

/// The frontmatter a person would re-record, asked where recording is not
/// trivial: several targets, a repeated one, and a target nothing currently
/// hashes.
///
/// The repeat is the case that found a real bug — `state` resolved a repeated
/// target to the first entry and an earlier `recorded_of` resolved it to the
/// last, so one function called a document current while the other wrote a
/// different hash for it.
///
/// @tests REQ-DOCLINK.confirmation_is_human
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_frontmatter_worth_writing() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::doclink::recorded_of;

    let target = Schema::Str {
        max_len: Some(1),
        examples: vec!["a.rs::f".into(), "b.rs::g".into(), "REQ-X".into()],
    };
    let schema = Schema::Struct {
        fields: [
            (
                "targets".to_string(),
                Schema::List { inner: Box::new(target.clone()), max_len: Some(3) },
            ),
            (
                "current".to_string(),
                Schema::List {
                    inner: Box::new(Schema::Tuple {
                        items: vec![
                            target,
                            Schema::Str { max_len: Some(1), examples: vec!["h1".into(), "h2".into()] },
                        ],
                    }),
                    max_len: Some(3),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let mut rng = gen::Rng::new(47);
    let (mut several, mut repeated, mut unknown, mut none) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let targets: Vec<String> = serde_json::from_value(v["targets"].clone()).unwrap();
        let current: Vec<(String, String)> = serde_json::from_value(v["current"].clone()).unwrap();

        let written = recorded_of(targets.clone(), current.clone());
        // Every target reaches the frontmatter, whether or not anything
        // currently hashes it: a target silently dropped here is a document
        // that would read as describing less than it does.
        for target in &targets {
            assert!(written.contains(target), "`{target}` did not reach the frontmatter");
        }

        if targets.is_empty() {
            none += 1;
        } else if targets.len() > 1 {
            several += 1;
        }
        let unique: std::collections::BTreeSet<&String> = targets.iter().collect();
        if unique.len() < targets.len() {
            repeated += 1;
        }
        if targets.iter().any(|t| !current.iter().any(|(name, _)| name == t)) {
            unknown += 1;
        }
    }
    support::covered(
        "REQ-DOCLINK.confirmation_is_human",
        &[
            ("more than one target", several),
            ("a target named twice", repeated),
            ("a target nothing currently hashes", unknown),
            ("no targets at all", none),
        ],
    );
}
