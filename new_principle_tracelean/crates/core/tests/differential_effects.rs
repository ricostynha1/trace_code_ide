//! The sandbox laws, checked against the model.
//!
//! This is the suite ADR-0002 was written for. The effect — copying a project
//! into a workspace, probing a host, watching where a tool wrote — cannot run
//! in Lean. The *law* about it is an ordinary total function over a witness,
//! and that is what both sides compute here.
//!
//! The generator is doing something specific: it draws paths from a small
//! alphabet that includes protected roots, build directories, escapes and
//! ordinary source files, so that a generated witness routinely contains a file
//! the copy was right to omit next to one it was wrong to omit. Random strings
//! would produce mirrored paths almost every time and the law would never be
//! asked the question it exists for.

mod harness;
mod support;

use std::collections::BTreeMap;

use tracelean_core::drt::run::{run, RunOptions};
use tracelean_core::drt::schema::Schema;

fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Paths worth generating: one of each class, plus the spellings that try to
/// disguise themselves.
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

/// Snapshots are kept short deliberately. Three independently generated
/// workspaces of four files each almost never coincide, and a run that broke no
/// law at all would then be a case the suite barely reached — agreement on
/// nothing but failures is not agreement about the law. Two entries drawn from
/// a small alphabet collide often enough for the clean path to be real.
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
        // The empty reason is the case the law exists for, so it is one of two
        // values rather than one draw in a large alphabet.
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
        RunOptions { seed: 53, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// Every violation the law can report must actually occur, or agreement is
/// agreement about the empty list.
///
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

    // The floors are the binding's (`.tracelean/drt.json`), so the coverage
    // half of the op's evidence is recorded and the run can reach L3.
    //
    // `copy differs` and a wholly clean run both need a coincidence between two
    // independently generated snapshots, so they are rarer than the rest by the
    // shape of the generator rather than by the shape of the law. The floors
    // are what this seed honestly reaches; raising them would mean narrowing
    // the alphabet until the other five stopped occurring.
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
