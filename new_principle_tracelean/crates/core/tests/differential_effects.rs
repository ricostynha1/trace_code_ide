mod harness;
mod support;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

fn path() -> Schema {
    Schema::Str {
        max_len: Some(1),
        examples: vec![
            "src/a.rs".into(),
            "src/b.rs".into(),
            "README.md".into(),
            ".git".into(),
            ".git/HEAD".into(),
            ".tracelean/drt.json".into(),
            "target/out".into(),
            "node_modules/x/y.js".into(),
            "src/../.git/config".into(),
            "../outside".into(),
            "/etc/passwd".into(),
            "".into(),
        ],
    }
}

fn workspace() -> Schema {
    strukt(&[(
        "files",
        Schema::List {
            inner: Box::new(Schema::Tuple {
                items: vec![path(), Schema::Str { max_len: Some(1), examples: vec!["x".into(), "y".into()] }],
            }),
            max_len: Some(2),
        },
    )])
}

fn capability() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    let reason = Schema::Str {
        max_len: Some(0),
        examples: vec!["".into(), "no bwrap".into()],
    };
    variants.insert(
        "contained".to_string(),
        Some(Box::new(strukt(&[("mechanism", reason.clone())]))),
    );
    variants.insert("unavailable".to_string(), Some(Box::new(strukt(&[("reason", reason)]))));
    Schema::Enum { variants }
}

fn input_schema() -> Schema {
    strukt(&[(
        "witness",
        strukt(&[
            (
                "copy",
                strukt(&[
                    ("before", workspace()),
                    ("workspace", workspace()),
                    ("after", workspace()),
                ]),
            ),
            ("writes", Schema::List { inner: Box::new(path()), max_len: Some(3) }),
            ("containment", capability()),
        ]),
    )])
}

/// @drt REQ-OBS.workspace_is_a_copy
/// @tests REQ-OBS.workspace_is_a_copy
/// @tests REQ-SBX.real_tree_untouched
/// @tests REQ-SBX.capability_reported
/// @tests ARCH-EFFECT-LAW.law_checked
/// @tests ARCH-EFFECT-LAW.axiomatised
/// @structural ARCH-EFFECT-LAW.law_checked reason="a claim that every stated law has a differential test behind it, which is a count over the model tree and the binding file rather than a value"
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_what_a_run_got_wrong() {
    let scratch = harness::scratch("effects");
    let op = "REQ-OBS.workspace_is_a_copy";
    let implementation = harness::rust_runner(
        "REQ-OBS",
        "workspace_is_a_copy",
        "crates/core/src/observe/effects.rs::run_violations",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Effects",
        "TraceLean.Effects.runViolations",
        op,
        &["witness"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 53, cases: 2_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_every_violation() {
    use tracelean_core::drt::gen;
    use tracelean_core::observe::effects::{run_violations, RunWitness, Violation};

    let schema = input_schema();
    let mut rng = gen::Rng::new(53);
    let (mut missing, mut differs, mut changed, mut escaped, mut unreported, mut clean) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut extra, mut protected_missing) = (0u64, 0u64);
    for _ in 0..2_000 {
        let value = gen::value(&schema, &mut rng);
        let witness: RunWitness = serde_json::from_value(value["witness"].clone())
            .expect("the schema generates what the type parses");
        let violations = run_violations(witness);
        if violations.is_empty() {
            clean += 1;
        }
        for violation in violations {
            match violation {
                Violation::MissingFromCopy { path } if path.starts_with(".git") || path.starts_with(".tracelean") => {
                    protected_missing += 1
                }
                Violation::MissingFromCopy { .. } => missing += 1,
                Violation::CopyDiffers { .. } => differs += 1,
                Violation::ExtraInCopy { .. } => extra += 1,
                Violation::RealTreeChanged { .. } => changed += 1,
                Violation::Escaped { .. } => escaped += 1,
                Violation::ContainmentUnreported => unreported += 1,
            }
        }
    }

    support::covered(
        "REQ-OBS.workspace_is_a_copy",
        &[
            ("missing from copy", missing),
            ("copy differs", differs),
            ("extra in copy", extra),
            ("protected root unseen", protected_missing),
            ("real tree changed", changed),
            ("escaped", escaped),
            ("containment unreported", unreported),
            ("no violation at all", clean),
        ],
    );
}
