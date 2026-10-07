//! The first full vertical slice: a requirement clause, a Lean model, a Rust
//! implementation, and a differential test between them.
//!
//! This is the shape everything else in the port is patterned on. It is also
//! the first thing in this tree that can reach L3.

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

const OP: &str = "REQ-EVID.weakest_link";

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
    Schema::Struct { fields }
}

fn build_rust_runner(root: &Path, scratch: &Path) -> RunnerSpec {
    let binding = Binding {
        req_id: "REQ-EVID".into(),
        clause: Some("weakest_link".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: "crates/core/src/evidence.rs::assurance".into(),
            params: BTreeMap::new(),
        },
    };
    let entry = rust_runner::resolve(root, &binding).expect("binding resolves");
    assert_eq!(entry.op, OP);

    let mut deps = BTreeMap::new();
    deps.insert(
        "tracelean-core".to_string(),
        root.join("crates").join("core").display().to_string(),
    );
    rust_runner::materialize(scratch, &[entry], &deps).expect("generated");

    let dir = rust_runner::package_dir(scratch);
    let built = Command::new("cargo")
        .args(["build", "--release", "--quiet"])
        .current_dir(&dir)
        .output()
        .expect("cargo runs");
    assert!(
        built.status.success(),
        "the generated Rust runner did not compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    RunnerSpec {
        cmd: vec![dir
            .join("target")
            .join("release")
            .join("tracelean-drt-runner")
            .display()
            .to_string()],
        cwd: None,
    }
}

fn build_lean_runner(root: &Path, scratch: &Path) -> RunnerSpec {
    lean_runner::toolchain().expect("a Lean toolchain");

    let entries = [LeanEntry {
        op: OP.to_string(),
        function: "TraceLean.Evidence.assurance".to_string(),
        arguments: vec!["records".to_string()],
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

/// @drt REQ-EVID.weakest_link
/// @tests REQ-EVID.weakest_link
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_assurance() {
    let root = project_root();
    let scratch = std::env::temp_dir().join("tracelean-drt-evidence");
    let _ = std::fs::remove_dir_all(&scratch);

    let implementation = build_rust_runner(&root, &scratch);
    let model = build_lean_runner(&root, &scratch);

    let options = RunOptions { seed: 7, cases: 2_000, shrink_rounds: 100 };
    let result = run(OP, &input_schema(), &model, &implementation, options)
        .expect("both runners answer");

    support::agreed(&result);
    assert_eq!(result.cases, 2_000);
    assert_eq!(result.seed, 7);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// A divergence must actually be reported when one exists — otherwise a clean
/// run says nothing. `chain` returns the per-bond list rather than the minimum,
/// so binding it to the same op as the model is a deliberate mismatch.
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
        clause: Some("weakest_link".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            // Deliberately the wrong function.
            entry: "crates/core/src/evidence.rs::chain".into(),
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

    let result = run(
        OP,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 3, cases: 200, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    let divergence = result.divergence.expect("a disagreement is found");
    // Reduced to the smallest input that still disagrees: the empty list, where
    // the minimum is `L1` and the chain is three of them.
    assert_eq!(divergence.input, serde_json::json!({"records": []}));
    assert_eq!(divergence.model.output, Some(serde_json::json!("L1")));
    assert_eq!(
        divergence.implementation.output,
        Some(serde_json::json!(["L1", "L1", "L1"]))
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The generator must actually reach the case that matters — a run in which no
/// case had records for all three bonds would agree vacuously.
///
/// @tests REQ-DRT-COVER.floor_stated
#[test]
fn generation_reaches_full_chains() {
    let schema = input_schema();
    let mut rng = gen::Rng::new(7);
    let full = (0..2_000)
        .filter(|_| {
            let v = gen::value(&schema, &mut rng);
            let records = v["records"].as_array().cloned().unwrap_or_default();
            let bonds: std::collections::BTreeSet<String> = records
                .iter()
                .filter_map(|r| r["bond"].as_str().map(str::to_string))
                .collect();
            bonds.len() == 3
        })
        .count();
    support::covered("REQ-EVID.weakest_link", &[("exercised all three bonds", full as u64)]);
}

/// The chain, not only the number it reduces to.
///
/// `assurance` is a minimum, so it cannot distinguish `L1 · L1 · L4` from
/// `L1 · L4 · L1` — and the whole point of `chain_rendered` is that a reader
/// can see which bond is the weak one. Agreement on the minimum is therefore no
/// evidence at all about the chain; this asks the question the requirement
/// actually asks.
///
/// @drt REQ-EVID.chain_rendered
/// @tests REQ-EVID.chain_rendered
#[test]
#[ignore = "builds a Lean package and a Rust crate; run with --ignored"]
fn model_and_implementation_agree_on_the_per_bond_chain() {
    let scratch = harness::scratch("chain");
    let op = "REQ-EVID.chain_rendered";
    let implementation = harness::rust_runner(
        "REQ-EVID",
        "chain_rendered",
        "crates/core/src/evidence.rs::chain",
        &scratch,
    );
    let model = harness::lean_runner(
        "TraceLean.Evidence",
        "TraceLean.Evidence.chain",
        op,
        &["records"],
        &scratch,
    );

    let result = run(
        op,
        &input_schema(),
        &model,
        &implementation,
        RunOptions { seed: 29, cases: 2_000, shrink_rounds: 100 },
    )
    .expect("both runners answer");

    support::agreed(&result);

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The chain has to be asked where it says more than the minimum does.
///
/// A run whose chains were all `L1 · L1 · L1` would agree perfectly and
/// establish nothing: the whole content of `chain_rendered` is that a reader
/// can see *which* bond is the weak one, and that is only visible when the
/// bonds differ. So the floor is on chains that are not flat, and separately on
/// chains where a bond is absent — absence is the case a reader most needs told
/// apart from a bond that was checked and came back low.
///
/// @tests REQ-EVID.chain_rendered
/// @tests REQ-DRT-COVER.law_coverage
#[test]
fn generation_reaches_chains_that_say_more_than_their_minimum() {
    use tracelean_core::evidence::{assurance, chain, Bond, Level, Record};

    let schema = input_schema();
    let mut rng = gen::Rng::new(29);
    let (mut varied, mut flat, mut absent, mut complete) = (0u64, 0u64, 0u64, 0u64);
    for _ in 0..2_000 {
        let v = gen::value(&schema, &mut rng);
        let records: Vec<Record> = serde_json::from_value(v["records"].clone()).unwrap();

        let rendered = chain(records.clone());
        assert_eq!(rendered.len(), 3, "a chain is one level per bond");
        // The minimum is the chain's minimum, so a reader who quotes one number
        // and a reader who reads the chain never disagree.
        assert_eq!(
            assurance(records.clone()),
            *rendered.iter().min().expect("three bonds"),
            "the aggregate is not the chain's minimum"
        );

        if rendered.iter().any(|level| *level != rendered[0]) {
            varied += 1;
        } else {
            flat += 1;
        }
        let bonds: std::collections::BTreeSet<Bond> =
            records.iter().map(|record| record.bond).collect();
        if bonds.len() == 3 {
            complete += 1;
        }
        // A bond nothing recorded reads `L1`, which is also what a bond checked
        // and found wanting reads. The chain must be asked both.
        if bonds.len() < 3 && rendered.contains(&Level::L1) {
            absent += 1;
        }
    }
    support::covered(
        "REQ-EVID.chain_rendered",
        &[
            ("a chain whose bonds are not all equal", varied),
            ("a chain that is flat", flat),
            ("a chain with a bond nothing recorded", absent),
            ("a chain with all three bonds recorded", complete),
        ],
    );
}
