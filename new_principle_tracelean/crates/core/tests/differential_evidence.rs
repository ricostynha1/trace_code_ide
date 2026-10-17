//! The first full vertical slice: a requirement clause, a Lean model, a Rust
//! implementation, and a differential test between them.
//!
//! This is the shape everything else in the port is patterned on. It is also
//! the first thing in this tree that could be marked tested.

mod harness;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracelean_core::drt::gen;
use tracelean_core::drt::lean_runner::{self, LeanEntry};
use tracelean_core::drt::rust_runner;
use tracelean_core::drt::run::{run, RunOptions, RunnerSpec};
use tracelean_core::drt::schema::Schema;
use tracelean_core::drt::{Binding, CallSpec};

mod support;

const OP: &str = "REQ-EVID.mark_per_bond";

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

/// The declared input shape, matching `.tracelean/drt.json`.
fn input_schema() -> Schema {
    let mut record = BTreeMap::new();
    record.insert(
        "bond".to_string(),
        Schema::simple_enum(&["requirementModel", "modelImpl", "modelProof"]),
    );
    record.insert("level".to_string(), Schema::simple_enum(&["L1", "L2", "L3", "L4"]));

    let mut fields = BTreeMap::new();
    fields.insert(
        "records".to_string(),
        Schema::List {
            inner: Box::new(Schema::Struct { fields: record }),
            max_len: Some(6),
        },
    );
    fields.insert("annotated".to_string(), Schema::Bool);
    Schema::Struct { fields }
}

fn build_lean_runner(root: &Path, scratch: &Path) -> RunnerSpec {
    lean_runner::toolchain().expect("a Lean toolchain");

    let entries = [LeanEntry {
        op: "REQ-EVID.marks_written".to_string(),
        function: "TraceLean.Evidence.written".to_string(),
        arguments: vec!["marks".to_string()],
    }];
    // The generated package sits at <scratch>/.tracelean/drt-lean, so the model
    // package is reached by an absolute path rather than a fragile relative one.
    let model_path = root.join("formal");
    lean_runner::materialize(
        scratch,
        &["TraceLean.Evidence"],
        "tracelean",
        &model_path.display().to_string(),
        &entries,
    )
    .expect("generated");

    let dir = lean_runner::package_dir(scratch);
    // Under the shared guard, retried while another build holds Lake's lock.
    let built = harness::lake_build(&model_path, &dir);
    assert!(
        built.status.success(),
        "the generated Lean runner did not build:\n{}\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    RunnerSpec {
        cmd: vec![dir
            .join(".lake")
            .join("build")
            .join("bin")
            .join("drtRunner")
            .display()
            .to_string()],
        cwd: None,
    }
}

/// @drt REQ-EVID.mark_per_bond
/// @tests REQ-EVID.mark_per_bond
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_marks() {
    let scratch = harness::scratch("evidence");
    let implementation =
        harness::rust_runner("REQ-EVID", "mark_per_bond", "crates/core/src/evidence.rs::marks", &scratch);
    let model = harness::lean_runner(
        "TraceLean.Evidence",
        "TraceLean.Evidence.marks",
        OP,
        &["records", "annotated"],
        &scratch,
    );

    let options = RunOptions { seed: 7, cases: 2_000 };
    let result = run(OP, &input_schema(), &model, &implementation, options).expect("both runners answer");

    support::agreed(&result);
    assert_eq!(result.cases, 2_000);
    assert_eq!(result.seed, 7);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A divergence must actually be reported when one exists — otherwise a clean
/// run says nothing. The model writes marks as text and the implementation
/// bound here caps them instead, so the two disagree on every input.
///
/// @tests REQ-DRT.divergence_reported
/// @structural REQ-DRT.divergence_reported reason="a claim about what the harness loop does with a disagreement, checked by producing a real one"
/// @tests REQ-DRT-GEN.shrink_preserves
/// @structural REQ-DRT-GEN.shrink_preserves reason="a claim about the shrink loop, which only ever moves to a candidate that still diverges — a property of the loop rather than of the reduction function"
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn a_real_disagreement_is_found_and_reduced() {
    let root = project_root();
    let scratch = std::env::temp_dir().join("tracelean-drt-evidence-neg");
    let _ = std::fs::remove_dir_all(&scratch);

    let binding = Binding {
        req_id: "REQ-EVID".into(),
        clause: Some("marks_written".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            // Deliberately the wrong function: same argument, another answer.
            entry: "crates/core/src/trace/rollup.rs::partial_cap".into(),
            params: BTreeMap::new(),
        },
    };
    let entry = rust_runner::resolve(&root, &binding).expect("resolves");
    let mut deps = BTreeMap::new();
    deps.insert(
        "tracelean-core".to_string(),
        root.join("crates").join("core").display().to_string(),
    );
    rust_runner::materialize(&scratch, &[entry], &deps).expect("generated");
    let dir = rust_runner::package_dir(&scratch);
    assert!(Command::new("cargo")
        .args(["build", "--release", "--quiet"])
        .current_dir(&dir)
        .status()
        .expect("cargo runs")
        .success());
    let implementation = RunnerSpec {
        cmd: vec![dir.join("target/release/tracelean-drt-runner").display().to_string()],
        cwd: None,
    };
    let model = build_lean_runner(&root, &scratch);

    let mut fields = BTreeMap::new();
    fields.insert("marks".to_string(), support::marks());
    let result = run(
        "REQ-EVID.marks_written",
        &Schema::Struct { fields },
        &model,
        &implementation,
        RunOptions { seed: 3, cases: 200 },
    )
    .expect("both runners answer");

    let divergence = result.divergence.expect("a disagreement is found");
    // Reduced to the smallest input that still disagrees: no mark held, which
    // the model writes as four dashes and the cap hands back unchanged.
    let none = serde_json::json!({
        "annotated": false, "judged": false, "tested": false, "proved": false, "structural": false
    });
    assert_eq!(divergence.input, serde_json::json!({ "marks": none }));
    assert_eq!(divergence.model.output, Some(serde_json::json!("----")));
    assert_eq!(divergence.implementation.output, Some(none));

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The generator must actually reach the cases that matter: a run in which no
/// case had records for all three bonds, or none held a mark without the one
/// before it, would agree without ever asking whether the marks are separate.
///
/// @tests REQ-DRT-COVER.floor_stated
/// @tests REQ-EVID.marks
#[test]
fn generation_reaches_marks_held_out_of_order() {
    use tracelean_core::evidence::{marks, Record};

    let schema = input_schema();
    let mut rng = gen::Rng::new(7);
    let (mut full, mut skipped) = (0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let records: Vec<Record> = serde_json::from_value(v["records"].clone()).unwrap();
        let bonds: std::collections::BTreeSet<_> = records.iter().map(|r| r.bond).collect();
        if bonds.len() == 3 {
            full += 1;
        }
        let held = marks(records, v["annotated"].as_bool().unwrap());
        let each = held.each();
        if each.windows(2).any(|w| !w[0].1 && w[1].1) {
            skipped += 1;
        }
    }
    support::covered(
        OP,
        &[("exercised all three bonds", full), ("a mark held without the one before it", skipped)],
    );
}

fn written_input() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("marks".to_string(), support::marks());
    Schema::Struct { fields }
}

/// The written marks, not only the marks.
///
/// @drt REQ-EVID.marks_written
/// @tests REQ-EVID.marks_written
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_writing_the_marks() {
    let scratch = harness::scratch("written");
    let op = "REQ-EVID.marks_written";
    let implementation =
        harness::rust_runner("REQ-EVID", "marks_written", "crates/core/src/evidence.rs::written", &scratch);
    let model = harness::lean_runner("TraceLean.Evidence", "TraceLean.Evidence.written", op, &["marks"], &scratch);

    let result = run(
        op,
        &written_input(),
        &model,
        &implementation,
        RunOptions { seed: 29, cases: 1_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// What a clause checked only by reading the tree holds.
///
/// @drt REQ-EVID.structural_not_applicable
/// @tests REQ-EVID.structural_not_applicable
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_a_structural_clause() {
    let scratch = harness::scratch("structural");
    let op = "REQ-EVID.structural_not_applicable";
    let implementation = harness::rust_runner(
        "REQ-EVID",
        "structural_not_applicable",
        "crates/core/src/evidence.rs::structural",
        &scratch,
    );
    let model =
        harness::lean_runner("TraceLean.Evidence", "TraceLean.Evidence.structural", op, &["marks"], &scratch);

    let result = run(
        op,
        &written_input(),
        &model,
        &implementation,
        RunOptions { seed: 31, cases: 1_000 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A structural clause has to be asked with a test and a proof already
/// recorded, which is what it must drop, and with neither.
///
/// @tests REQ-EVID.structural_not_applicable
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_structural_clauses_holding_what_cannot_apply() {
    use tracelean_core::evidence::{structural, Marks};

    let mut rng = gen::Rng::new(31);
    let (mut dropped, mut nothing, mut judged) = (0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&written_input(), &mut rng);
        let marks: Marks = serde_json::from_value(v["marks"].clone()).unwrap();
        let read = structural(marks);
        assert!(read.structural && !read.tested && !read.proved, "{marks:?} became {read:?}");
        assert_eq!((read.annotated, read.judged), (marks.annotated, marks.judged));
        if marks.tested || marks.proved {
            dropped += 1;
        } else {
            nothing += 1;
        }
        if marks.judged {
            judged += 1;
        }
    }
    support::covered(
        "REQ-EVID.structural_not_applicable",
        &[
            ("a test or proof held, and dropped", dropped),
            ("neither held", nothing),
            ("judged", judged),
        ],
    );
}

/// The writing has to be asked where a gap sits between two marks, which is
/// where a ladder and fixed positions disagree, as well as at both ends.
///
/// @tests REQ-EVID.marks_written
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_gaps_between_marks() {
    use tracelean_core::evidence::{written, Marks};

    let mut rng = gen::Rng::new(29);
    let (mut gap, mut none, mut all, mut neither) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..1_000 {
        let v = gen::value(&written_input(), &mut rng);
        let marks: Marks = serde_json::from_value(v["marks"].clone()).unwrap();
        let text = written(marks);
        assert_eq!(text.chars().count(), 4, "marks are four positions");
        let inner = text.trim_matches('-');
        if inner.contains('-') {
            gap += 1;
        }
        if text == "----" {
            none += 1;
        }
        if text == "AJTP" {
            all += 1;
        }
        // A test or proof held where neither applies is still written as not
        // applicable: the dot says nothing can be held there.
        if text.ends_with("··") {
            assert!(marks.structural);
            neither += 1;
        }
    }
    support::covered(
        "REQ-EVID.marks_written",
        &[
            ("marks with a gap between two held", gap),
            ("no mark held", none),
            ("every mark held", all),
            ("testing and proving not applicable", neither),
        ],
    );
}
