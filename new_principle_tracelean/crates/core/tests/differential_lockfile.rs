//! The order the lockfile writes links in, checked against the model.
//!
//! The lockfile is committed, so a change to it is meant to mean a change to
//! the project. That holds only if the same tree always produces the same
//! bytes, and the one part of those bytes this project chooses is the order of
//! the link list.
//!
//! Generated links share prefixes deliberately: an order that compared only the
//! first few fields would look correct on distinct links and produce a
//! different file on every run for links that agree up to their hashes.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Tiny alphabets, so generated links collide on their leading fields and the
/// later ones have to break the tie. `max_len: 0` makes the generator's random
/// branch produce the empty string rather than a fresh one, which is what keeps
/// the collisions frequent.
fn small(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

/// The leading fields draw from one value each, so links routinely agree all
/// the way down to their hashes — the case a partial order would leave to
/// chance, and the reason this order goes through every field.
fn lock_link() -> Schema {
    strukt(&[
        ("role", small(&["models"])),
        ("req", small(&["REQ-A"])),
        ("clause", Schema::Option { inner: Box::new(small(&["one"])) }),
        ("file", small(&["a.rs"])),
        ("anchor", small(&["a.rs::f"])),
        ("body_hash", small(&["h1", "h2"])),
        ("link_hash", small(&["k1", "k2"])),
        ("qualifier", Schema::Option { inner: Box::new(small(&["partial", "exempt"])) }),
    ])
}

fn input_schema() -> Schema {
    strukt(&[("links", Schema::List { inner: Box::new(lock_link()), max_len: Some(5) })])
}

/// @drt REQ-LOCK.deterministic_bytes
/// @tests REQ-LOCK.deterministic_bytes
/// @tests ARCH-DETERMINISM.stable_ordering
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_order_links_are_written_in() {
    let scratch = harness::scratch("lockfile");
    let op = "REQ-LOCK.deterministic_bytes";
    let implementation = harness::rust_runner(
        "REQ-LOCK",
        "deterministic_bytes",
        "crates/core/src/trace/lockfile.rs::ordered_links",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lockfile",
        "TraceLean.Lockfile.orderedLinks",
        op,
        &["links"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 12, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Determinism stated directly: ordering is idempotent, and it does not depend
/// on the order the links arrived in.
///
/// @tests REQ-LOCK.deterministic_bytes
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn the_order_is_a_function_of_the_links_alone() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::lockfile::{ordered_links, LockLink};

    let schema = input_schema();
    let mut rng = gen::Rng::new(12);
    let mut tied = 0;
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let links: Vec<LockLink> = serde_json::from_value(v["links"].clone()).unwrap();

        let sorted = ordered_links(links.clone());
        assert_eq!(sorted, ordered_links(sorted.clone()), "ordering is not idempotent");

        let mut reversed = links.clone();
        reversed.reverse();
        assert_eq!(
            sorted,
            ordered_links(reversed),
            "the order depends on the order the links arrived in"
        );

        // Links agreeing on everything but their hashes are the case a partial
        // order would leave to chance.
        for pair in sorted.windows(2) {
            if pair[0].role == pair[1].role
                && pair[0].req == pair[1].req
                && pair[0].clause == pair[1].clause
                && pair[0].file == pair[1].file
                && pair[0].anchor == pair[1].anchor
            {
                tied += 1;
            }
        }
    }
    support::covered(
        "REQ-LOCK.deterministic_bytes",
        &[("two links agreeing up to their hashes", tied)],
    );
}

/// A version this build does not know, checked against the model.
///
/// @drt REQ-LOCK.version_stamped
/// @tests REQ-LOCK.version_stamped
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_versions_are_readable() {
    let scratch = harness::scratch("lock-version");
    let op = "REQ-LOCK.version_stamped";
    let implementation = harness::rust_runner(
        "REQ-LOCK",
        "version_stamped",
        "crates/core/src/trace/lockfile.rs::version_verdict",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lockfile",
        "TraceLean.Lockfile.versionVerdict",
        op,
        &["version"],
        &scratch,
    );

    let schema = Schema::Struct {
        fields: [(
            "version".to_string(),
            Schema::Option {
                inner: Box::new(Schema::Nat { max: Some(4), edges: vec![0, 1, 2] }),
            },
        )]
        .into_iter()
        .collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 11, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Evidence passes through rendering unchanged, checked against the model.
///
/// The model is the identity, which is the whole claim. A run that agreed would
/// be worthless if the generator only ever produced the empty list, so the
/// coverage check below is the part that makes this test say anything.
///
/// @drt REQ-LOCK.evidence_preserved
/// @tests REQ-LOCK.evidence_preserved
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_that_evidence_passes_through() {
    let scratch = harness::scratch("lock-evidence");
    let op = "REQ-LOCK.evidence_preserved";
    let implementation = harness::rust_runner(
        "REQ-LOCK",
        "evidence_preserved",
        "crates/core/src/trace/lockfile.rs::carried_evidence",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Lockfile",
        "TraceLean.Lockfile.carriedEvidence",
        op,
        &["evidence"],
        &scratch,
    );

    let result = run(
        op,
        &evidence_list_schema(),
        &model,
        &implementation,
        RunOptions { seed: 13, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// An evidence list with every backend represented.
fn evidence_list_schema() -> Schema {
    let names = |values: &[&str]| Schema::Str {
        max_len: Some(1),
        examples: values.iter().map(|v| (*v).to_string()).collect(),
    };
    let mut detail: std::collections::BTreeMap<String, Option<Box<Schema>>> =
        std::collections::BTreeMap::new();
    detail.insert(
        "judge".to_string(),
        Some(Box::new(Schema::Struct {
            fields: [
                ("verdict".to_string(), names(&["accepted", "rejected"])),
                ("judgedBy".to_string(), names(&["ana", "bo"])),
                ("promptVersion".to_string(), names(&["v1"])),
            ]
            .into_iter()
            .collect(),
        })),
    );
    detail.insert(
        "drt".to_string(),
        Some(Box::new(Schema::Struct {
            fields: [
                ("seed".to_string(), Schema::Nat { max: Some(9), edges: vec![0, 7] }),
                ("cases".to_string(), Schema::Nat { max: Some(9), edges: vec![0, 2] }),
                ("op".to_string(), names(&["REQ-X.one"])),
            ]
            .into_iter()
            .collect(),
        })),
    );
    detail.insert(
        "proof".to_string(),
        Some(Box::new(Schema::Struct {
            fields: [
                ("theoremName".to_string(), names(&["t1"])),
                ("toolchain".to_string(), names(&["lean-4.12.0"])),
            ]
            .into_iter()
            .collect(),
        })),
    );

    let record = Schema::Struct {
        fields: [
            (
                "key".to_string(),
                Schema::Struct {
                    fields: [
                        ("reqId".to_string(), names(&["REQ-X", "REQ-Y"])),
                        (
                            "clause".to_string(),
                            Schema::Option { inner: Box::new(names(&["one", "two"])) },
                        ),
                        (
                            "bond".to_string(),
                            Schema::simple_enum(&["requirementModel", "modelImpl", "modelProof"]),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                },
            ),
            ("level".to_string(), Schema::simple_enum(&["L1", "L2", "L3", "L4"])),
            ("detail".to_string(), Schema::Enum { variants: detail }),
            ("linkHash".to_string(), names(&["link1", "link2"])),
            (
                "inputs".to_string(),
                Schema::List {
                    inner: Box::new(Schema::Tuple {
                        items: vec![
                            // Exactly the names `required_inputs` asks for.
                            // Without them no generated record can ever be
                            // reproducible, and the agreement on
                            // `record_reproducible` would be about the failing
                            // branch only — which is what the `Vacuous` verdict
                            // reported when the alphabet was `["model", "code"]`.
                            // A name no backend requires is still reached:
                            // `requirement` is not required of a proof, and
                            // `implementation` is not required of a judgement.
                            names(&["model", "implementation", "requirement", "toolchain"]),
                            names(&["h1", "h2"]),
                        ],
                    }),
                    // Long enough that carrying *both* names a backend requires
                    // is ordinary rather than lucky. At four the happy path
                    // occurred 47 times in 2000; the floor is a statement about
                    // how often the law must be asked, so the generator moved
                    // rather than the floor.
                    max_len: Some(8),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    Schema::Struct {
        fields: [(
            "evidence".to_string(),
            Schema::List { inner: Box::new(record), max_len: Some(3) },
        )]
        .into_iter()
        .collect(),
    }
}

/// Rendering reads nothing and every backend's evidence reaches the artefact.
///
/// @tests REQ-LOCK.pure_render
/// @structural REQ-LOCK.pure_render reason="an absence of filesystem access, which a model of the function could not distinguish from its presence"
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_backend_and_rendering_reads_nothing() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::lockfile::carried_evidence;
    use tracelean_core::trace::record::{Detail, Evidence};

    let schema = evidence_list_schema();
    let mut rng = gen::Rng::new(13);
    let (mut judge, mut drt, mut proof, mut empty) = (0, 0, 0, 0);
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let records: Vec<Evidence> = serde_json::from_value(value["evidence"].clone())
            .expect("the schema generates what the type parses");
        if records.is_empty() {
            empty += 1;
        }
        // Rendering is a function of its argument. Called on a default index
        // with no filesystem anywhere near it, it still returns what it was
        // given — which is what `pure_render` and `evidence_preserved` say
        // together.
        assert_eq!(carried_evidence(records.clone()), records);
        for record in records {
            match record.detail {
                Detail::Judge { .. } => judge += 1,
                Detail::Drt { .. } => drt += 1,
                Detail::Proof { .. } => proof += 1,
            }
        }
    }
    support::covered(
        "REQ-LOCK.evidence_preserved",
        &[
            ("a judged record", judge),
            ("a differentially tested record", drt),
            ("a proved record", proof),
            ("no evidence at all", empty),
        ],
    );
}

/// A change in the serialised form corresponds to a change in the repository,
/// and nothing else does.
///
/// Both directions matter and they fail differently. If reordering the scan
/// changed the bytes, every diff would carry noise and people would stop
/// reading them — which is worse than not committing the index at all. If
/// changing a hash left the bytes alone, the artefact would say a claim still
/// holds after the thing it is about has moved.
///
/// @tests REQ-LOCK.diff_is_meaningful
/// @structural REQ-LOCK.diff_is_meaningful reason="a claim relating two serialisations to two repository states, not a property of one call"
/// @tests REQ-LOCK.deterministic_bytes
#[test]
fn the_bytes_move_when_the_repository_moves_and_not_otherwise() {
    use tracelean_core::trace::lockfile::{ordered_links, to_bytes, LockLink, Lockfile, VERSION};

    let link = |req: &str, body: &str| LockLink {
        role: "implements".into(),
        req: req.into(),
        clause: Some("one".into()),
        file: "a.rs".into(),
        anchor: "a.rs::f".into(),
        body_hash: body.into(),
        link_hash: "l1".into(),
        qualifier: None,
    };
    let lockfile = |links: Vec<LockLink>| Lockfile {
        version: VERSION,
        requirements: Default::default(),
        links: ordered_links(links),
        evidence: vec![],
    };

    let forwards = lockfile(vec![link("REQ-A", "h1"), link("REQ-B", "h2")]);
    let backwards = lockfile(vec![link("REQ-B", "h2"), link("REQ-A", "h1")]);
    assert_eq!(
        to_bytes(&forwards),
        to_bytes(&backwards),
        "the order the scan happened to walk the tree in reached the artefact"
    );

    let moved = lockfile(vec![link("REQ-A", "h9"), link("REQ-B", "h2")]);
    assert_ne!(
        to_bytes(&forwards),
        to_bytes(&moved),
        "a body hash changed and the committed artefact did not"
    );

    // And it is a function: the same value twice is the same bytes twice.
    assert_eq!(to_bytes(&forwards), to_bytes(&forwards));
}

/// Whether a record carries what would reproduce and invalidate it, checked
/// against the model.
///
/// A record that named no inputs could never be invalidated, so it would be
/// believed forever — which is the failure this whole system exists to prevent,
/// arriving through the one door nobody watches.
///
/// @drt REQ-EVID.record_reproducible
/// @tests REQ-EVID.record_reproducible
/// @tests REQ-STALE.inputs_identified
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_whether_a_record_can_be_reproduced() {
    let scratch = harness::scratch("reproducibility");
    let op = "REQ-EVID.record_reproducible";
    let implementation = harness::rust_runner(
        "REQ-EVID",
        "record_reproducible",
        "crates/core/src/trace/record.rs::reproducibility",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Record",
        "TraceLean.Record.reproducibility",
        op,
        &["record"],
        &scratch,
    );

    let Schema::Struct { fields } = evidence_list_schema() else { panic!("a struct") };
    let Schema::List { inner, .. } = fields["evidence"].clone() else { panic!("a list") };
    let schema = Schema::Struct {
        fields: [("record".to_string(), *inner)].into_iter().collect(),
    };

    let result = run(
        op,
        &schema,
        &model,
        &implementation,
        RunOptions { seed: 113, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// All three answers, and the one that matters asked of all three backends.
///
/// `Reproducible` is the comfortable answer and the easy one to reach; the two
/// findings are what the law is for. A record that named no inputs would never
/// go stale and so would be believed forever, so `MissingInput` has to occur —
/// and it has to occur for every backend, because each names different inputs
/// and a match arm that forgot one would still pass a run that never reached it.
///
/// @tests REQ-EVID.record_reproducible
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_both_ways_a_record_fails_to_be_reproducible() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::record::{reproducibility, Detail, Evidence, Reproducibility};

    let Schema::Struct { fields } = evidence_list_schema() else { panic!("a struct") };
    let Schema::List { inner, .. } = fields["evidence"].clone() else { panic!("a list") };
    let schema = Schema::Struct {
        fields: [("record".to_string(), *inner)].into_iter().collect(),
    };

    let mut rng = gen::Rng::new(113);
    let (mut fine, mut incomplete, mut missing) = (0u64, 0u64, 0u64);
    let mut backends_missing_an_input = std::collections::BTreeSet::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let record: Evidence = serde_json::from_value(v["record"].clone()).unwrap();
        let backend = match record.detail {
            Detail::Judge { .. } => "judge",
            Detail::Drt { .. } => "drt",
            Detail::Proof { .. } => "proof",
        };
        match reproducibility(record) {
            Reproducibility::Reproducible => fine += 1,
            Reproducibility::Incomplete { field } => {
                assert!(!field.is_empty(), "a finding that does not name its field");
                incomplete += 1;
            }
            Reproducibility::MissingInput { name } => {
                assert!(!name.is_empty(), "a finding that does not name its input");
                backends_missing_an_input.insert(backend);
                missing += 1;
            }
        }
    }
    assert_eq!(
        backends_missing_an_input.len(),
        3,
        "only {backends_missing_an_input:?} were ever asked for a missing input"
    );
    support::covered(
        "REQ-EVID.record_reproducible",
        &[
            ("a record that can be reproduced", fine),
            ("a record missing a field its method needs", incomplete),
            ("a record that does not name an input it depends on", missing),
        ],
    );
}
