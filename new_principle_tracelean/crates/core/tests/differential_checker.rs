//! The checker's decisions, checked against the model.
//!
//! Two separate things, and both are properties a reader has to be able to
//! trust without reading the scanner: what a clause's claimed roles imply, and
//! what each kind of finding means.
//!
//! The coverage decision is worth generating because sixteen role
//! combinations is more than anyone writes fixtures for, and the interesting
//! ones are the near-misses — modelled and implemented but unbound, implemented
//! and tested but unmodelled.

mod harness;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn role() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for name in ["models", "specifies", "implements", "tests", "drt", "proves", "pins"] {
        variants.insert(name.into(), None);
    }
    Schema::Enum { variants }
}

fn coverage_input() -> Schema {
    strukt(&[
        ("roles", Schema::List { inner: Box::new(role()), max_len: Some(5) }),
        ("exempt", Schema::Bool),
        ("structural", Schema::Bool),
    ])
}

/// @drt REQ-CHECK.exactly_once
/// @tests REQ-CHECK.exactly_once
/// @tests REQ-CHECK.structural_is_not_exempt
/// @tests REQ-CHECK.unbound_reported
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_roles_imply() {
    let scratch = harness::scratch("coverage");
    let op = "REQ-CHECK.exactly_once";
    let implementation = harness::rust_runner(
        "REQ-CHECK",
        "exactly_once",
        "crates/core/src/trace/checker.rs::coverage_kinds",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Checker",
        "TraceLean.Checker.coverageKinds",
        op,
        &["roles", "exempt", "structural"],
        &scratch,
    );

    let result = run(
        op,
        &coverage_input(),
        &model,
        &implementation,
        RunOptions { seed: 41, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

fn kind() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for name in [
        "dangling", "danglingRefines", "refinesCycle", "duplicateId", "unmodeled",
        "unimplemented", "unbound", "untested", "contested", "unsoundExemption",
        "unsoundQualifier", "malformed", "imprecise", "severalModels", "severalSpecs",
        "severalPins",
    ] {
        variants.insert(name.into(), None);
    }
    Schema::Enum { variants }
}

/// @drt REQ-CHECK.named_kinds
/// @tests REQ-CHECK.named_kinds
/// @tests REQ-CHECK.progress_not_fault
/// @tests REQ-CHECK.severity_policy
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_each_kind_means() {
    let scratch = harness::scratch("kinds");
    let op = "REQ-CHECK.named_kinds";
    let implementation = harness::rust_runner(
        "REQ-CHECK",
        "named_kinds",
        "crates/core/src/trace/checker.rs::facts",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Checker",
        "TraceLean.Checker.facts",
        op,
        &["kind"],
        &scratch,
    );

    let result = run(
        op,
        &strukt(&[("kind", kind())]),
        &model,
        &implementation,
        RunOptions { seed: 42, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

fn qualifier() -> Schema {
    let reason = Schema::Option {
        inner: Box::new(Schema::Str { max_len: Some(0), examples: vec!["platform".into()] }),
    };
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    variants.insert("partial".into(), Some(Box::new(strukt(&[("reason", reason.clone())]))));
    variants.insert(
        "exempt".into(),
        Some(Box::new(strukt(&[
            ("reason", reason.clone()),
            (
                "judgedBy",
                Schema::Option {
                    inner: Box::new(Schema::Str { max_len: Some(0), examples: vec!["ana".into()] }),
                },
            ),
            (
                "expires",
                Schema::Option {
                    inner: Box::new(Schema::Str {
                        max_len: Some(0),
                        examples: vec!["2027".into(), "2027-01-01".into()],
                    }),
                },
            ),
        ]))),
    );
    variants.insert(
        "nondeterministic".into(),
        Some(Box::new(strukt(&[("reason", reason)]))),
    );
    Schema::Enum { variants }
}

/// @drt REQ-CHECK.qualifier_soundness
/// @tests REQ-CHECK.qualifier_soundness
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_unsound_qualifiers() {
    let scratch = harness::scratch("qualifiers");
    let op = "REQ-CHECK.qualifier_soundness";
    let implementation = harness::rust_runner(
        "REQ-CHECK",
        "qualifier_soundness",
        "crates/core/src/trace/checker.rs::qualifier_kinds",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Checker",
        "TraceLean.Checker.qualifierKinds",
        op,
        &["qualifier", "today"],
        &scratch,
    );
    // A date before, on and after the `until`s the qualifiers carry, and none.
    let today = Schema::Option {
        inner: Box::new(Schema::Str {
            max_len: Some(0),
            examples: vec!["2026-10-11".into(), "2027-01-01".into(), "2028".into()],
        }),
    };

    let result = run(
        op,
        &strukt(&[("qualifier", Schema::Option { inner: Box::new(qualifier()) }), ("today", today)]),
        &model,
        &implementation,
        RunOptions { seed: 43, cases: 1_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The two properties stated directly: exactly one chain finding per clause,
/// and nothing that describes progress ever blocks a build.
///
/// @tests REQ-CHECK.exactly_once
/// @tests REQ-CHECK.progress_not_fault
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn at_most_one_chain_finding_and_no_progress_ever_blocks() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::annotation::Role;
    use tracelean_core::trace::checker::{coverage_kinds, facts, Kind};

    let all = [
        Kind::Dangling, Kind::DanglingRefines, Kind::RefinesCycle, Kind::DuplicateId,
        Kind::Unmodeled, Kind::Unimplemented, Kind::Unbound, Kind::Untested, Kind::Contested,
        Kind::UnsoundExemption, Kind::UnsoundQualifier, Kind::Malformed, Kind::Imprecise,
        Kind::SeveralModels, Kind::SeveralSpecs, Kind::SeveralPins,
    ];
    for kind in all {
        let f = facts(kind);
        assert!(
            !(f.progress && f.blocks_by_default),
            "{kind:?} describes progress and blocks a build"
        );
    }

    let schema = coverage_input();
    let mut rng = gen::Rng::new(41);
    let mut seen: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let roles: Vec<Role> = serde_json::from_value(v["roles"].clone()).unwrap();
        let exempt = v["exempt"].as_bool().unwrap();
        let structural = v["structural"].as_bool().unwrap();

        let kinds = coverage_kinds(roles, exempt, structural);
        let chain = kinds
            .iter()
            .filter(|k| matches!(k, Kind::Unmodeled | Kind::Unimplemented | Kind::Unbound))
            .count();
        assert!(chain <= 1, "two chain findings for one clause: {kinds:?}");
        for k in kinds {
            *seen.entry(format!("{k:?}")).or_default() += 1;
        }
    }
    let counts: Vec<(&str, u64)> = ["Unmodeled", "Unimplemented", "Unbound", "Untested"]
        .iter()
        .map(|name| (*name, seen.get(*name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-CHECK.exactly_once", &counts);
}

fn crowded_input() -> Schema {
    strukt(&[("roles", Schema::List { inner: Box::new(role()), max_len: Some(6) })])
}

/// @drt REQ-CHECK.one_of_each_role
/// @tests REQ-CHECK.one_of_each_role
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_which_roles_are_crowded() {
    let scratch = harness::scratch("crowded");
    let op = "REQ-CHECK.one_of_each_role";
    let implementation = harness::rust_runner(
        "REQ-CHECK",
        "one_of_each_role",
        "crates/core/src/trace/checker.rs::crowded_kinds",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Checker",
        "TraceLean.Checker.crowdedKinds",
        op,
        &["roles"],
        &scratch,
    );

    let result = run(
        op,
        &crowded_input(),
        &model,
        &implementation,
        RunOptions { seed: 44, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The generated role lists reach every outcome the clause names, and the
/// implementation reports a role exactly when it occurs twice or more.
///
/// @tests REQ-CHECK.one_of_each_role
#[test]
fn every_crowded_role_is_reached_and_counted() {
    use tracelean_core::drt::gen;
    use tracelean_core::trace::annotation::Role;
    use tracelean_core::trace::checker::{crowded_kinds, Kind};

    let schema = crowded_input();
    let mut rng = gen::Rng::new(44);
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let roles: Vec<Role> = serde_json::from_value(v["roles"].clone()).unwrap();
        let kinds = crowded_kinds(roles.clone());
        for (role, kind) in [
            (Role::Models, Kind::SeveralModels),
            (Role::Specifies, Kind::SeveralSpecs),
            (Role::Pins, Kind::SeveralPins),
        ] {
            let many = roles.iter().filter(|r| **r == role).count() > 1;
            assert_eq!(kinds.contains(&kind), many, "{roles:?} -> {kinds:?}");
        }
        if kinds.is_empty() {
            *seen.entry("none crowded".into()).or_default() += 1;
        }
        for k in kinds {
            *seen.entry(format!("{k:?}")).or_default() += 1;
        }
    }
    let counts: Vec<(&str, u64)> = ["none crowded", "SeveralModels", "SeveralSpecs", "SeveralPins"]
        .iter()
        .map(|name| (*name, seen.get(*name).copied().unwrap_or(0)))
        .collect();
    support::covered("REQ-CHECK.one_of_each_role", &counts);
}
