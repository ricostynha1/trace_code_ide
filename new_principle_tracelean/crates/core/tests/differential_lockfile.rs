mod harness;

use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn small(examples: &[&str]) -> Schema {
    Schema::Str {
        max_len: Some(0),
        examples: examples.iter().map(|s| s.to_string()).collect(),
    }
}

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
    let op = "REQ-LOCK.deterministic_bytes";

    harness::agree(op, &input_schema(), 12, 2_000);
}

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
    harness::covered(
        "REQ-LOCK.deterministic_bytes",
        &[("two links agreeing up to their hashes", tied)],
    );
}

/// @drt REQ-LOCK.version_stamped
/// @tests REQ-LOCK.version_stamped
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_versions_are_readable() {
    let op = "REQ-LOCK.version_stamped";

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

    harness::agree(op, &schema, 11, 2_000);
}

/// @drt REQ-LOCK.evidence_preserved
/// @tests REQ-LOCK.evidence_preserved
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_that_evidence_passes_through() {
    let op = "REQ-LOCK.evidence_preserved";

    harness::agree(op, &evidence_list_schema(), 13, 2_000);
}

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
                            names(&["model", "implementation", "requirement", "toolchain"]),
                            names(&["h1", "h2"]),
                        ],
                    }),
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
        assert_eq!(carried_evidence(records.clone()), records);
        for record in records {
            match record.detail {
                Detail::Judge { .. } => judge += 1,
                Detail::Drt { .. } => drt += 1,
                Detail::Proof { .. } => proof += 1,
            }
        }
    }
    harness::covered(
        "REQ-LOCK.evidence_preserved",
        &[
            ("a judged record", judge),
            ("a differentially tested record", drt),
            ("a proved record", proof),
            ("no evidence at all", empty),
        ],
    );
}

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

    assert_eq!(to_bytes(&forwards), to_bytes(&forwards));
}

/// @drt REQ-EVID.record_reproducible
/// @tests REQ-EVID.record_reproducible
/// @tests REQ-STALE.inputs_identified
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_whether_a_record_can_be_reproduced() {
    let op = "REQ-EVID.record_reproducible";

    let Schema::Struct { fields } = evidence_list_schema() else { panic!("a struct") };
    let Schema::List { inner, .. } = fields["evidence"].clone() else { panic!("a list") };
    let schema = Schema::Struct {
        fields: [("record".to_string(), *inner)].into_iter().collect(),
    };

    harness::agree(op, &schema, 113, 2_000);
}

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
    harness::covered(
        "REQ-EVID.record_reproducible",
        &[
            ("a record that can be reproduced", fine),
            ("a record missing a field its method needs", incomplete),
            ("a record that does not name an input it depends on", missing),
        ],
    );
}
