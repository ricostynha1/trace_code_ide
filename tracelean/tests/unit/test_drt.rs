//! Differential testing: protocol, generation, shrinking, comparison, and the
//! evidence a run produces.
//!
//! The Lean side is stood in for by small Python runners. That is deliberate
//! rather than a compromise: it exercises the real subprocess path — spawning,
//! line framing, flushing, timeouts, restarts — without requiring a Lean
//! toolchain, and leaves only Lean codegen itself gated behind one.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;
use tempfile::TempDir;

use tracelean_lib::drt::{
    config::{self, Binding, CallSpec, DrtConfig},
    gen::{self, Rng},
    lean_runner,
    protocol::{replies_agree, values_agree, Case, Reply, Runner, RunnerError, RunnerSpec},
    run::{self, CoverageFloor, RunOptions, Triage},
    schema::{output_variant, Schema},
};

// --- test runners --------------------------------------------------------

/// Write a Python runner and return a spec that starts it.
fn python_runner(dir: &TempDir, name: &str, body: &str) -> RunnerSpec {
    let path = dir.path().join(name);
    let source = format!(
        r#"import json, sys
def handle(op, x):
{body}
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    c = json.loads(line)
    try:
        r = {{"case": c["case"], "output": handle(c["op"], c["input"])}}
    except Exception as e:
        r = {{"case": c["case"], "error": str(e)}}
    sys.stdout.write(json.dumps(r) + "\n")
    sys.stdout.flush()
"#
    );
    std::fs::write(&path, source).unwrap();
    RunnerSpec {
        cmd: vec!["python3".into(), path.to_string_lossy().into_owned()],
        cwd: None,
        env: BTreeMap::new(),
    }
}

/// A binding plus the stub process standing in for the implementation.
///
/// A real binding names a function and `CallSpec::spec` turns it into a
/// process; these tests exercise the comparison machinery itself, so they hand
/// `run` a process directly and leave the binding's `entry` unused.
fn string_binding(model: RunnerSpec, implementation: RunnerSpec) -> (Binding, RunnerSpec) {
    let binding = Binding {
        req_id: "REQ-A".into(),
        clause: Some("post".into()),
        op: "default".into(),
        model,
        implementation: CallSpec {
            language: "python".into(),
            entry: "unused-by-these-tests.py::handle".into(),
            params: BTreeMap::new(),
            convert: None,
        },
        input: Schema::Struct {
            fields: BTreeMap::from([(
                "password".to_string(),
                Schema::Str { max_len: Some(20), examples: vec![] },
            )]),
        },
        coverage_floor: CoverageFloor::None,
    };
    (binding, implementation)
}

// --- protocol ------------------------------------------------------------

#[test]
fn runner_answers_a_case() {
    let dir = TempDir::new().unwrap();
    let spec = python_runner(&dir, "echo.py", "    return x");
    let mut runner = Runner::spawn(&spec).unwrap();
    let case = Case { case: 1, op: "default".into(), input: json!({"a": 1}) };
    let reply = runner.ask(&case, Duration::from_secs(5)).unwrap();
    assert_eq!(reply.case, 1);
    assert_eq!(reply.output, Some(json!({"a": 1})));
}

#[test]
fn runner_reports_a_missing_program() {
    let spec = RunnerSpec {
        cmd: vec!["definitely-not-a-real-program-xyz".into()],
        cwd: None,
        env: BTreeMap::new(),
    };
    assert!(matches!(Runner::spawn(&spec), Err(RunnerError::Spawn(_))));
}

#[test]
fn runner_times_out_rather_than_hanging() {
    let dir = TempDir::new().unwrap();
    let spec = python_runner(&dir, "slow.py", "    import time; time.sleep(30); return x");
    let mut runner = Runner::spawn(&spec).unwrap();
    let case = Case { case: 1, op: "default".into(), input: json!(null) };
    assert_eq!(
        runner.ask(&case, Duration::from_millis(300)),
        Err(RunnerError::Timeout)
    );
}

#[test]
fn runner_death_carries_the_stderr_text() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("die.py");
    std::fs::write(
        &path,
        "import sys\nsys.stderr.write('boom: the real reason\\n')\nsys.stderr.flush()\nsys.exit(1)\n",
    )
    .unwrap();
    let spec = RunnerSpec {
        cmd: vec!["python3".into(), path.to_string_lossy().into_owned()],
        cwd: None,
        env: BTreeMap::new(),
    };
    let mut runner = Runner::spawn(&spec).unwrap();
    let case = Case { case: 1, op: "default".into(), input: json!(null) };
    match runner.ask(&case, Duration::from_secs(5)) {
        Err(RunnerError::Died(message)) => {
            assert!(message.contains("boom"), "stderr must survive: {message}")
        }
        other => panic!("expected death, got {other:?}"),
    }
}

/// The reason a dying runner gives must not depend on how busy the machine is.
///
/// Reading the captured stderr the instant stdout closed was a race: on a
/// loaded machine the child had died but its stderr reader had not been
/// scheduled, so the death was reported with an empty reason. Run it enough
/// times, under load, that a regression shows up here rather than in the field.
#[test]
fn a_dying_runner_reports_its_reason_every_time() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("die.py");
    std::fs::write(
        &path,
        "import sys\nsys.stderr.write('boom: the real reason\\n')\nsys.stderr.flush()\nsys.exit(1)\n",
    )
    .unwrap();
    let spec = RunnerSpec {
        cmd: vec!["python3".into(), path.to_string_lossy().into_owned()],
        cwd: None,
        env: BTreeMap::new(),
    };
    for attempt in 0..12 {
        let mut runner = Runner::spawn(&spec).unwrap();
        let case = Case { case: 1, op: "default".into(), input: json!(null) };
        match runner.ask(&case, Duration::from_secs(5)) {
            Err(RunnerError::Died(message)) => assert!(
                message.contains("boom"),
                "attempt {attempt}: stderr must survive the race, got {message:?}"
            ),
            other => panic!("attempt {attempt}: expected death, got {other:?}"),
        }
    }
}

#[test]
fn an_error_reply_is_an_answer_not_a_crash() {
    let dir = TempDir::new().unwrap();
    let spec = python_runner(&dir, "raise.py", "    raise ValueError('nope')");
    let mut runner = Runner::spawn(&spec).unwrap();
    let case = Case { case: 3, op: "default".into(), input: json!(null) };
    let reply = runner.ask(&case, Duration::from_secs(5)).unwrap();
    assert!(reply.error.is_some());
    assert!(reply.output.is_none());
    // And the runner is still alive for the next case.
    let next = Case { case: 4, op: "default".into(), input: json!(null) };
    assert!(runner.ask(&next, Duration::from_secs(5)).is_ok());
}

// --- comparison ----------------------------------------------------------

#[test]
fn both_sides_failing_counts_as_agreement() {
    let a = Reply::failed(1, "ValueError");
    let b = Reply::failed(1, "Except.error weakPassword");
    assert!(replies_agree(&a, &b), "the messages differ; the behaviour does not");
}

#[test]
fn one_side_failing_is_a_divergence() {
    let a = Reply::ok(1, json!("ok"));
    let b = Reply::failed(1, "boom");
    assert!(!replies_agree(&a, &b));
}

#[test]
fn absent_optional_field_equals_explicit_null() {
    // Lean's derived ToJson omits a `x?` field that is `none`.
    assert!(values_agree(&json!({"a": 1}), &json!({"a": 1, "b": null})));
}

#[test]
fn numbers_compare_by_canonical_value() {
    assert!(values_agree(&json!(1), &json!(1.0)));
    assert!(!values_agree(&json!(1), &json!(2)));
}

#[test]
fn nullary_constructor_is_a_bare_string() {
    // The encoding Lean actually derives, pinned so a toolchain upgrade that
    // changes it is caught here rather than in a confusing divergence.
    assert!(values_agree(&json!("weakPassword"), &json!("weakPassword")));
    assert!(!values_agree(&json!("weakPassword"), &json!({"weakPassword": {}})));
}

// --- generation ----------------------------------------------------------

#[test]
fn generation_is_deterministic_for_a_seed() {
    let schema = Schema::Struct {
        fields: BTreeMap::from([
            ("n".to_string(), Schema::Nat { max: Some(1000), edges: Vec::new() }),
            ("s".to_string(), Schema::Str { max_len: Some(10), examples: vec![] }),
        ]),
    };
    let run_once = |seed: u64| {
        let mut rng = Rng::new(seed);
        (0..50).map(|_| gen::generate(&schema, &mut rng)).collect::<Vec<_>>()
    };
    assert_eq!(run_once(7), run_once(7), "same seed must replay exactly");
    assert_ne!(run_once(7), run_once(8));
}

#[test]
fn generation_hits_boundary_values() {
    let schema = Schema::Nat { max: Some(100), edges: Vec::new() };
    let mut rng = Rng::new(3);
    let values: Vec<u64> = (0..300)
        .map(|_| gen::generate(&schema, &mut rng).as_u64().unwrap())
        .collect();
    // 8 is exactly the kind of threshold a requirement turns on; uniform
    // sampling over 0..100 would find it about three times in 300 draws.
    assert!(values.contains(&0));
    assert!(values.contains(&8));
}

#[test]
fn every_enum_variant_is_reachable() {
    let schema = Schema::Enum {
        variants: BTreeMap::from([
            ("weakPassword".to_string(), None),
            ("locked".to_string(), Some(Schema::Nat { max: Some(60), edges: Vec::new() })),
        ]),
    };
    let mut rng = Rng::new(11);
    let seen: std::collections::BTreeSet<String> = (0..100)
        .map(|_| schema.constructor_of(&gen::generate(&schema, &mut rng)).unwrap())
        .collect();
    assert!(seen.contains("weakPassword"));
    assert!(seen.contains("locked"));
}

#[test]
fn nullary_variants_generate_as_bare_strings() {
    let schema = Schema::Enum {
        variants: BTreeMap::from([("weakPassword".to_string(), None)]),
    };
    let mut rng = Rng::new(1);
    assert_eq!(gen::generate(&schema, &mut rng), json!("weakPassword"));
}

#[test]
fn options_generate_none_sometimes() {
    let schema = Schema::Option { inner: Box::new(Schema::Nat { max: Some(5), edges: Vec::new() }) };
    let mut rng = Rng::new(5);
    let values: Vec<_> = (0..100).map(|_| gen::generate(&schema, &mut rng)).collect();
    assert!(values.iter().any(|v| v.is_null()), "none is the case implementations forget");
    assert!(values.iter().any(|v| !v.is_null()));
}

// --- shrinking -----------------------------------------------------------

#[test]
fn shrinking_reduces_numbers_and_strings() {
    assert!(gen::shrink(&json!(100)).contains(&json!(0)));
    assert!(gen::shrink(&json!("hello")).contains(&json!("")));
    assert!(gen::shrink(&json!([1, 2, 3])).contains(&json!([])));
}

#[test]
fn shrinking_a_leaf_value_terminates() {
    assert!(gen::shrink(&json!(0)).is_empty());
    assert!(gen::shrink(&json!("")).is_empty());
    assert!(gen::shrink(&json!(null)).is_empty());
}

// --- end-to-end runs -----------------------------------------------------

#[test]
fn agreeing_sides_produce_no_divergence_and_earn_l3() {
    let dir = TempDir::new().unwrap();
    let model = python_runner(&dir, "m.py", "    return len(x['password']) >= 8");
    let implementation = python_runner(&dir, "i.py", "    return len(x['password']) >= 8");
    let (mut binding, implementation) = string_binding(model, implementation);
    binding.coverage_floor = CoverageFloor::MinOutputVariants { n: 2 };

    let options = RunOptions { seed: 4, cases: 60, ..RunOptions::default() };
    let result = run::run(&binding, &implementation, &options).unwrap();

    assert_eq!(result.cases_run, 60);
    assert!(result.divergences.is_empty(), "{:?}", result.divergences);
    assert!(result.coverage_floor_met, "coverage: {:?}", result.coverage);
    assert!(result.earns_l3());
}

#[test]
fn a_disagreeing_implementation_is_caught_and_shrunk() {
    let dir = TempDir::new().unwrap();
    // The model is the requirement: at least 8 characters. The implementation
    // has the classic off-by-one.
    let model = python_runner(&dir, "m.py", "    return len(x['password']) >= 8");
    let implementation = python_runner(&dir, "i.py", "    return len(x['password']) > 8");
    let (binding, implementation) = string_binding(model, implementation);

    let options = RunOptions { seed: 9, cases: 200, ..RunOptions::default() };
    let result = run::run(&binding, &implementation, &options).unwrap();

    assert!(!result.divergences.is_empty(), "an off-by-one must be caught");
    assert!(!result.earns_l3(), "a run with divergences cannot earn L3");

    // Every divergence must be an 8-character password: the only input on
    // which `>= 8` and `> 8` differ.
    for d in &result.divergences {
        let password = d.input["password"].as_str().unwrap();
        assert_eq!(
            password.chars().count(),
            8,
            "shrinking should isolate the boundary, got {password:?}"
        );
    }
}

#[test]
fn a_crashing_implementation_diverges_rather_than_aborting_the_run() {
    let dir = TempDir::new().unwrap();
    let model = python_runner(&dir, "m.py", "    return True");
    let implementation = python_runner(
        &dir,
        "i.py",
        "    if len(x['password']) == 0: raise ValueError('empty')\n    return True",
    );
    let (binding, implementation) = string_binding(model, implementation);

    let options = RunOptions { seed: 2, cases: 80, ..RunOptions::default() };
    let result = run::run(&binding, &implementation, &options).unwrap();

    assert_eq!(result.cases_run, 80, "the run continues past a crash");
    assert!(result.divergences.iter().any(|d| d.implementation.error.is_some()));
}

#[test]
fn a_runner_that_dies_is_restarted_and_recorded_as_an_incident() {
    let dir = TempDir::new().unwrap();
    let model = python_runner(&dir, "m.py", "    return True");
    // Dies on the third case, then works again after the restart.
    let path = dir.path().join("flaky.py");
    std::fs::write(
        &path,
        r#"import json, sys, os
state = "/tmp/tracelean-drt-flaky-marker"
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    c = json.loads(line)
    if c["case"] == 3 and not os.path.exists(state):
        open(state, "w").close()
        sys.stderr.write("dying once\n")
        sys.exit(1)
    sys.stdout.write(json.dumps({"case": c["case"], "output": True}) + "\n")
    sys.stdout.flush()
"#,
    )
    .unwrap();
    let _ = std::fs::remove_file("/tmp/tracelean-drt-flaky-marker");
    let implementation = RunnerSpec {
        cmd: vec!["python3".into(), path.to_string_lossy().into_owned()],
        cwd: None,
        env: BTreeMap::new(),
    };
    let (binding, implementation) = string_binding(model, implementation);

    let options = RunOptions { seed: 1, cases: 8, ..RunOptions::default() };
    let result = run::run(&binding, &implementation, &options).unwrap();
    let _ = std::fs::remove_file("/tmp/tracelean-drt-flaky-marker");

    assert!(!result.incidents.is_empty(), "a death must be reported, not hidden");
    assert_eq!(result.cases_run, 8, "the case that killed the runner still counts");
}

#[test]
fn coverage_floor_blocks_l3_when_only_one_outcome_was_seen() {
    let dir = TempDir::new().unwrap();
    // Both sides always agree — and always return the same thing, so the run
    // establishes almost nothing.
    let model = python_runner(&dir, "m.py", "    return True");
    let implementation = python_runner(&dir, "i.py", "    return True");
    let (mut binding, implementation) = string_binding(model, implementation);
    binding.coverage_floor = CoverageFloor::MinOutputVariants { n: 2 };

    let options = RunOptions { seed: 1, cases: 30, ..RunOptions::default() };
    let result = run::run(&binding, &implementation, &options).unwrap();

    assert!(result.divergences.is_empty());
    assert!(!result.coverage_floor_met);
    assert!(!result.earns_l3(), "cases without diversity are theatre, not evidence");
}

#[test]
fn the_seed_corpus_is_replayed_first() {
    let dir = TempDir::new().unwrap();
    let model = python_runner(&dir, "m.py", "    return len(x['password']) >= 8");
    let implementation = python_runner(&dir, "i.py", "    return len(x['password']) > 8");
    let (binding, implementation) = string_binding(model, implementation);

    let options = RunOptions {
        seed: 1,
        cases: 1,
        seeds_corpus: vec![json!({"password": "12345678"})],
        ..RunOptions::default()
    };
    let result = run::run(&binding, &implementation, &options).unwrap();
    assert_eq!(result.divergences.len(), 1, "a known-bad input must be tried first");
    assert_eq!(result.divergences[0].input["password"], json!("12345678"));
}

// --- evidence ------------------------------------------------------------

#[test]
fn a_clean_run_becomes_l3_evidence_keyed_by_clause() {
    let result = run::DrtResult {
        req_id: "REQ-A".into(),
        clause: Some("post".into()),
        seed: 4,
        cases_run: 1000,
        divergences: vec![],
        coverage: run::Coverage {
            input_constructors_seen: vec!["value".into()],
            input_constructors_total: 1,
            output_variants_seen: vec!["true".into(), "false".into()],
        },
        coverage_floor_met: true,
        duration_ms: 12,
        incidents: vec![],
    };
    let hashes = BTreeMap::from([("model".to_string(), "v1:aaa".to_string())]);
    let record = run::to_evidence(&result, hashes, "4.29.1".into(), None);

    assert_eq!(record.level, tracelean_lib::trace::Level::L3);
    assert_eq!(record.key.clause.as_deref(), Some("post"));
    assert_eq!(record.key.bond, tracelean_lib::trace::Bond::ModelImpl);
}

#[test]
fn a_run_with_divergences_does_not_earn_l3() {
    let result = run::DrtResult {
        req_id: "REQ-A".into(),
        clause: None,
        seed: 1,
        cases_run: 10,
        divergences: vec![run::Divergence {
            case: 1,
            input: json!({}),
            op: "default".into(),
            model: run::ReplySummary { output: Some(json!(true)), error: None },
            implementation: run::ReplySummary { output: Some(json!(false)), error: None },
            shrunk_steps: 0,
            triage: Triage::Untriaged,
        }],
        coverage: run::Coverage::default(),
        coverage_floor_met: true,
        duration_ms: 1,
        incidents: vec![],
    };
    let record = run::to_evidence(&result, BTreeMap::new(), "4.29.1".into(), None);
    assert_eq!(record.level, tracelean_lib::trace::Level::L1);
}

#[test]
fn seeds_round_trip_and_deduplicate() {
    let dir = TempDir::new().unwrap();
    run::append_seeds(dir.path(), "REQ-A", &[json!({"a": 1}), json!({"a": 2})]).unwrap();
    run::append_seeds(dir.path(), "REQ-A", &[json!({"a": 1})]).unwrap();
    let seeds = run::load_seeds(dir.path(), "REQ-A");
    assert_eq!(seeds.len(), 2, "a repeated witness is stored once");
}

// --- configuration and scaffolding --------------------------------------

#[test]
fn config_round_trips() {
    let dir = TempDir::new().unwrap();
    let binding = config::scaffold_binding("REQ-A", Some("post"), "rust");
    let config = DrtConfig { bindings: vec![binding] };
    config.save(dir.path()).unwrap();

    let loaded = DrtConfig::load(dir.path()).unwrap();
    assert_eq!(loaded.bindings.len(), 1);
    assert_eq!(loaded.bindings[0].req_id, "REQ-A");
}

#[test]
fn a_requirement_level_binding_serves_a_clause_without_its_own() {
    let config = DrtConfig {
        bindings: vec![config::scaffold_binding("REQ-A", None, "rust")],
    };
    assert!(config.binding("REQ-A", Some("post")).is_some());
    assert!(config.binding("REQ-B", Some("post")).is_none());
}

#[test]
fn module_names_are_legal_lean_identifiers() {
    assert_eq!(lean_runner::module_name("REQ-AUTH-03"), "DrtREQAUTH03");
    assert!(!lean_runner::module_name("REQ-AUTH-03").contains('-'));
}

#[test]
fn the_generated_runner_flushes_every_line() {
    // Without the flush, Lean's block-buffered stdout deadlocks the first case.
    let source = lean_runner::runner_source("DrtX", &["Auth".into()], &[], "  .ok c.input");
    assert!(source.contains("stdout.flush"));
    assert!(source.contains("import Auth"));
    assert!(source.contains("deriving FromJson, ToJson"));
}

#[test]
fn generate_writes_a_package_without_a_toolchain() {
    let dir = TempDir::new().unwrap();
    let package = lean_runner::generate(
        dir.path(),
        "REQ-AUTH-03",
        "../../model",
        "authModel",
        &["Auth".into()],
        &[],
        "  .ok c.input",
    )
    .unwrap();
    assert!(package.join("lakefile.lean").exists());
    assert!(package.join("DrtREQAUTH03.lean").exists());
    let lakefile = std::fs::read_to_string(package.join("lakefile.lean")).unwrap();
    assert!(lakefile.contains("lean_exe drtRunner"));
    assert!(lakefile.contains("require authModel from \"../../model\""));
}

#[test]
fn building_without_a_toolchain_explains_itself() {
    // There is no Lean on this machine, and the message a user gets matters:
    // "not configured" is a different problem from "broken".
    let dir = TempDir::new().unwrap();
    if lean_runner::toolchain().is_ok() {
        return; // a real toolchain is present; nothing to assert about its absence
    }
    let error = lean_runner::build(dir.path()).unwrap_err();
    assert!(
        error.contains("elan") || error.contains("Lean toolchain"),
        "unhelpful error: {error}"
    );
}

#[test]
fn output_variants_are_named_by_shape() {
    assert_eq!(output_variant(&json!("weakPassword")), "weakPassword");
    assert_eq!(output_variant(&json!({"locked": 5})), "locked");
    assert_eq!(output_variant(&json!(true)), "true");
}

#[test]
fn unused_import_guard() {
    // Keeps `PathBuf` meaningful in this file's imports.
    let _: PathBuf = PathBuf::from(".");
}

// --- deriving the runner from `@models` annotations ----------------------

#[test]
fn module_names_follow_the_file_path() {
    use std::path::Path;
    assert_eq!(
        lean_runner::module_for_path(Path::new("Auth/Login.lean")).as_deref(),
        Some("Auth.Login")
    );
    assert_eq!(
        lean_runner::module_for_path(Path::new("Model.lean")).as_deref(),
        Some("Model")
    );
}

#[test]
fn declarations_are_read_with_their_arguments() {
    let (name, args) =
        lean_runner::parse_declaration("def login (user : String) (password : String) : Bool :=\n  true")
            .unwrap();
    assert_eq!(name, "login");
    assert_eq!(args, vec!["user", "password"]);
}

#[test]
fn a_binder_group_contributes_every_name_in_it() {
    let (_, args) = lean_runner::parse_declaration("def f (a b : Nat) : Nat := a + b").unwrap();
    assert_eq!(args, vec!["a", "b"]);
}

#[test]
fn a_nullary_definition_has_no_arguments() {
    let (name, args) = lean_runner::parse_declaration("def maxUpload : Nat := 100").unwrap();
    assert_eq!(name, "maxUpload");
    assert!(args.is_empty());
}

#[test]
fn an_unparseable_declaration_refuses_rather_than_guessing() {
    // A wrong guess compiles and answers the wrong question, which is worse
    // than saying so.
    assert!(lean_runner::parse_declaration("inductive AuthError where\n  | weakPassword").is_none());
}

#[test]
fn the_dispatch_reads_arguments_by_name_and_rejects_unknown_ops() {
    let entries = vec![lean_runner::ModelEntry {
        module: "Auth".into(),
        function: "login".into(),
        op: "default".into(),
        arguments: vec!["password".into()],
        argument_types: vec!["String".into()],
        whole_input: false,
    }];
    let dispatch = lean_runner::dispatch_for(&entries);
    assert!(dispatch.contains("\"default\""));
    assert!(dispatch.contains("getObjValAs? _ \"password\""));
    assert!(dispatch.contains("unknown op"));
}

#[test]
fn a_nullary_entry_dispatches_without_argument_decoding() {
    let entries = vec![lean_runner::ModelEntry {
        module: "Auth".into(),
        function: "maxUpload".into(),
        op: "default".into(),
        arguments: vec![],
        argument_types: vec![],
        whole_input: false,
    }];
    let dispatch = lean_runner::dispatch_for(&entries);
    assert!(dispatch.contains("toJson (maxUpload)"));
    assert!(!dispatch.contains("getObjValAs?"));
}

#[test]
fn no_entries_produces_an_explaining_dispatch_not_broken_lean() {
    let dispatch = lean_runner::dispatch_for(&[]);
    assert!(dispatch.contains(".error"));
    assert!(dispatch.contains("no model entry points"));
}

/// `.tracelean/drt.json` is hand-written and version-controlled, so every
/// variant of every type in it has to survive a round trip. `MinOutputVariants`
/// did not: as a tuple variant under `#[serde(tag = "kind")]` it could not be
/// serialized at all, and a project that chose that floor would have found out
/// when saving its config failed.
#[test]
fn every_coverage_floor_round_trips_through_the_config_file() {
    for floor in [
        CoverageFloor::None,
        CoverageFloor::AllInputConstructors,
        CoverageFloor::MinOutputVariants { n: 3 },
    ] {
        let json = serde_json::to_string(&floor).expect("coverage floor must serialize");
        let back: CoverageFloor = serde_json::from_str(&json).expect("and parse back");
        assert_eq!(back, floor, "round trip changed the floor: {}", json);
    }
}

/// The example project ships a hand-written `.tracelean/drt.json`. It is the
/// only part of the differential-testing setup a user writes by hand, so a
/// typo in the shipped one would teach the wrong syntax to everybody who copies
/// it. This parses the real file.
#[test]
fn the_example_projects_drt_config_parses() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("example");
    if !root.join(".tracelean/drt.json").is_file() {
        eprintln!("no example project — skipping");
        return;
    }
    let config = DrtConfig::load(&root).expect("the shipped drt.json must parse");
    assert!(
        !config.bindings.is_empty(),
        "the example must demonstrate at least one binding"
    );
    for binding in &config.bindings {
        assert!(!binding.model.cmd.is_empty(), "{} has no model command", binding.req_id);
        assert!(
            binding.implementation.entry.contains("::"),
            "{} names no function to call",
            binding.req_id
        );
    }
}

// --- schema inference from the model's own types -------------------------

use tracelean_lib::drt::infer::{self, InferError};

const MODEL: &str = r#"
structure Order where
  subtotalCents : Nat
  remote        : Bool
  deriving Repr, FromJson, ToJson

structure Priced where
  discountCents : Nat
  totalCents    : Nat

inductive Outcome where
  | accepted
  | rejected : String

def discountCents (subtotal : Nat) : Nat :=
  if subtotal >= 5000 then subtotal * 10 / 100 else 0

def shippingCents (afterDiscount : Nat) (remote : Bool) : Nat :=
  if afterDiscount >= 10000 then 0 else 599

def price (o : Order) : Priced :=
  { discountCents := 0, totalCents := o.subtotalCents }

def classify (o : Order) (why : Outcome) : Bool := true

def unsupported (f : Nat -> Nat) : Nat := f 0
"#;

#[test]
fn a_single_structure_argument_becomes_the_input_schema_itself() {
    // The natural reading, and what a hand-written binding does anyway: the
    // case payload *is* the structure.
    let schema = infer::infer_input(MODEL, "price").expect("should infer");
    match schema {
        Schema::Struct { fields } => {
            assert_eq!(fields.len(), 2);
            assert!(matches!(fields.get("subtotalCents"), Some(Schema::Nat { .. })));
            assert!(matches!(fields.get("remote"), Some(Schema::Bool)));
        }
        other => panic!("expected a struct, got {other:?}"),
    }
}

#[test]
fn several_arguments_become_a_struct_keyed_by_their_names() {
    let schema = infer::infer_input(MODEL, "shippingCents").expect("should infer");
    match schema {
        Schema::Struct { fields } => {
            assert!(matches!(fields.get("afterDiscount"), Some(Schema::Nat { .. })));
            assert!(matches!(fields.get("remote"), Some(Schema::Bool)));
        }
        other => panic!("expected a struct, got {other:?}"),
    }
}

#[test]
fn a_scalar_argument_is_still_wrapped_so_the_payload_is_one_json_value() {
    let schema = infer::infer_input(MODEL, "discountCents").expect("should infer");
    match schema {
        Schema::Struct { fields } => {
            assert!(matches!(fields.get("subtotal"), Some(Schema::Nat { .. })));
        }
        other => panic!("expected a struct, got {other:?}"),
    }
}

#[test]
fn inductives_become_enums_with_their_payloads() {
    let schema = infer::infer_input(MODEL, "classify").expect("should infer");
    let Schema::Struct { fields } = schema else { panic!("expected a struct") };
    match fields.get("why") {
        Some(Schema::Enum { variants }) => {
            assert!(matches!(variants.get("accepted"), Some(None)));
            assert!(matches!(variants.get("rejected"), Some(Some(Schema::Str { .. }))));
        }
        other => panic!("expected an enum, got {other:?}"),
    }
}

/// The important failure: a type outside the grammar names itself and stops.
/// Guessing here would produce a generator that exercises the wrong values,
/// and a differential run that finds nothing is *evidence* — so a wrong
/// generator is worse than no generator at all.
#[test]
fn an_uninferable_type_names_itself_rather_than_being_guessed() {
    let err = infer::infer_input(MODEL, "unsupported").unwrap_err();
    match &err {
        InferError::Unsupported { declaration, argument, ty } => {
            assert_eq!(declaration, "unsupported");
            assert_eq!(argument, "f");
            assert!(ty.contains("Nat"), "the message must name the offending type: {ty}");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
    let message = err.to_string();
    assert!(message.contains("Declare this binding's input by hand"), "{message}");
}

#[test]
fn a_declaration_that_is_not_there_says_so() {
    assert_eq!(
        infer::infer_input(MODEL, "nope").unwrap_err(),
        InferError::NoSuchDeclaration("nope".into())
    );
}

/// The example project's hand-written binding is the reference: inference must
/// agree with what a careful person wrote, or one of the two is wrong.
#[test]
fn inference_agrees_with_the_example_projects_hand_written_binding() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("example");
    let model = root.join("formal/Checkout.lean");
    if !model.is_file() {
        eprintln!("no example project — skipping");
        return;
    }
    let source = std::fs::read_to_string(&model).unwrap();
    let inferred = infer::infer_input(&source, "price").expect("should infer");

    let config = DrtConfig::load(&root).expect("shipped config parses");
    let binding = config
        .binding("REQ-CHECKOUT", Some("total"))
        .expect("the example binds REQ-CHECKOUT.total");

    let (Schema::Struct { fields: a }, Schema::Struct { fields: b }) =
        (&inferred, &binding.input)
    else {
        panic!("both should be structs");
    };
    assert_eq!(
        a.keys().collect::<Vec<_>>(),
        b.keys().collect::<Vec<_>>(),
        "inferred fields must match the hand-written ones"
    );
}

// --- binding a clause, as one action -------------------------------------

use tracelean_lib::commands::Command as TlCommand;
use tracelean_lib::drt::bind;

fn example_project() -> Option<PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("example");
    root.join("product").is_dir().then_some(root)
}

#[test]
fn a_proposal_derives_the_schema_the_adapter_and_the_binding() {
    let Some(root) = example_project() else { return };
    let index = tracelean_lib::trace::build(&root);
    let proposal = bind::propose(&root, &index, "REQ-SHIPPING", Some("flat"))
        .expect("shipping.flat has a model and an implementation");

    assert_eq!(proposal.language, "python");
    assert_eq!(proposal.impl_symbol, "shipping_cents");
    // A proposal writes no harness file in any language: the runner shipped
    // with TraceLean calls the function the binding names.
    let call = &proposal.binding.implementation;
    assert_eq!(call.language, "python");
    assert!(
        call.entry.ends_with("::shipping_cents"),
        "the binding must name the implementation function: {}",
        call.entry
    );
    assert_eq!(
        call.params.get("afterDiscount").map(String::as_str),
        Some("after_discount_cents"),
        "the model and the implementation spell this argument differently, and the \
         binding is where that correspondence is recorded: {:?}",
        call.params
    );
    assert!(proposal.config_after.contains("REQ-SHIPPING"));
}

/// The point of routing through the command stream: an accidental binding is
/// one undo away rather than a manual cleanup across three files.
#[test]
fn applying_a_proposal_is_one_undoable_batch() {
    let Some(root) = example_project() else { return };
    let index = tracelean_lib::trace::build(&root);
    let proposal = bind::propose(&root, &index, "REQ-SHIPPING", Some("flat")).unwrap();
    let command = bind::apply(&root, &proposal).unwrap();

    let TlCommand::Batch { commands } = &command else {
        panic!("expected one batch, got {command:?}")
    };
    assert!(
        !commands.is_empty(),
        "the binding has to change the config at least: {commands:?}"
    );
    assert!(
        commands.iter().any(|c| format!("{c:?}").contains("drt.json")),
        "the config entry is the binding: {commands:?}"
    );
    assert!(
        matches!(command.inverse(), TlCommand::Batch { .. }),
        "the whole binding must invert as a unit"
    );
}

/// A clause with no model cannot be bound, and the message says why rather than
/// producing an adapter that compares an implementation against nothing.
#[test]
fn a_clause_with_no_model_refuses_with_a_reason() {
    let Some(root) = example_project() else { return };
    let index = tracelean_lib::trace::build(&root);
    let err = bind::propose(&root, &index, "REQ-RECEIPT", Some("lines")).unwrap_err();
    assert!(err.contains("`@models`"), "got: {err}");
}

/// A Rust implementation gets a skeleton and an explicit statement that it is a
/// skeleton — better than a generated `Cargo.toml` edit the user did not ask
/// for, and far better than a silent TODO.
#[test]
fn a_rust_implementation_says_what_is_left_to_do() {
    let Some(root) = example_project() else { return };
    let index = tracelean_lib::trace::build(&root);
    // REQ-RECEIPT.lines is Rust, but has no model; use the checker's own view
    // to find any Rust-implemented clause that does have one, and skip if the
    // example has none.
    let rust_clause = index.links.iter().find(|l| {
        l.role == tracelean_lib::trace::Role::Implements
            && l.anchor.file.extension().and_then(|e| e.to_str()) == Some("rs")
            && index
                .links_for_clause(&l.req_id, l.clause.as_deref())
                .iter()
                .any(|other| other.role == tracelean_lib::trace::Role::Models)
    });
    let Some(link) = rust_clause else {
        eprintln!("no Rust-implemented modeled clause in the example — skipping");
        return;
    };
    let proposal =
        bind::propose(&root, &index, &link.req_id, link.clause.as_deref()).unwrap();
    assert!(
        proposal.problems.iter().any(|p| p.contains("Rust")),
        "a Rust adapter is not ready to run and must say so: {:?}",
        proposal.problems
    );
}

/// Materialize bindings into the example project. Not part of the suite: it
/// writes files. Run with
/// `cargo test -p tracelean-tests materialize_example_bindings -- --ignored`.
#[test]
#[ignore]
fn materialize_example_bindings() {
    let root = example_project().expect("example project");
    for (req, clause) in [
        ("REQ-CHECKOUT", Some("total")),
        ("REQ-DISCOUNT", Some("tiers")),
        ("REQ-DISCOUNT", Some("rounding")),
        ("REQ-SHIPPING", Some("free")),
        ("REQ-SHIPPING", Some("flat")),
        ("REQ-SHIPPING", Some("remote")),
    ] {
        let index = tracelean_lib::trace::build(&root);
        let proposal = bind::propose(&root, &index, req, clause).expect("propose");
        std::fs::write(root.join(".tracelean/drt.json"), &proposal.config_after).unwrap();
        println!("bound {req}{:?}", clause);
        for p in &proposal.problems {
            println!("  problem: {p}");
        }
    }
}

/// Generate and build the example project's model runner. Not part of the
/// suite: it writes files and needs a Lean toolchain. Run with
/// `cargo test -p tracelean-tests build_example_runner -- --ignored --nocapture`.
#[test]
#[ignore]
fn build_example_runner() {
    let root = example_project().expect("example project");
    let mut state = tracelean_lib::AppState::new();
    state.set_project_root(root);
    println!("{}", tracelean_lib::service::drt_build_runner(&state).unwrap());
}

/// Run a real differential test over every binding in the example project.
/// Not part of the suite: it needs a built Lean runner. Run with
/// `cargo test -p tracelean-tests run_example_drt -- --ignored --nocapture`.
#[test]
#[ignore]
fn run_example_drt() {
    let root = example_project().expect("example project");
    let config = tracelean_lib::drt::DrtConfig::load(&root).unwrap();
    let mut state = tracelean_lib::AppState::new();
    state.set_project_root(root.clone());

    for binding in &config.bindings {
        let clause = binding.clause.as_deref();
        let label = format!(
            "{}{}",
            binding.req_id,
            clause.map(|c| format!(".{c}")).unwrap_or_default()
        );
        let result = tracelean_lib::service::drt_run(&state, &binding.req_id, clause, 42, 2000)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        println!(
            "{label}: {} cases, {} divergence(s), floor {}",
            result.cases_run,
            result.divergences.len(),
            if result.coverage_floor_met { "met" } else { "MISSED" }
        );
        assert_eq!(result.cases_run, 2000, "{label} did not run");

        // REQ-DISCOUNT.cap is the example's planted defect: the model applies
        // the cap the requirement asks for and `engine/pricing.py` does not.
        // It has to diverge, and the run that found it is what the example
        // exists to demonstrate. Everything else agreeing is the other half of
        // the same claim -- a harness that reports divergences everywhere is
        // as useless as one that never does.
        if label == "REQ-DISCOUNT.cap" {
            assert!(
                !result.divergences.is_empty(),
                "the planted defect in capped_discount_cents was not found — either it was \
                 fixed, or the generator stopped reaching past the 5000-cent cap"
            );
            let witness = &result.divergences[0];
            assert_eq!(
                witness.model.output,
                Some(serde_json::json!(5000)),
                "the model caps the discount"
            );
            // Above 33,334 cents the tier discount exceeds the cap. Every
            // witness has to be in that region, whether the generator found it
            // or the saved corpus replayed it -- a divergence reported below it
            // would mean the two sides disagree about something else.
            let subtotal = witness.input["subtotal"].as_u64().expect("a subtotal");
            assert!(subtotal > 33_334, "witness below the cap boundary: {witness:?}");
            assert!(
                !tracelean_lib::drt::run::load_seeds(&root, "REQ-DISCOUNT").is_empty(),
                "a divergence that was found once must be saved for replay"
            );
            assert!(
                !result.earns_l3(),
                "a run that found a divergence must not earn L3"
            );
            continue;
        }

        assert!(
            result.divergences.is_empty(),
            "{label} diverged: {:?}",
            result.divergences.first()
        );
    }
}

// --- one runner for the whole project ------------------------------------

#[test]
fn an_op_is_qualified_by_requirement_and_clause() {
    // One runner binary serves every binding, dispatching on `op`. When every
    // binding called itself "default" the generated `match` had several arms
    // with the same literal, the first one won, and every other requirement was
    // answered by the wrong model function -- silently, since a wrong answer is
    // still an answer.
    use tracelean_lib::drt::config::qualified_op;
    assert_eq!(qualified_op("REQ-SHIPPING", Some("flat")), "REQ-SHIPPING.flat");
    assert_eq!(qualified_op("REQ-SHIPPING", None), "REQ-SHIPPING");
    assert_ne!(
        qualified_op("REQ-SHIPPING", Some("flat")),
        qualified_op("REQ-SHIPPING", Some("remote"))
    );
}

#[test]
fn a_proposal_gives_its_binding_a_unique_op() {
    let Some(root) = example_project() else { return };
    let index = tracelean_lib::trace::build(&root);
    let flat = bind::propose(&root, &index, "REQ-SHIPPING", Some("flat")).unwrap();
    let remote = bind::propose(&root, &index, "REQ-SHIPPING", Some("remote")).unwrap();
    assert_ne!(flat.binding.op, remote.binding.op);
    // The op is what the shipped runner dispatches on, so two clauses of one
    // requirement must not collide: the first arm would answer both.
    assert!(flat.binding.op.contains("flat"));
}

#[test]
fn a_proposal_records_a_keyword_mismatch_in_the_binding_and_says_so() {
    // The model says `subtotal` where Python says `subtotal_cents`. The binding
    // records the correspondence so the runner can call the function, and the
    // proposal says it was inferred from declaration order rather than letting
    // a guess pass as a fact.
    let Some(root) = example_project() else { return };
    let index = tracelean_lib::trace::build(&root);
    let proposal = bind::propose(&root, &index, "REQ-DISCOUNT", Some("rounding")).unwrap();
    assert!(
        proposal.problems.iter().any(|p| p.contains("subtotal_cents")),
        "expected a keyword-mismatch problem, got {:?}",
        proposal.problems
    );
    assert_eq!(
        proposal
            .binding
            .implementation
            .params
            .get("subtotal")
            .map(String::as_str),
        Some("subtotal_cents"),
        "the mismatch must be resolved in the binding, not left for a human to patch in code"
    );
}

#[test]
fn a_lakefile_declares_a_name_that_is_not_its_directory() {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("lakefile.toml"),
        "name = \"checkout-model\"\ndefaultTargets = [\"Checkout\"]\n",
    )
    .unwrap();
    assert_eq!(
        lean_runner::lake_package_name(dir.path()).as_deref(),
        Some("checkout-model")
    );

    let other = TempDir::new().unwrap();
    std::fs::write(other.path().join("lakefile.lean"), "package «my-pkg» where\n").unwrap();
    assert_eq!(lean_runner::lake_package_name(other.path()).as_deref(), Some("my-pkg"));
}

#[test]
fn a_require_naming_the_package_wrongly_is_refused_before_lake_sees_it() {
    // Not a style check. Lake 4.12 answers `require X from "dir"` where `dir`
    // declares itself `Y` by DELETING dir -- it prints
    // "package '«Y»' was required as 'X'" and removes the source tree on the
    // way. That cost this project its entire Lean model once. The name is
    // derived correctly now, so this guard should never fire; it exists because
    // the failure mode is silent data loss outside TraceLean's own directory.
    let root = TempDir::new().unwrap();
    let model = root.path().join("model");
    std::fs::create_dir_all(&model).unwrap();
    std::fs::write(model.join("lakefile.toml"), "name = \"model-pkg\"\n").unwrap();

    let package = lean_runner::package_dir(root.path());
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("lakefile.lean"),
        "package drt where\nrequire model from \"../../model\"\n",
    )
    .unwrap();

    let error = lean_runner::check_requires(&package).unwrap_err();
    assert!(error.contains("model-pkg"), "{error}");
    assert!(error.contains("deleting"), "{error}");
    assert!(model.exists(), "the guard must not touch anything");

    // The name the generator now derives is accepted.
    std::fs::write(
        package.join("lakefile.lean"),
        "package drt where\nrequire «model-pkg» from \"../../model\"\n",
    )
    .unwrap();
    assert!(lean_runner::check_requires(&package).is_ok());
}

#[test]
fn a_declaration_is_named_with_the_namespace_it_is_written_in() {
    // The runner imports the model rather than opening it, so `discountCents`
    // fails to elaborate: the declaration is `Checkout.discountCents`. The
    // tree-sitter grammar parses `namespace Foo` and its `end Foo` as two flat
    // siblings, so the nesting is read from the source.
    let source = "namespace Checkout\n\ndef f : Nat := 0\n\nend Checkout\n\ndef g : Nat := 1\n";
    assert_eq!(lean_runner::namespace_prefix(source, 2).as_deref(), Some("Checkout"));
    assert_eq!(lean_runner::namespace_prefix(source, 6), None);

    let nested = "namespace A\nnamespace B\ndef f : Nat := 0\n";
    assert_eq!(lean_runner::namespace_prefix(nested, 2).as_deref(), Some("A.B"));

    // A `section` scopes variables, not names, but its `end` still matches.
    let sectioned = "namespace A\nsection\ndef f : Nat := 0\nend\nend A\n";
    assert_eq!(lean_runner::namespace_prefix(sectioned, 2).as_deref(), Some("A"));
}

#[test]
fn a_flattened_structure_argument_is_decoded_whole() {
    // `infer_input` gives a model taking one `Order` the schema of `Order`
    // itself, not `{"o": {...}}`. A dispatch that reads a field named after the
    // binder would look for `"o"` and never find it -- every case an error,
    // every case a divergence.
    let entries = vec![lean_runner::ModelEntry {
        module: "Checkout".into(),
        function: "Checkout.price".into(),
        op: "REQ-CHECKOUT.total".into(),
        arguments: vec!["o".into()],
        argument_types: vec!["Order".into()],
        whole_input: true,
    }];
    let dispatch = lean_runner::dispatch_for(&entries);
    assert!(dispatch.contains(":= Order"), "{dispatch}");
    assert!(!dispatch.contains("getObjValAs? _ \"o\""), "{dispatch}");
    assert!(dispatch.contains("Checkout.price o"), "{dispatch}");
}

#[test]
fn the_generator_samples_the_thresholds_the_model_names() {
    // An unbounded `Nat` is drawn from the whole 32-bit range, so a model
    // branching at 5000 and 20000 is tested above both in nearly every case --
    // and the coverage floor still reports "met". The boundaries come from the
    // model's own literals, where every threshold it has is written down.
    let source = "def discountCents (subtotal : Nat) : Nat :=\n  \
                  if subtotal >= 20000 then subtotal * 15 / 100\n  \
                  else if subtotal >= 5000 then subtotal * 10 / 100\n  else 0\n";
    let edges = infer::boundaries(source);
    for expected in [4999, 5000, 5001, 19999, 20000, 20001] {
        assert!(edges.contains(&expected), "missing {expected} in {edges:?}");
    }

    let schema = infer::infer_input(source, "discountCents").unwrap();
    let tracelean_lib::drt::Schema::Struct { fields } = &schema else {
        panic!("expected a struct, got {schema:?}");
    };
    let tracelean_lib::drt::Schema::Nat { edges, .. } = &fields["subtotal"] else {
        panic!("expected a Nat");
    };
    assert!(edges.contains(&20000));

    // And a case actually lands on one.
    let generated: Vec<u64> = (0..400)
        .filter_map(|seed| {
            let mut rng = tracelean_lib::drt::gen::Rng::new(seed);
            tracelean_lib::drt::gen::generate(&schema, &mut rng)["subtotal"].as_u64()
        })
        .collect();
    assert!(
        generated.iter().any(|v| (4999..=5001).contains(v)),
        "no case reached the 10% band boundary"
    );
}

/// Print the example project's findings and coverage. Not part of the suite:
/// it is a look at the fixture, not an assertion about it. Run with
/// `cargo test -p tracelean-tests probe_example_status -- --ignored --nocapture`.
#[test]
#[ignore]
fn probe_example_status() {
    let root = example_project().expect("example project");
    let mut state = tracelean_lib::AppState::new();
    state.set_project_root(root);
    for summary in tracelean_lib::service::trace_overview(&state).unwrap() {
        println!("{summary:?}");
    }
    println!("---");
    for finding in tracelean_lib::service::trace_findings(&state).unwrap() {
        println!("{:?} {} {}", finding.kind, finding.blocking, finding.message);
    }
}

/// Print the example project's graph. Not part of the suite. Run with
/// `cargo test -p tracelean-tests probe_project_graph -- --ignored --nocapture`.
#[test]
#[ignore]
fn probe_project_graph() {
    let root = example_project().expect("example project");
    let mut state = tracelean_lib::AppState::new();
    state.set_project_root(root);
    let graph = tracelean_lib::service::trace_project_graph(&state, None).unwrap();
    println!("{} nodes, {} edges, {} omitted", graph.nodes.len(), graph.edges.len(), graph.omitted_declarations);
    for n in &graph.nodes {
        println!(
            "{:?} {} lines={} reqs={:?} roles={:?} assurance={:?} findings={}",
            n.kind, n.id, n.lines, n.requirements, n.roles, n.assurance, n.findings.len()
        );
    }
    for e in graph.edges.iter().filter(|e| e.kind == tracelean_lib::trace::graph::EdgeKind::References) {
        println!("ref {} -> {}", e.from, e.to);
    }
}

/// Show what the agent sees from `query_project_graph`. Run with
/// `cargo test -p tracelean-tests probe_agent_graph_tool -- --ignored --nocapture`.
#[test]
#[ignore]
fn probe_agent_graph_tool() {
    let root = example_project().expect("example project");
    let mut state = tracelean_lib::AppState::new();
    let symbols = tracelean_lib::SymbolTable::new();
    let perms = tracelean_lib::AgentPermissions::full_access("probe");
    let call = tracelean_lib::ai::ToolCall {
        name: "query_project_graph".into(),
        arguments: serde_json::json!({ "req_id": "REQ-SHIPPING" }),
    };
    let result = tracelean_lib::ai::tool_executor::execute_tool(
        &call, &root, &mut state, &symbols, &perms,
    );
    println!("success={}\n{}", result.success, result.content);
}

// --- the shipped runner ----------------------------------------------------
//
// The project writes no harness code: TraceLean's own runner imports the
// function a binding names and calls it. These tests are the guard on that,
// and they need no Lean toolchain because they only exercise the
// implementation side of the protocol.

fn python_project(source: &str) -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("engine")).unwrap();
    std::fs::write(dir.path().join("engine/thing.py"), source).unwrap();
    dir
}

fn call_binding(entry: &str, params: &[(&str, &str)]) -> Binding {
    Binding {
        req_id: "REQ-A".into(),
        clause: None,
        op: "REQ-A".into(),
        model: RunnerSpec { cmd: vec!["true".into()], cwd: None, env: BTreeMap::new() },
        implementation: CallSpec {
            language: "python".into(),
            entry: entry.into(),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            convert: None,
        },
        input: Schema::Struct { fields: BTreeMap::new() },
        coverage_floor: CoverageFloor::None,
    }
}

fn ask_once(root: &std::path::Path, binding: &Binding, input: serde_json::Value) -> Reply {
    let spec = binding
        .implementation
        .spec(root, std::slice::from_ref(binding))
        .expect("the shipped runner materializes");
    let mut runner = Runner::spawn(&spec).expect("runner starts");
    runner
        .ask(
            &Case { case: 1, op: binding.op.clone(), input },
            std::time::Duration::from_secs(20),
        )
        .expect("a reply")
}

#[test]
fn the_shipped_runner_calls_the_function_a_binding_names() {
    let dir = python_project("def price(subtotal_cents, remote):\n    return {\"total\": subtotal_cents + (400 if remote else 0)}\n");
    // The model spells this argument `subtotalCents`; Python spells it
    // `subtotal_cents`. The binding is where that is reconciled -- the one
    // project-specific fact in the whole arrangement.
    let binding = call_binding("engine/thing.py::price", &[("subtotalCents", "subtotal_cents")]);
    let reply = ask_once(
        dir.path(),
        &binding,
        serde_json::json!({ "subtotalCents": 1000, "remote": true }),
    );
    assert_eq!(reply.output, Some(serde_json::json!({ "total": 1400 })), "{reply:?}");
}

#[test]
fn the_shipped_runner_unwraps_a_dataclass_into_the_models_json_shape() {
    // Lean's `deriving ToJson` emits an object with named fields. A Python
    // implementation returning a dataclass is not that, and having to write an
    // adapter for so ordinary a return type is exactly the cost this removes.
    let dir = python_project(
        "import dataclasses\n\n@dataclasses.dataclass\nclass Priced:\n    discountCents: int\n    totalCents: int\n\ndef price(subtotal):\n    return Priced(discountCents=10, totalCents=subtotal - 10)\n",
    );
    let binding = call_binding("engine/thing.py::price", &[]);
    let reply = ask_once(dir.path(), &binding, serde_json::json!({ "subtotal": 100 }));
    assert_eq!(
        reply.output,
        Some(serde_json::json!({ "discountCents": 10, "totalCents": 90 })),
        "{reply:?}"
    );
}

#[test]
fn the_shipped_runner_reports_a_raise_instead_of_dying() {
    // Both sides failing is agreement; one side failing is a divergence. A
    // runner that died on a bad input would destroy the run instead.
    let dir = python_project("def price(subtotal):\n    raise ValueError('nope')\n");
    let binding = call_binding("engine/thing.py::price", &[]);
    let reply = ask_once(dir.path(), &binding, serde_json::json!({ "subtotal": 1 }));
    assert!(
        reply.error.as_deref().unwrap_or_default().contains("ValueError: nope"),
        "{reply:?}"
    );
    assert!(reply.output.is_none());
}

#[test]
fn a_binding_that_cannot_load_answers_its_own_cases_rather_than_killing_the_run() {
    let dir = python_project("def price(subtotal):\n    return subtotal\n");
    let binding = call_binding("engine/missing.py::price", &[]);
    let reply = ask_once(dir.path(), &binding, serde_json::json!({ "subtotal": 1 }));
    let error = reply.error.unwrap_or_default();
    assert!(error.contains("REQ-A"), "the reply must name the broken op: {error}");
    assert!(error.contains("missing.py"), "and what could not be loaded: {error}");
}


#[test]
fn a_binding_describes_a_call_and_nothing_else() {
    // There is deliberately no adapter form. Arbitrary code between the
    // implementation and the comparator could make a divergence disappear, and
    // that is the one place in this system where hiding one would be both easy
    // and invisible -- so a binding names a function and stops there.
    let call: CallSpec =
        serde_json::from_str(r#"{"language":"python","entry":"engine/thing.py::price"}"#)
            .expect("a binding parses");
    assert_eq!(call.entry, "engine/thing.py::price");

    let adapter = serde_json::from_str::<CallSpec>(r#"{"cmd":["python3","harness/mine.py"]}"#);
    assert!(adapter.is_err(), "a command is not a binding: {adapter:?}");
}

/// Bind the example's capped-discount clause. Ignored: it writes files.
#[test]
#[ignore]
fn bind_example_cap() {
    let root = example_project().expect("example project");
    let index = tracelean_lib::trace::build(&root);
    let proposal = bind::propose(&root, &index, "REQ-DISCOUNT", Some("cap")).expect("propose");
    std::fs::write(root.join(".tracelean/drt.json"), &proposal.config_after).unwrap();
    println!("bound {} -> {}", proposal.binding.op, proposal.binding.implementation.entry);
    for p in &proposal.problems {
        println!("  problem: {p}");
    }
}
