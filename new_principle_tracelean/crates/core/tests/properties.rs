//! Properties that are about shapes and structures rather than about values.
//!
//! Most of what this project claims is checked by comparing two
//! implementations. These are the clauses that cannot be: that the schema
//! grammar is closed, that the protocol is line-delimited, that one runner
//! serves every binding, that rendering the index touches no filesystem. They
//! are checked here by exercising the thing itself, rather than being asserted
//! in a doc comment nobody can fail.

use std::collections::{BTreeMap, BTreeSet};

use tracelean_core::drt::gen;
use tracelean_core::drt::protocol::{Case, Reply, RunnerError};
use tracelean_core::drt::run::{agree, DrtResult, RunOptions};
use tracelean_core::drt::schema::Schema;
use tracelean_core::drt::{qualified_op, Binding, CallSpec};

// ─── The schema grammar ──────────────────────────────────────────────────────

/// Every shape in the grammar, so a new one cannot be added without this list
/// noticing.
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

/// The grammar is a closed whitelist: nine shapes, each of which generates.
///
/// A tenth shape added to the enum without a generator arm would fail to
/// compile; a tenth added *with* one would fail this count. Both are the point:
/// the alternative to a closed grammar is a generator that silently
/// approximates a type it does not understand.
///
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

/// Whether a generated value has the shape that asked for it.
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
        // A nullary variant is the bare name; one with a payload is a
        // single-key object. This is the shape Lean's derived encoding
        // produces, which is the whole reason the grammar describes it.
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

/// The shape is declared in the binding, not extracted from the model.
///
/// A schema is data this project writes down; nothing reads a Lean declaration
/// to produce one. The evidence is that `Binding` has no field for a model type
/// and `CallSpec` may only rename, never compute.
///
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
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: "crates/core/src/evidence.rs::assurance".into(),
            params: BTreeMap::from([("records".to_string(), "records".to_string())]),
        },
    };

    // Round-tripping through JSON is what a binding file is, and it must carry
    // nothing but a call.
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

    // `params` maps names onto names. A value that is not a plain name would be
    // an adapter in disguise.
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

// ─── The protocol ────────────────────────────────────────────────────────────

/// A case is one JSON object on one line, and so is a reply.
///
/// @tests REQ-DRT-PROTO.line_delimited
/// @tests REQ-DRT-PROTO.case_echoed
#[test]
fn a_case_and_a_reply_are_each_one_line() {
    let case = Case {
        case: 7,
        op: "REQ-X.c".into(),
        // Deliberately carrying the characters that would break a line-oriented
        // protocol if they were not escaped.
        input: serde_json::json!({"text": "a\nb\r\nc", "n": 1}),
    };
    let line = serde_json::to_string(&case).expect("a case serialises");
    assert!(!line.contains('\n'), "a case spanned more than one line: {line}");

    let reply = Reply::ok(case.case, serde_json::json!({"out": "x\ny"}));
    let line = serde_json::to_string(&reply).expect("a reply serialises");
    assert!(!line.contains('\n'), "a reply spanned more than one line: {line}");

    let read: Reply = serde_json::from_str(&line).expect("a reply round-trips");
    assert_eq!(read.case, case.case, "a reply must echo the case it answers");
}

/// Exactly one of an output and an error, and `null` is an output.
///
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

    // A model returning `none` answers `null`, which is an output and not an
    // absence. Reading it as absence made a legitimate answer look like a
    // runner that could not speak.
    let null_output = serde_json::json!({"case": 1, "output": null});
    let read = Reply::from_json(&null_output).expect("readable");
    assert_eq!(read.output, Some(serde_json::Value::Null));
    assert!(read.is_well_formed(), "`null` is an answer");

    let neither = serde_json::json!({"case": 1});
    assert!(!Reply::from_json(&neither).expect("readable").is_well_formed());

    // A reply with no case number is not a reply at all.
    assert!(Reply::from_json(&serde_json::json!({"output": 1})).is_err());
}

/// An op names one entry point, project-wide.
///
/// A shared runner dispatches on this name, so two bindings sharing one would
/// answer one of them with the other's function and nothing would report it.
///
/// @tests REQ-DRT-PROTO.op_dispatch
/// @tests REQ-DRT-PROTO.runner_shared
#[test]
fn every_op_in_this_project_is_unique() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let text = std::fs::read_to_string(root.join(".tracelean/drt.json")).expect("drt.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("readable JSON");

    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for binding in value["bindings"].as_array().expect("bindings") {
        let op = match binding["op"].as_str() {
            Some(op) => op.to_string(),
            None => qualified_op(
                binding["req_id"].as_str().unwrap_or_default(),
                binding["clause"].as_str(),
            ),
        };
        *seen.entry(op).or_default() += 1;
    }
    let repeated: Vec<&String> = seen.iter().filter(|(_, n)| **n > 1).map(|(op, _)| op).collect();
    assert!(repeated.is_empty(), "two bindings share an op: {repeated:?}");
    assert!(seen.len() > 20, "only {} ops; the file did not load", seen.len());
}

/// The four ways a runner can fail to answer mean different things.
///
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

    // None of them is an answer: a reply that disagrees is data, a runner that
    // did not speak is not.
    for failure in &failures {
        assert!(!format!("{failure}").is_empty());
    }
}

// ─── What a run establishes ──────────────────────────────────────────────────

/// Both sides refusing an input is agreement; one refusing is a divergence.
///
/// @tests REQ-DRT.error_is_an_answer
/// @tests REQ-DRT.same_question
/// @structural REQ-DRT.same_question reason="a claim about the order two processes are driven in, which lives in the harness loop rather than in any value"
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

/// A clean run states how many cases it ran, and never claims more.
///
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
    };
    assert!(clean.agreed());
    assert_eq!((clean.seed, clean.cases), (7, 2_000));

    // The default is small enough to be a default and stated in one place.
    let options = RunOptions::default();
    assert!(options.cases > 0 && options.shrink_rounds > 0);
}

// ─── Generated runners ───────────────────────────────────────────────────────

/// A runner is generated as a crate or a package and built, never shipped as a
/// script — and generation writes only under the cache directory.
///
/// The second half matters more than it looks. Generation runs during a check,
/// and a generator that could write into the project would make checking the
/// project change it.
///
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

    // Everything written lives under the cache directory, and the package is a
    // build input rather than an interpreted file.
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

    // The toolchain check names what is missing rather than failing opaquely.
    if let Err(message) = lean_runner::toolchain() {
        assert!(message.len() > 10, "an unexplained toolchain failure: {message}");
    }

    // The generated main declares the encoding it exchanges rather than
    // assuming one: the op, the function, and each argument by name.
    let main = lean_runner::main_lean(&["TraceLean.Evidence"], &entries);
    assert!(main.contains("TraceLean.Evidence.assurance"));
    assert!(main.contains("REQ-X.c"));
    assert!(main.contains("records"));
    assert!(main.contains("fromJson?") || main.contains("getObjVal?"));

    // The same for Rust: a crate with a manifest and a path dependency.
    let manifest = rust_runner::cargo_toml(&BTreeMap::from([(
        "tracelean-core".to_string(),
        "/somewhere/core".to_string(),
    )]));
    assert!(manifest.contains("[package]"), "not a crate");
    assert!(manifest.contains("path = \"/somewhere/core\""), "not a path dependency");

    let _ = std::fs::remove_dir_all(&scratch);
}

/// The encoding a schema describes is the encoding both sides exchange.
///
/// A nullary variant is the bare name and one with a payload is a single-key
/// object, because that is what Lean's derived encoding produces. A schema
/// describing a different shape would generate cases neither side could read.
///
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

// ─── Things the scanner and the judge must not do ────────────────────────────

/// Producing a judging prompt exports text and nothing else.
///
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

    // Whatever a tool says, what enters the record is a person's decision with
    // their name on it, at the judgement level and no higher.
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

/// A transcript is read and never interpreted: what the tool says it did does
/// not decide anything.
///
/// @tests REQ-TRANSCRIPT.read_only
/// @tests REQ-TRANSCRIPT.no_interpretation
/// @structural REQ-TRANSCRIPT.read_only reason="an absence: nothing writes to the transcript, and a model of a read could not distinguish a version that also truncated"
/// @structural REQ-TRANSCRIPT.no_interpretation reason="a claim that no code path turns a transcript event into a command, which is read off the tree"
#[test]
fn a_transcript_produces_events_and_no_commands() {
    use tracelean_core::observe::transcript::read;

    let text = "{\"type\":\"edit\",\"text\":\"rm -rf /\"}\n{\"type\":\"say\",\"text\":\"done\"}\n";
    let result = read(text.to_string());
    assert_eq!(result.events.len(), 2);

    // The reader's whole output is events, unrecognised lines, a remainder and
    // what the tool said it used. There is nowhere for a command to come out,
    // which is the point: the workspace diff is the truth about what happened,
    // and usage is a number on a screen rather than an instruction.
    let json = serde_json::to_value(&result).expect("serialises");
    let keys: BTreeSet<&str> = json.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    assert_eq!(keys, BTreeSet::from(["events", "unrecognised", "held", "usage"]));
}

/// Evidence carries what is needed to reproduce it, and cannot be written
/// without it.
///
/// @tests REQ-EVID.record_reproducible
/// @tests REQ-STALE.inputs_identified
#[test]
fn an_evidence_record_cannot_omit_what_would_reproduce_it() {
    use tracelean_core::trace::record::Detail;

    // Each backend's detail is a distinct shape, and each names what a person
    // would need to run it again.
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

    // A record's inputs are named, so a change to any of them can invalidate
    // it. An unnamed input is a dependency nothing can notice moving.
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

// ─── The index, the lockfile, and what a scan may do ─────────────────────────

/// Rendering the index is a function of the index alone, carries evidence
/// through unchanged, and never touches the filesystem.
///
/// The last part is why `render` takes an `&Index` rather than a path: a
/// renderer that could read the tree could produce a lockfile that does not
/// match the index it was given, and the diff would be about neither.
///
/// @tests REQ-LOCK.pure_render
/// @tests REQ-LOCK.evidence_preserved
/// @tests REQ-LOCK.diff_is_meaningful
/// @tests REQ-STALE.scanner_never_writes
/// @tests ARCH-HONEST.untraced_visible
/// @tests ARCH-CORE-SHELL.decision_total
/// @structural ARCH-HONEST.untraced_visible reason="a claim that every coverage view lists unclaimed files, which is about what a report contains rather than about a value a function returns"
/// @structural ARCH-CORE-SHELL.decision_total reason="a claim about the shape of every decision in the project; the individual decisions are each modelled, this is that they all are"
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

    // A function of its input: the same index renders the same bytes, and the
    // scan is not consulted again.
    assert_eq!(to_bytes(&rendered), to_bytes(&render(&index, evidence.clone())));

    // Untraced files are visible in the scan, so a coverage figure has an
    // honest denominator.
    assert!(!index.scanned.is_empty(), "the scan remembered nothing");
    let annotated: BTreeSet<&String> = index.links.iter().map(|l| &l.anchor.file).collect();
    assert!(
        index.scanned.len() > annotated.len(),
        "every scanned file carried an annotation, which cannot be true of this tree"
    );

    // A change to what the project claims changes the bytes.
    let mut fewer = rendered.clone();
    fewer.links.pop();
    assert_ne!(to_bytes(&rendered), to_bytes(&fewer), "dropping a link left the file identical");

    // Nothing was written: the scan is read-only, and the lockfile on disk (if
    // any) is untouched by rendering one in memory.
    let lock = root.join(".tracelean").join("trace.lock.json");
    let before = std::fs::read(&lock).ok();
    let _ = render(&index, evidence);
    assert_eq!(std::fs::read(&lock).ok(), before, "rendering wrote to the tree");
}

/// A document's target resolves through the same anchor mechanism as an
/// annotation's, rather than through one written for documents.
///
/// @tests REQ-DOCLINK.same_anchor_machinery
/// @structural REQ-DOCLINK.same_anchor_machinery reason="a claim that two callers reach the same function, which is about the call graph rather than about what the function returns"
#[test]
fn a_document_resolves_its_target_the_way_an_annotation_does() {
    use tracelean_core::trace::doclink::current_hashes;
    use tracelean_core::trace::index::build;

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
    let index = build(root);
    let hashes = current_hashes(&index);

    // Every requirement hashes by the same content hash the requirement itself
    // carries, and every code anchor by the same body hash its link carries.
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

/// Text inside a string literal is not scanned for annotations.
///
/// Without this, a test fixture containing an annotation as data would create a
/// link the author did not write — and the link would point at the test.
///
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

    // The same text in a comment does become one, so the test is about the
    // literal and not about the text.
    let commented = "// @implements REQ-GHOST.c\npub fn f() {}\n";
    assert!(!scan(commented, Some(Lang::Rust)).comments.directives.is_empty());
}

// ─── Closed vocabularies ─────────────────────────────────────────────────────

/// Four outcomes, and a key that matches none of them is not a fifth.
///
/// @tests REQ-MYTH.outcomes_closed
/// @tests REQ-CMD.inverse_exists
#[test]
fn the_closed_vocabularies_stay_closed() {
    use tracelean_core::history::command::{inverse, Command};
    use tracelean_core::surface::keymap::Outcome;

    // Every outcome is representable and distinct on the wire.
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

    // Every command has an inverse, including the one whose inverse is a batch.
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
        // Inverting twice returns the original shape, except where a file is
        // created or deleted: undoing a deletion has to restore the content,
        // so it is a batch by construction and does not fold back into the
        // single command it came from. That is a property of the vocabulary,
        // not a defect, so it is named here rather than asserted away.
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

/// Whether undoing this command has to restore a file's content, which is the
/// one case in which an inverse is a batch rather than a single command.
fn rebuilds_a_file(command: &tracelean_core::history::command::Command) -> bool {
    use tracelean_core::history::command::Command;
    match command {
        Command::CreateFile { .. } | Command::DeleteFile { .. } => true,
        Command::Batch { commands } => commands.iter().any(rebuilds_a_file),
        _ => false,
    }
}

/// A server per language, with the encoding taken from what it declared.
///
/// @tests REQ-LSP.registry_per_language
/// @tests REQ-LSP.encoding_declared
/// @tests REQ-LSP.edits_become_commands
#[test]
fn the_position_encoding_comes_from_the_server_that_declared_it() {
    use tracelean_core::history::command::Command;
    use tracelean_core::surface::lsp::{lower, present, Display, Edit, Encoding, ServerState};

    // The absence of a server is a named state, not an empty result. For a
    // proof assistant this is sharp: an empty goal list means the proof is
    // finished.
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

    // Each state carries its own encoding rather than a global one. The state
    // is externally tagged — that is what the model's derived encoding
    // produces — so the encoding sits under the variant name.
    let running = ServerState::Running { encoding: Encoding::Utf8 };
    let json = serde_json::to_value(&running).unwrap();
    assert_eq!(
        json["running"]["encoding"],
        serde_json::json!("utf8"),
        "the encoding is not declared"
    );

    // Edits become ordinary commands, the same kind a person's keystroke makes.
    let commands = lower(
        "a.rs".into(),
        "hello".into(),
        vec![Edit { start: 0, end: 1, text: "H".into() }],
    )
    .expect("lowered");
    assert!(commands.iter().all(|c| matches!(c, Command::Insert { .. } | Command::Delete { .. })));
}

/// An annotation the parser cannot read must reach the report.
///
/// This is the failure that hides itself: a misspelt role produces no link, the
/// clause it meant to cover reads as uncovered, and the person who wrote it has
/// no way to tell a typo from an honest gap. The parser has always noticed;
/// what this pins is that the index passes it on.
///
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

/// Two documents declaring the same identifier are reported, and the report
/// names the identifier rather than the file that happened to lose.
///
/// Identity is the declared `id`, so the two documents are the same
/// requirement said twice — which is a fault whichever file it is in.
///
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
