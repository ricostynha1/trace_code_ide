mod harness;

use harness::strukt;

use tracelean_core::drt::schema::Schema;

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
    let op = "REQ-SELFWRITE.own_writes_ignored";

    harness::agree(op, &input_schema(), 37, 2_000);
}

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
    harness::covered(
        "REQ-SELFWRITE.own_writes_ignored",
        &[
            ("a suppressed observation", hit),
            ("an external observation", miss),
            ("a matched write with rounds to spare", aged),
            ("a suppression with an identical write left pending", twin),
        ],
    );
}

/// @drt REQ-SELFWRITE.no_deadlock
/// @tests REQ-SELFWRITE.no_deadlock
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_expiry() {
    let op = "REQ-SELFWRITE.no_deadlock";

    harness::agree(op, &Schema::Struct {
            fields: [(
                "pending".to_string(),
                Schema::List { inner: Box::new(self_write()), max_len: Some(4) },
            )]
            .into_iter()
            .collect(),
        }, 38, 2_000);
}
