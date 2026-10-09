//! Suppressing the editor's own writes, checked against the model.
//!
//! The editor writes into the tree it observes, so its own writes come back as
//! observations. The failure modes are opposite and both silent: suppress too
//! much and a real external change disappears; suppress too little and the
//! editor fights itself. The model pins the consumable semantics — one
//! suppression, one observation, first match — and random pending sets with
//! repeated paths and contents are what tell the two implementations apart.

mod harness;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

mod support;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

// Deliberately tiny alphabets. A suppression only fires when a pending write
// matches on both path and content, so wide strings would generate two thousand
// misses and check nothing.
fn path() -> Schema {
    Schema::Str { max_len: Some(1), examples: vec!["a.rs".into(), "b.rs".into()] }
}

fn content() -> Schema {
    Schema::Str { max_len: Some(1), examples: vec!["".into(), "one".into()] }
}

fn self_write() -> Schema {
    strukt(&[
        ("path", path()),
        ("content", content()),
        ("age", Schema::Nat { max: Some(3), edges: vec![0, 1] }),
    ])
}

fn input_schema() -> Schema {
    strukt(&[
        (
            "pending",
            Schema::List { inner: Box::new(self_write()), max_len: Some(3) },
        ),
        ("path", path()),
        ("content", content()),
    ])
}

/// @drt REQ-SELFWRITE.own_writes_ignored
/// @tests REQ-SELFWRITE.own_writes_ignored
/// @tests REQ-SELFWRITE.suppression_is_consumed
/// @tests REQ-SELFWRITE.unmatched_is_external
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_suppression() {
    let scratch = harness::scratch("selfwrite");
    let op = "REQ-SELFWRITE.own_writes_ignored";
    let implementation = harness::rust_runner(
        "REQ-SELFWRITE",
        "own_writes_ignored",
        "crates/core/src/observe/mirror.rs::suppress",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Mirror",
        "TraceLean.Mirror.suppress",
        op,
        &["pending", "path", "content"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 37, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Both outcomes, and the cases that distinguish *consumed* from *kept*: the
/// matched write had rounds to spare, and another identical write stays.
///
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_suppression_and_passthrough() {
    use tracelean_core::drt::gen;
    use tracelean_core::observe::mirror::{suppress, SelfWrite};

    let schema = input_schema();
    let mut rng = gen::Rng::new(37);
    let (mut hit, mut miss, mut aged, mut twin) = (0, 0, 0, 0);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let pending: Vec<SelfWrite> = serde_json::from_value(v["pending"].clone()).unwrap();
        let (path, content) =
            (v["path"].as_str().unwrap().to_string(), v["content"].as_str().unwrap().to_string());
        let matching: Vec<&SelfWrite> =
            pending.iter().filter(|w| w.path == path && w.content == content).collect();
        let first_age = matching.first().map(|w| w.age);
        let twins = matching.len();
        let result = suppress(pending, path, content);
        if result.suppressed {
            hit += 1;
            if first_age.unwrap_or(0) > 1 {
                aged += 1;
            }
            if twins > 1 {
                twin += 1;
            }
        } else {
            miss += 1;
        }
    }
    support::covered(
        "REQ-SELFWRITE.own_writes_ignored",
        &[
            ("a suppressed observation", hit),
            ("an external observation", miss),
            ("a matched write with rounds to spare", aged),
            ("a suppression with an identical write left pending", twin),
        ],
    );
}

/// Ageing pending suppressions, checked against the model.
///
/// The requirement this serves is a liveness one: a self-write that is never
/// observed must not suppress a later real change forever. Ageing is three
/// lines, and the failure mode — an entry that reaches zero and stays — is
/// exactly the kind of off-by-one that reads correctly.
///
/// @drt REQ-SELFWRITE.no_deadlock
/// @tests REQ-SELFWRITE.no_deadlock
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_expiry() {
    let scratch = harness::scratch("expire");
    let op = "REQ-SELFWRITE.no_deadlock";
    let implementation = harness::rust_runner(
        "REQ-SELFWRITE",
        "no_deadlock",
        "crates/core/src/observe/mirror.rs::expire",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Mirror",
        "TraceLean.Mirror.expire",
        op,
        &["pending"],
        &scratch,
    );

    let result = run(
        op,
        &Schema::Struct {
            fields: [(
                "pending".to_string(),
                Schema::List { inner: Box::new(self_write()), max_len: Some(4) },
            )]
            .into_iter()
            .collect(),
        },
        &model,
        &implementation,
        RunOptions { seed: 38, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}
