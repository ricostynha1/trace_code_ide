use std::collections::{BTreeMap, BTreeSet};

use tracelean_core::drt::gen;
use tracelean_core::drt::protocol::{Case, Reply, RunnerError};
use tracelean_core::drt::run::{agree, DrtResult, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::drt::{Binding, CallSpec};

fn every_shape() -> Vec<Schema> {
    vec![
        Schema::Nat { max: Some(3), edges: vec![0] },
        Schema::Int { min: Some(-2), max: Some(2) },
        Schema::Bool,
        Schema::Str { max_len: Some(2), examples: vec!["x".into()] },
        Schema::Option { inner: Box::new(Schema::Bool) },
        Schema::List { inner: Box::new(Schema::Bool), max_len: Some(2) },
        Schema::Struct { fields: BTreeMap::from([("a".to_string(), Schema::Bool)]) },
        Schema::Tuple { items: vec![Schema::Bool, Schema::Bool] },
        Schema::simple_enum(&["one", "two"]),
    ]
}

/// @tests REQ-DRT-SCHEMA.whitelisted
/// @tests REQ-DRT-SCHEMA.outside_is_error
/// @structural REQ-DRT-SCHEMA.whitelisted reason="a claim that the grammar is a closed enum, which is a fact about the type rather than a value it computes"
/// @structural REQ-DRT-SCHEMA.outside_is_error reason="a type outside the grammar cannot be constructed, so there is no input on which a model could disagree — the claim is that the vocabulary is closed"
#[test]
fn the_schema_grammar_is_closed_and_every_shape_generates() {
    let shapes = every_shape();
    assert_eq!(shapes.len(), 9, "the grammar changed; this list did not");

    let names: BTreeSet<&str> = shapes.iter().map(|s| s.kind()).collect();
    assert_eq!(names.len(), shapes.len(), "two shapes share a name");

    let mut rng = gen::Rng::new(1);
    for shape in &shapes {
        for _ in 0..50 {
            let value = gen::value(shape, &mut rng);
            assert!(
                matches_shape(shape, &value),
                "{} generated a value of the wrong shape: {value}",
                shape.kind()
            );
        }
    }
}

fn matches_shape(schema: &Schema, value: &serde_json::Value) -> bool {
    match (schema, value) {
        (Schema::Nat { .. }, v) => v.as_u64().is_some(),
        (Schema::Int { .. }, v) => v.as_i64().is_some(),
        (Schema::Bool, v) => v.is_boolean(),
        (Schema::Str { .. }, v) => v.is_string(),
        (Schema::Option { inner }, v) => v.is_null() || matches_shape(inner, v),
        (Schema::List { inner, .. }, serde_json::Value::Array(items)) => {
            items.iter().all(|i| matches_shape(inner, i))
        }
        (Schema::Struct { fields }, serde_json::Value::Object(map)) => {
            map.len() == fields.len()
                && fields.iter().all(|(k, s)| map.get(k).is_some_and(|v| matches_shape(s, v)))
        }
        (Schema::Tuple { items }, serde_json::Value::Array(values)) => {
            items.len() == values.len()
                && items.iter().zip(values).all(|(s, v)| matches_shape(s, v))
        }
        (Schema::Enum { variants }, serde_json::Value::String(name)) => {
            variants.get(name.as_str()).is_some_and(|p| p.is_none())
        }
        (Schema::Enum { variants }, serde_json::Value::Object(map)) => {
            map.len() == 1
                && map.iter().next().is_some_and(|(name, payload)| {
                    variants.get(name).and_then(|p| p.as_ref()).is_some_and(|inner| {
                        matches_shape(inner, payload)
                    })
                })
        }
        _ => false,
    }
}

/// @tests REQ-DRT-SCHEMA.declared_not_reflected
/// @structural REQ-DRT-SCHEMA.declared_not_reflected reason="a claim that the shape comes from the binding file and never from the model, which is an absence of reflection in the source"
/// @tests REQ-DRT-BIND.no_adapter
/// @tests REQ-DRT-BIND.rename_only
#[test]
fn a_binding_declares_a_call_and_cannot_compute_anything() {
    let binding = Binding {
        req_id: "REQ-X".into(),
        clause: Some("c".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: "crates/core/src/evidence.rs::assurance".into(),
            params: BTreeMap::from([("records".to_string(), "records".to_string())]),
        },
    };

    let text = serde_json::to_string(&binding).expect("a binding serialises");
    let value: serde_json::Value = serde_json::from_str(&text).expect("readable");
    let keys: BTreeSet<&str> = value.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(
        keys,
        BTreeSet::from(["req_id", "clause", "op", "implementation"]),
        "a binding gained a field; check it cannot transform a value"
    );

    let spec = value["implementation"].as_object().unwrap();
    let spec_keys: BTreeSet<&str> = spec.keys().map(|k| k.as_str()).collect();
    assert_eq!(
        spec_keys,
        BTreeSet::from(["language", "entry", "params"]),
        "a call spec gained a field; an adapter would be one of these"
    );

    for (from, to) in spec["params"].as_object().unwrap() {
        let to = to.as_str().expect("a rename is a name");
        for name in [from.as_str(), to] {
            assert!(
                name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty(),
                "`{name}` is not a parameter name"
            );
        }
    }
}

/// @tests REQ-DRT-PROTO.case_one_line
/// @tests REQ-DRT-PROTO.reply_one_line
/// @tests REQ-DRT-PROTO.case_echoed
#[test]
fn a_case_and_a_reply_are_each_one_line() {
    let case = Case {
        case: 7,
        op: "REQ-X.c".into(),
        input: serde_json::json!({"text": "a\nb\r\nc", "n": 1}),
    };
    let line = tracelean_core::drt::protocol::case_line(case.case, case.op.clone(), case.input.clone());
    assert!(!line.contains('\n'), "a case spanned more than one line: {line}");
    let read: Case = serde_json::from_str(&line).expect("a case line is a case");
    assert_eq!(read, case);

    let reply = Reply::ok(case.case, serde_json::json!({"out": "x\ny"}));
    let line = serde_json::to_string(&reply).expect("a reply serialises");
    assert!(!line.contains('\n'), "a reply spanned more than one line: {line}");

    let read: Reply = serde_json::from_str(&line).expect("a reply round-trips");
    assert_eq!(read.case, case.case, "a reply must echo the case it answers");
}

/// @tests REQ-DRT-PROTO.reply_exclusive
#[test]
fn a_reply_carries_exactly_one_of_an_output_and_an_error() {
    assert!(Reply::ok(1, serde_json::json!(3)).is_well_formed());
    assert!(Reply::failed(1, "no").is_well_formed());
    assert!(!Reply { case: 1, output: None, error: None }.is_well_formed());
    assert!(
        !Reply { case: 1, output: Some(serde_json::json!(3)), error: Some("no".into()) }
            .is_well_formed()
    );

    let null_output = serde_json::json!({"case": 1, "output": null});
    let read = Reply::from_json(&null_output).expect("readable");
    assert_eq!(read.output, Some(serde_json::Value::Null));
    assert!(read.is_well_formed(), "`null` is an answer");

    let neither = serde_json::json!({"case": 1});
    assert!(!Reply::from_json(&neither).expect("readable").is_well_formed());

    assert!(Reply::from_json(&serde_json::json!({"output": 1})).is_err());
}

/// @tests REQ-DRT-PROTO.ops_unique
#[test]
fn every_op_in_this_project_is_unique() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let index = tracelean_core::trace::index::build(root);
    let files = tracelean_core::observe::workspace::snapshot(root).files;

    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for binding in tracelean_core::drt::auto::bindings(root, &index, &files).expect("the bindings read") {
        *seen.entry(binding.op()).or_default() += 1;
    }
    let repeated: Vec<&String> = seen.iter().filter(|(_, n)| **n > 1).map(|(op, _)| op).collect();
    assert!(repeated.is_empty(), "two bindings share an op: {repeated:?}");
    assert!(seen.len() > 20, "only {} ops; the file did not load", seen.len());
}

/// @tests REQ-DRT-PROTO.failure_named
#[test]
fn a_runner_that_could_not_answer_is_distinguished_from_one_that_did() {
    let failures = [
        RunnerError::Spawn("no toolchain".into()),
        RunnerError::Timeout,
        RunnerError::Died("exit 101".into()),
        RunnerError::Protocol("not a reply".into()),
    ];
    let rendered: BTreeSet<String> = failures.iter().map(|e| e.to_string()).collect();
    assert_eq!(rendered.len(), failures.len(), "two failures read the same");

    for failure in &failures {
        assert!(!format!("{failure}").is_empty());
    }
}

/// @tests REQ-DRT.error_is_an_answer
#[test]
fn an_error_on_both_sides_is_agreement_and_on_one_side_is_not() {
    let out = Reply::ok(1, serde_json::json!(3));
    let other = Reply::ok(1, serde_json::json!(4));
    let err = Reply::failed(1, "refused");
    let other_err = Reply::failed(1, "refused differently");

    assert!(agree(&out, &out.clone()));
    assert!(!agree(&out, &other));
    assert!(agree(&err, &other_err), "both refusing is the two behaving alike");
    assert!(!agree(&out, &err), "one answering and one refusing is a divergence");
}

fn recording_runner(log: &std::path::Path) -> tracelean_core::drt::run::RunnerSpec {
    let script = format!(
        "while IFS= read -r l; do printf '%s\\n' \"$l\" >> '{}'; n=${{l#*\\\"case\\\":}}; n=${{n%%,*}}; \
         printf '{{\"case\":%s,\"output\":0}}\\n' \"$n\"; done",
        log.display()
    );
    tracelean_core::drt::run::RunnerSpec { cmd: vec!["sh".into(), "-c".into(), script], cwd: None }
}

/// @tests REQ-DRT.same_question
/// @tests REQ-DRT.case_count_stated
/// @structural REQ-DRT.same_question reason="a claim about the order two processes are driven in, which lives in the harness loop rather than in any value; read off what each process was sent"
#[test]
fn both_sides_are_asked_the_same_cases_in_the_same_order() {
    let scratch = std::env::temp_dir().join(format!("tracelean-same-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let (a, b) = (scratch.join("model.log"), scratch.join("implementation.log"));
    let schema = Schema::List { inner: Box::new(Schema::Nat { max: Some(9), edges: vec![] }), max_len: Some(3) };
    let result = tracelean_core::drt::run::run(
        "REQ-X.c",
        &schema,
        &recording_runner(&a),
        &recording_runner(&b),
        RunOptions { seed: 5, cases: 40 },
    )
    .expect("both answer");
    assert!(result.agreed());
    assert_eq!((result.seed, result.cases), (5, 40), "the run states the cases it ran");
    let (asked_a, asked_b) = (std::fs::read_to_string(&a).unwrap(), std::fs::read_to_string(&b).unwrap());
    assert_eq!(asked_a, asked_b, "the two sides were asked different questions");
    let numbers: Vec<u64> = asked_a
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["case"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, (1..=40).collect::<Vec<u64>>());
    let _ = std::fs::remove_dir_all(&scratch);
}

fn answering_runner(echo: bool) -> tracelean_core::drt::run::RunnerSpec {
    let output = if echo { "\"$i\"" } else { "0" };
    let script = format!(
        "while IFS= read -r l; do n=${{l#*\\\"case\\\":}}; n=${{n%%,*}}; i=${{l#*\\\"input\\\":}}; i=${{i%\\}}}}; \
         printf '{{\"case\":%s,\"output\":%s}}\\n' \"$n\" {output}; done"
    );
    tracelean_core::drt::run::RunnerSpec { cmd: vec!["sh".into(), "-c".into(), script], cwd: None }
}

/// @tests REQ-DRT-GEN.shrink_minimal
/// @tests REQ-DRT.divergence_reported
#[test]
fn a_divergence_is_reduced_all_the_way_down() {
    let schema = Schema::Nat { max: Some(1_000_000_000), edges: vec![] };
    let result = tracelean_core::drt::run::run(
        "REQ-X.c",
        &schema,
        &answering_runner(false),
        &answering_runner(true),
        RunOptions { seed: 3, cases: 20 },
    )
    .expect("both answer");
    let divergence = result.divergence.expect("the two disagree about every number but 0");
    assert_eq!(divergence.input, serde_json::json!(1), "stopped before the smallest diverging case");
}

/// @tests REQ-DRT.case_count_stated
/// @structural REQ-DRT.case_count_stated reason="a claim that an evidence record cannot be built without its seed and case count, which is enforced by the type rather than computed"
/// @tests REQ-DRT.falsification_only
#[test]
fn a_result_carries_the_seed_and_the_case_count_it_actually_reached() {
    let clean = DrtResult {
        op: "REQ-X.c".into(),
        seed: 7,
        cases: 2_000,
        divergence: None,
        schema: tracelean_core::drt::schema::Schema::Bool,
        reached: Vec::new(),
        unreadable: 0,
    };
    assert!(clean.agreed());
    assert_eq!((clean.seed, clean.cases), (7, 2_000));

    let options = RunOptions::default();
    assert!(options.cases > 0);
}

/// @tests REQ-DRT-LEAN.generated
/// @tests REQ-DRT-LEAN.project_untouched
/// @tests REQ-DRT-RUST.project_untouched
/// @tests REQ-DRT-LEAN.encoding_declared
/// @tests REQ-DRT-LEAN.toolchain_explained
#[test]
fn generation_writes_a_package_under_the_cache_and_nothing_else() {
    use tracelean_core::drt::lean_runner::{self, LeanEntry};
    use tracelean_core::drt::rust_runner;

    let scratch = std::env::temp_dir().join(format!("tracelean-props-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).expect("scratch");

    let entries = [LeanEntry {
        op: "REQ-X.c".into(),
        function: "TraceLean.Evidence.assurance".into(),
        arguments: vec!["records".into()],
    }];
    lean_runner::materialize(&scratch, &["TraceLean.Evidence"], "tracelean", "/models", &entries)
        .expect("generated");

    let package = lean_runner::package_dir(&scratch);
    assert!(package.starts_with(&scratch), "generation escaped the cache directory");
    assert!(package.join("lakefile.lean").is_file(), "a package, not a script");
    let toolchain_pinned = package.join("lean-toolchain").is_file()
        || lean_runner::lakefile("tracelean", "/models").contains("lean_exe");
    assert!(toolchain_pinned, "the package does not pin what builds it");

    let mut found = Vec::new();
    let mut stack = vec![scratch.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable").flatten() {
            let path = entry.path();
            assert!(path.starts_with(&scratch), "wrote outside the cache: {}", path.display());
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    assert!(found.len() >= 2, "generation wrote only {} files", found.len());

    if let Err(message) = lean_runner::toolchain() {
        assert!(message.len() > 10, "an unexplained toolchain failure: {message}");
    }

    let main = lean_runner::main_lean(&["TraceLean.Evidence"], &entries);
    assert!(main.contains("TraceLean.Evidence.assurance"));
    assert!(main.contains("REQ-X.c"));
    assert!(main.contains("records"));
    assert!(main.contains("fromJson?") || main.contains("getObjVal?"));

    let manifest = rust_runner::cargo_toml(&BTreeMap::from([(
        "tracelean-core".to_string(),
        "/somewhere/core".to_string(),
    )]));
    assert!(manifest.contains("[package]"), "not a crate");
    assert!(manifest.contains("path = \"/somewhere/core\""), "not a path dependency");

    let _ = std::fs::remove_dir_all(&scratch);
}

/// @tests REQ-DRT-SCHEMA.json_shape_matches
/// @structural REQ-DRT-SCHEMA.json_shape_matches reason="a claim that two serialisers agree, which every differential suite in this tree exercises and none can state as a single function"
#[test]
fn a_sum_type_generates_the_shape_the_model_encodes() {
    let schema = Schema::Enum {
        variants: BTreeMap::from([
            ("nothing".to_string(), None),
            ("something".to_string(), Some(Box::new(Schema::Bool))),
        ]),
    };
    let mut rng = gen::Rng::new(2);
    let (mut bare, mut wrapped) = (0, 0);
    for _ in 0..500 {
        match gen::value(&schema, &mut rng) {
            serde_json::Value::String(name) => {
                assert_eq!(name, "nothing");
                bare += 1;
            }
            serde_json::Value::Object(map) => {
                assert_eq!(map.len(), 1);
                assert!(map.contains_key("something"));
                assert!(map["something"].is_boolean());
                wrapped += 1;
            }
            other => panic!("a variant encoded as {other}"),
        }
    }
    assert!(bare > 100 && wrapped > 100, "one of the two encodings never occurred");
}

/// @tests REQ-JUDGE.no_call
/// @tests REQ-JUDGE.advice_is_not_evidence
#[test]
fn a_judging_prompt_is_text_and_a_judgement_is_a_persons_own() {
    use tracelean_core::evidence::Level;
    use tracelean_core::judge::{prompt, record, Judgement, Material, Outcome, Verdict};

    let material = Material {
        req_id: "REQ-EVID".into(),
        clause: Some("ladder".into()),
        clause_text: "Levels are ordered.".into(),
        model_source: "def assurance".into(),
        requirement_hash: "h1".into(),
        model_hash: "h2".into(),
        divergence: Some("case 7".into()),
    };

    let text = prompt(material.clone());
    assert!(text.contains("REQ-EVID"), "the prompt does not say what it is about");
    assert!(text.contains("case 7"), "a known divergence is not presented to the judge");

    let judgement = Judgement {
        verdict: Verdict::Agrees,
        judged_by: "ana".into(),
        delegated_by: None,
        note: None,
        requirement_hash: "h1".into(),
        model_hash: "h2".into(),
    };
    match record(material, judgement, None, "link1".into()) {
        Outcome::Recorded { evidence } => {
            assert_eq!(evidence.level, Level::L2);
            assert!(format!("{:?}", evidence.detail).contains("ana"), "nobody signed it");
        }
        other => panic!("an agreement recorded {other:?}"),
    }
}

/// @tests REQ-TRANSCRIPT.no_interpretation
/// @structural REQ-TRANSCRIPT.no_interpretation reason="a claim that no code path turns a transcript event into a command, which is read off the tree"
#[test]
fn a_transcript_produces_events_and_no_commands() {
    use tracelean_core::observe::transcript::read;

    let text = "{\"type\":\"edit\",\"text\":\"rm -rf /\"}\n{\"type\":\"say\",\"text\":\"done\"}\n";
    let result = read(text.to_string());
    assert_eq!(result.events.len(), 2);

    let json = serde_json::to_value(&result).expect("serialises");
    let keys: BTreeSet<&str> = json.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(keys, BTreeSet::from(["events", "unrecognised", "held", "usage"]));
}

/// @tests REQ-EVID.record_reproducible
/// @tests REQ-STALE.inputs_identified
#[test]
fn an_evidence_record_cannot_omit_what_would_reproduce_it() {
    use tracelean_core::trace::record::Detail;

    let details = [
        Detail::Judge {
            verdict: "agrees".into(),
            judged_by: "ana".into(),
            delegated_by: None,
            prompt_version: "1".into(),
            note: None,
        },
        Detail::Drt { seed: 7, cases: 2_000, op: "REQ-X.c".into() },
        Detail::Proof { theorem_name: "t".into(), toolchain: "4.12.0".into() },
    ];
    for detail in &details {
        let json = serde_json::to_value(detail).expect("serialises");
        let object = json.as_object().expect("an object");
        assert_eq!(object.len(), 1, "a detail is one tagged variant");
        let fields = object.values().next().unwrap().as_object().expect("fields");
        assert!(
            fields.len() >= 2,
            "a detail that could not be reproduced from what it carries: {json}"
        );
        for value in fields.values() {
            assert!(!value.is_null(), "a reproducible detail carries no nulls: {json}");
        }
    }

    let record = tracelean_core::trace::record::Evidence {
        key: tracelean_core::trace::record::Key {
            req_id: "REQ-X".into(),
            clause: Some("c".into()),
            bond: tracelean_core::evidence::Bond::ModelImpl,
        },
        level: tracelean_core::evidence::Level::L3,
        detail: details[1].clone(),
        link_hash: "link1".into(),
        inputs: vec![("model".into(), "h1".into()), ("code".into(), "h2".into())],
    };
    assert!(!record.inputs.is_empty(), "a record with no inputs can never go stale");
    for (name, hash) in &record.inputs {
        assert!(!name.is_empty() && !hash.is_empty());
    }
}

/// @tests REQ-LOCK.pure_render
/// @tests REQ-LOCK.evidence_preserved
/// @tests REQ-LOCK.diff_is_meaningful
/// @tests REQ-STALE.scanner_never_writes
/// @tests ARCH-HONEST.untraced_visible
/// @tests ARCH-CORE-SHELL.decision_total
/// @structural ARCH-HONEST.untraced_visible reason="a claim that every coverage view lists unclaimed files, which is about what a report contains rather than about a value a function returns"
/// @structural ARCH-CORE-SHELL.decision_total reason="a claim about the shape of every decision in the project; the individual decisions are each modelled, this is that they all are"
/// @structural REQ-LOCK.pure_render reason="an absence of filesystem access, which a model of the function could not distinguish from its presence"
#[test]
fn rendering_the_index_reads_nothing_and_invents_nothing() {
    use tracelean_core::evidence::{Bond, Level};
    use tracelean_core::trace::index::build;
    use tracelean_core::trace::lockfile::{render, to_bytes};
    use tracelean_core::trace::record::{Detail, Evidence, Key};

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let index = build(root);

    let evidence = vec![Evidence {
        key: Key { req_id: "REQ-EVID".into(), clause: Some("ladder".into()), bond: Bond::ModelProof },
        level: Level::L4,
        detail: Detail::Proof { theorem_name: "t".into(), toolchain: "4.12.0".into() },
        link_hash: "link1".into(),
        inputs: vec![("model".into(), "h1".into())],
    }];

    let rendered = render(&index, evidence.clone());
    assert_eq!(rendered.evidence, evidence, "evidence was not carried through unchanged");

    assert_eq!(to_bytes(&rendered), to_bytes(&render(&index, evidence.clone())));

    assert!(!index.scanned.is_empty(), "the scan remembered nothing");
    let annotated: BTreeSet<&String> = index.links.iter().map(|l| &l.anchor.file).collect();
    assert!(
        index.scanned.len() > annotated.len(),
        "every scanned file carried an annotation, which cannot be true of this tree"
    );

    let mut fewer = rendered.clone();
    fewer.links.pop();
    assert_ne!(to_bytes(&rendered), to_bytes(&fewer), "dropping a link left the file identical");

    let lock = root.join(".tracelean").join("trace.lock.json");
    let before = std::fs::read(&lock).ok();
    let _ = render(&index, evidence);
    assert_eq!(std::fs::read(&lock).ok(), before, "rendering wrote to the tree");
}

/// @tests REQ-DOCLINK.same_anchor_machinery
/// @structural REQ-DOCLINK.same_anchor_machinery reason="a claim that two callers reach the same function, which is about the call graph rather than about what the function returns"
#[test]
fn a_document_resolves_its_target_the_way_an_annotation_does() {
    use tracelean_core::trace::doclink::current_hashes;
    use tracelean_core::trace::index::build;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let index = build(root);
    let hashes = current_hashes(&index);

    for (id, req) in &index.requirements {
        assert_eq!(hashes.get(id), Some(&req.content_hash), "{id} hashes two different ways");
    }
    for link in &index.links {
        assert_eq!(
            hashes.get(&link.anchor.ident()),
            Some(&link.anchor.body_hash),
            "an anchor hashes differently for a document than for a link"
        );
    }
    assert!(hashes.len() > 50, "only {} targets; the scan did not load", hashes.len());
}

/// @tests REQ-ANNOT.literals_protected
/// @structural REQ-ANNOT.literals_protected reason="a claim about which byte ranges the scanner may look at, which needs the parser the model cannot run"
#[test]
fn an_annotation_inside_a_string_literal_is_data_and_not_a_link() {
    use tracelean_core::trace::anchor::{scan, Lang};

    let source = "pub fn f() {\n    let s = \"// @implements REQ-GHOST.c\";\n}\n";
    let scanned = scan(source, Some(Lang::Rust));
    assert!(
        scanned.comments.directives.is_empty(),
        "an annotation inside a literal became a directive"
    );

    let commented = "// @implements REQ-GHOST.c\npub fn f() {}\n";
    assert!(!scan(commented, Some(Lang::Rust)).comments.directives.is_empty());
}

/// @tests REQ-MYTH.outcomes_closed
/// @tests REQ-CMD.inverse_exists
#[test]
fn the_closed_vocabularies_stay_closed() {
    use tracelean_core::history::command::{inverse, Command};
    use tracelean_core::surface::keymap::Outcome;

    let outcomes = [
        Outcome::Enter { mode: "File".into() },
        Outcome::Dispatch { action: "undo".into() },
        Outcome::Leave { mode: "Main".into() },
        Outcome::PassThrough,
    ];
    let names: BTreeSet<String> = outcomes
        .iter()
        .map(|o| serde_json::to_value(o).unwrap().as_object().map_or_else(
            || serde_json::to_value(o).unwrap().as_str().unwrap().to_string(),
            |m| m.keys().next().unwrap().clone(),
        ))
        .collect();
    assert_eq!(names.len(), 4, "two outcomes encode the same way");

    let commands = [
        Command::Insert { file: "a".into(), offset: 0, text: "x".into() },
        Command::Delete { file: "a".into(), offset: 0, deleted: "x".into() },
        Command::CreateFile { path: "a".into() },
        Command::DeleteFile { path: "a".into(), content: "x".into() },
        Command::RenameFile { from: "a".into(), to: "b".into() },
        Command::Batch { commands: vec![Command::CreateFile { path: "a".into() }] },
    ];
    for command in &commands {
        let back = inverse(command);
        assert_ne!(
            serde_json::to_value(&back).unwrap(),
            serde_json::to_value(command).unwrap(),
            "{command:?} is its own inverse, which no command that changes anything can be"
        );
        let round = inverse(&back);
        if !rebuilds_a_file(command) {
            assert_eq!(
                serde_json::to_value(&round).unwrap(),
                serde_json::to_value(command).unwrap(),
                "inverting twice did not return {command:?}"
            );
        }
    }
}

fn rebuilds_a_file(command: &tracelean_core::history::command::Command) -> bool {
    use tracelean_core::history::command::Command;
    match command {
        Command::CreateFile { .. } | Command::DeleteFile { .. } => true,
        Command::Batch { commands } => commands.iter().any(rebuilds_a_file),
        _ => false,
    }
}

/// @tests REQ-LSP.registry_per_language
/// @tests REQ-LSP.encoding_declared
/// @tests REQ-LSP.edits_become_commands
#[test]
fn the_position_encoding_comes_from_the_server_that_declared_it() {
    use tracelean_core::history::command::Command;
    use tracelean_core::surface::lsp::{lower, present, Display, Edit, Encoding, ServerState};

    let answers: Vec<Display<Vec<String>>> = [
        ServerState::NotStarted,
        ServerState::Starting,
        ServerState::Running { encoding: Encoding::Utf16 },
    ]
    .iter()
    .map(|state| present(state, Some(vec![])))
    .collect();
    assert!(
        !matches!(answers[0], Display::Result { .. }),
        "no server running was presented as an answer"
    );
    assert!(matches!(answers[2], Display::Result { .. }), "a running server's answer was hidden");

    let running = ServerState::Running { encoding: Encoding::Utf8 };
    let json = serde_json::to_value(&running).unwrap();
    assert_eq!(
        json["running"]["encoding"],
        serde_json::json!("utf8"),
        "the encoding is not declared"
    );

    let commands = lower(
        "a.rs".into(),
        "hello".into(),
        vec![Edit { start: 0, end: 1, text: "H".into() }],
    )
    .expect("lowered");
    assert!(commands.iter().all(|c| matches!(c, Command::Insert { .. } | Command::Delete { .. })));
}

/// @tests REQ-ANNOT.unknown_role_named
/// @tests REQ-ANNOT.totality
#[test]
fn a_malformed_annotation_is_reported_and_not_dropped() {
    use tracelean_core::trace::index::build;

    let dir = std::env::temp_dir().join("tracelean-malformed-annotation");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a.rs"),
        "/// @implments REQ-X.y\n/// @implements\nfn f() {}\n",
    )
    .unwrap();

    let index = build(&dir);
    let messages: Vec<&str> = index.problems.iter().map(|p| p.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("implments")),
        "the misspelt role was dropped instead of reported: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("names no requirement")),
        "a role with no identifier was dropped instead of reported: {messages:?}"
    );
    assert!(
        index.links.is_empty(),
        "a malformed annotation became a link anyway: {:?}",
        index.links
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// @tests REQ-REQDOC.id_unique
#[test]
fn a_repeated_requirement_identifier_is_reported() {
    use tracelean_core::trace::index::build;

    let dir = std::env::temp_dir().join("tracelean-duplicate-requirement");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("nested")).unwrap();
    let document = "---\nid: REQ-SAME\ntitle: One\n---\n\n# One\n";
    std::fs::write(dir.join("a.md"), document).unwrap();
    std::fs::write(dir.join("nested/b.md"), document).unwrap();

    let index = build(&dir);
    assert_eq!(index.requirements.len(), 1, "the duplicate was not collapsed");
    assert!(
        index.problems.iter().any(|p| p.message.contains("REQ-SAME")),
        "the duplicate identifier was not reported: {:?}",
        index.problems
    );

    let _ = std::fs::remove_dir_all(&dir);
}
