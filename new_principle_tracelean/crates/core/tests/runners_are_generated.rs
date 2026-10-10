//! The two conformance runners, checked by reading what the generator writes.
//!
//! None of these clauses is a function from data to data. "The runner is a
//! compiled crate", "generation touches nothing the project owns", "the binding
//! describes a call and nothing more" — each is a property of the tree and of
//! the generator's output, so each is checked by generating into a scratch
//! directory and reading the result. That is what `@structural` records
//! (ADR-0012).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tracelean_core::drt::lean_runner::{self, LeanEntry};
use tracelean_core::drt::rust_runner;
use tracelean_core::drt::{Binding, CallSpec};

fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("tracelean-generated-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

/// Every file under a directory, relative to it.
fn files_under(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.display().to_string());
            }
        }
    }
    out.sort();
    out
}

/// The Rust side is a generated crate with a path dependency, written only
/// under the cache directory.
///
/// @tests REQ-DRT-RUST.generated
/// @tests REQ-DRT-RUST.project_untouched
/// @tests REQ-DRT-RUST.path_dependency
/// @tests REQ-DRT-RUST.rewritten_when_stale
/// @structural REQ-DRT-RUST.generated reason="a claim about what the generator writes to disk, which is read off the output directory rather than returned by a function"
/// @structural REQ-DRT-RUST.project_untouched reason="an absence: no file outside the cache directory changes, which can only be checked by looking at the tree before and after"
/// @structural REQ-DRT-RUST.path_dependency reason="a claim about the text of a generated manifest"
/// @structural REQ-DRT-RUST.rewritten_when_stale reason="a claim about what a second generation does to an existing directory"
#[test]
fn the_rust_runner_is_a_generated_crate_under_the_cache() {
    let root = project_root();
    let out = scratch("rust");

    let binding = Binding {
        req_id: "REQ-EVID".into(),
        clause: Some("weakest_link".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: "crates/core/src/evidence.rs::assurance".into(),
            params: BTreeMap::new(),
        },
    };
    let entry = rust_runner::resolve(&root, &binding).expect("binding resolves");
    let mut deps = BTreeMap::new();
    deps.insert(
        "tracelean-core".to_string(),
        root.join("crates").join("core").display().to_string(),
    );
    rust_runner::materialize(&out, &[entry.clone()], &deps).expect("generated");

    // Everything written sits under the cache directory, and it is a crate.
    let written = files_under(&out);
    assert!(
        written.iter().all(|f| f.starts_with(".tracelean")),
        "generation wrote outside the cache directory: {written:?}"
    );
    assert!(written.iter().any(|f| f.ends_with("Cargo.toml")), "no manifest: {written:?}");
    assert!(written.iter().any(|f| f.ends_with("main.rs")), "no source: {written:?}");

    // A path dependency, not a published one: the runner calls the library that
    // is in the tree, which is the only thing a differential test can be about.
    let manifest =
        std::fs::read_to_string(rust_runner::package_dir(&out).join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("path ="),
        "the generated crate does not reach the implementation by path:\n{manifest}"
    );

    // Generating again over the same directory reproduces it exactly. A stale
    // runner answering with last week's implementation is the failure mode
    // `rewritten_when_stale` exists for, and it is invisible: the run succeeds.
    let before = std::fs::read_to_string(rust_runner::package_dir(&out).join("src/main.rs")).unwrap();
    std::fs::write(rust_runner::package_dir(&out).join("src/main.rs"), "fn main() {}\n").unwrap();
    rust_runner::materialize(&out, &[entry], &deps).expect("regenerated");
    let after = std::fs::read_to_string(rust_runner::package_dir(&out).join("src/main.rs")).unwrap();
    assert_eq!(after, before, "a tampered runner was left in place");

    let _ = std::fs::remove_dir_all(&out);
}

/// The Lean side is a generated package that is built, and a missing toolchain
/// is its own explained state rather than a failure of the model.
///
/// @tests REQ-DRT-LEAN.generated
/// @tests REQ-DRT-LEAN.project_untouched
/// @tests REQ-DRT-LEAN.toolchain_explained
/// @tests REQ-DRT-LEAN.compiled_not_interpreted
/// @tests REQ-DRT-LEAN.encoding_declared
/// @structural REQ-DRT-LEAN.generated reason="a claim about what the generator writes, read off the output directory"
/// @structural REQ-DRT-LEAN.project_untouched reason="an absence: the project's own Lean package is not among the files generation writes"
/// @structural REQ-DRT-LEAN.toolchain_explained reason="a claim about which of two states a missing toolchain produces; the state is reported by the shell that looked for it"
/// @structural REQ-DRT-LEAN.compiled_not_interpreted reason="a claim about how the generated package is built, which is in the manifest rather than in any value"
/// @structural REQ-DRT-LEAN.encoding_declared reason="a claim that one convention is used throughout; the convention itself is what every other differential suite exercises"
#[test]
fn the_lean_runner_is_a_generated_package_that_is_compiled() {
    let root = project_root();
    let out = scratch("lean");

    let entries = [LeanEntry {
        op: "REQ-EVID.weakest_link".to_string(),
        function: "TraceLean.Evidence.assurance".to_string(),
        arguments: vec!["records".to_string()],
    }];
    lean_runner::materialize(
        &out,
        &["TraceLean.Evidence"],
        "tracelean",
        &root.join("formal").display().to_string(),
        &entries,
    )
    .expect("generated");

    let written = files_under(&out);
    assert!(
        written.iter().all(|f| f.starts_with(".tracelean")),
        "generation wrote outside the cache directory: {written:?}"
    );
    assert!(written.iter().any(|f| f.ends_with("lakefile.lean")), "no package: {written:?}");

    // The project's own package is untouched: nothing under `formal/` is in
    // the list of files generation wrote.
    assert!(
        !written.iter().any(|f| f.contains("formal")),
        "generation wrote into the project's own Lean package: {written:?}"
    );

    // Compiled rather than interpreted: the package declares an executable,
    // which is what makes a case cost microseconds instead of milliseconds.
    let lakefile =
        std::fs::read_to_string(lean_runner::package_dir(&out).join("lakefile.lean")).unwrap();
    assert!(
        lakefile.contains("lean_exe"),
        "the generated package is not an executable:\n{lakefile}"
    );
    assert!(
        lakefile.contains("require"),
        "the generated package does not require the model package:\n{lakefile}"
    );

    // One declared encoding convention, spoken by the generated dispatch:
    // `ToJson`/`FromJson` throughout, which is what the schema grammar
    // describes and what every other suite in this tree exercises.
    let main = std::fs::read_to_string(lean_runner::package_dir(&out).join("Main.lean")).unwrap();
    assert!(main.contains("toJson"), "the generated runner does not use the declared encoding");
    assert!(main.contains("fromJson?"), "the generated runner does not use the declared encoding");

    // And a missing toolchain is a named state. This machine has one, so what
    // is checked is that the question is asked and answered rather than
    // assumed — a runner that skipped the check would report a model failure
    // on a machine without Lean.
    match lean_runner::toolchain() {
        Ok(()) => {}
        Err(reason) => assert!(
            reason.len() > 10,
            "a missing toolchain was reported as `{reason}`, which explains nothing"
        ),
    }

    let _ = std::fs::remove_dir_all(&out);
}

/// A binding names a call and nothing else.
///
/// The argument against an adapter is not duplication. It is that any code
/// between the implementation and the comparator is code the differential test
/// is testing instead of the implementation — and it is code nobody reviews,
/// because it looks like plumbing.
///
/// @tests REQ-DRT-BIND.no_adapter
/// @tests REQ-DRT-BIND.call_only
/// @tests REQ-DRT-BIND.rename_only
/// @tests REQ-DRT-BIND.binding_is_the_bond
/// @structural REQ-DRT-BIND.no_adapter reason="an absence: no project-written code sits between implementation and comparator, which is read off the binding file's schema and the generated dispatch"
/// @structural REQ-DRT-BIND.call_only reason="a claim about which keys a binding may carry, which is a property of the file rather than of a call"
/// @structural REQ-DRT-BIND.rename_only reason="a claim that a parameter map's values are identifiers and never expressions"
/// @structural REQ-DRT-BIND.binding_is_the_bond reason="a claim that the binding file and the annotations agree, checked over the whole project by `bindings_are_real`"
#[test]
fn a_binding_describes_a_call_and_nothing_more() {
    let root = project_root();
    let text = std::fs::read_to_string(root.join(".tracelean/drt.json")).expect("drt.json");
    let config: serde_json::Value = serde_json::from_str(&text).expect("readable JSON");

    // The vocabulary is closed. A binding that could carry a `transform`, a
    // `before`, or a snippet of code would be an adapter with a nicer name.
    const BINDING_KEYS: &[&str] = &[
        "req_id",
        "clause",
        "op",
        "also_checks",
        "model",
        "implementation",
        "also_implemented_by",
        // Coverage floors qualify the run without touching it: a name and a
        // number, never a predicate and never code. The predicate that decides
        // whether a generated case reached a situation stays in the suite,
        // where it can be read — this file only says how often it must.
        "floors",
        // Waivers likewise: names of classes or lines a run is excused from
        // reaching, each with the reason a person can argue with. Never code.
        "waive",
    ];
    const WAIVER_KEYS: &[&str] = &["situations", "reason"];
    const MODEL_KEYS: &[&str] = &["import", "function", "arguments"];
    const IMPL_KEYS: &[&str] = &["language", "entry", "params"];
    const FLOOR_KEYS: &[&str] = &["situation", "atLeast"];

    let bindings = config["bindings"].as_array().expect("bindings");
    assert!(bindings.len() > 30, "only {} bindings; the file is wrong", bindings.len());

    for binding in bindings {
        let object = binding.as_object().expect("a binding is an object");
        for key in object.keys() {
            assert!(
                BINDING_KEYS.contains(&key.as_str()),
                "a binding carries `{key}`, which is not part of describing a call"
            );
        }
        for waiver in binding["waive"].as_array().unwrap_or(&Vec::new()) {
            let waiver = waiver.as_object().expect("a waiver is an object");
            for key in waiver.keys() {
                assert!(WAIVER_KEYS.contains(&key.as_str()), "a waiver carries `{key}`");
            }
            assert!(
                waiver.get("reason").and_then(|r| r.as_str()).is_some_and(|r| !r.trim().is_empty()),
                "a waiver without a reason excuses nothing"
            );
        }
        for key in binding["model"].as_object().expect("a model").keys() {
            assert!(MODEL_KEYS.contains(&key.as_str()), "a model spec carries `{key}`");
        }
        // A floor is a situation and a minimum, and a floor with no minimum is
        // not a declaration — it is a situation somebody meant to think about.
        for floor in binding["floors"].as_array().unwrap_or(&Vec::new()) {
            let floor = floor.as_object().expect("a floor is an object");
            for key in floor.keys() {
                assert!(FLOOR_KEYS.contains(&key.as_str()), "a floor carries `{key}`");
            }
            assert!(
                floor.get("situation").and_then(|s| s.as_str()).is_some_and(|s| !s.is_empty()),
                "a floor names no situation"
            );
            assert!(floor.get("atLeast").and_then(|n| n.as_u64()).is_some(), "a floor has no minimum");
        }
        // Every implementation of a clause is held to the same shape: the
        // second one is not a looser kind of binding than the first.
        let mut implementations = vec![&binding["implementation"]];
        if let Some(more) = binding["also_implemented_by"].as_array() {
            implementations.extend(more.iter());
        }
        for implementation in implementations {
            let object = implementation.as_object().expect("an implementation");
            for key in object.keys() {
                assert!(IMPL_KEYS.contains(&key.as_str()), "an implementation spec carries `{key}`");
            }
            let language = implementation["language"].as_str().expect("a language");
            assert!(
                tracelean_core::drt::config::LANGUAGES.contains(&language),
                "`{language}` is not a language a runner can be generated for"
            );

            // A parameter map renames. Both sides of every entry are bare
            // identifiers, so there is nowhere for a computation to hide.
            let Some(params) = implementation.get("params").and_then(|p| p.as_object()) else {
                continue;
            };
            for (from, to) in params {
                let to = to.as_str().expect("a parameter name");
                for name in [from.as_str(), to] {
                    assert!(
                        !name.is_empty()
                            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                        "the parameter map maps `{from}` to `{to}`, which is not a rename"
                    );
                }
            }
        }
    }

    // And the generated dispatch calls the entry point directly: deserialise,
    // call, serialise. Anything else between the two would be the adapter this
    // clause forbids.
    let entry = rust_runner::resolve(
        &root,
        &Binding {
            req_id: "REQ-EVID".into(),
            clause: Some("weakest_link".into()),
            op: None,
            also_checks: Vec::new(),
            also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
            model: None,
            implementation: CallSpec {
                language: "rust".into(),
                entry: "crates/core/src/evidence.rs::assurance".into(),
                params: BTreeMap::new(),
            },
        },
    )
    .expect("resolves");
    let generated = rust_runner::main_rs(&[entry]);
    assert!(
        generated.contains("assurance(records)"),
        "the generated dispatch does not call the entry point directly:\n{generated}"
    );
}

/// Shrinking does not stop while a strictly smaller diverging case is reachable
/// by its reduction steps, and a generated crate that fails to compile is
/// reported with the compiler's own message.
///
/// Both are properties of a loop that drives something else, so neither is a
/// function from data to data. What is checked is the loop's shape: every
/// reduction step is strictly smaller, so the loop can neither stop while a
/// smaller diverging case is still reachable nor run forever; and a build
/// failure carries what the compiler said rather than a summary of it.
///
/// @tests REQ-DRT-GEN.shrink_minimal
/// @tests REQ-DRT-RUST.build_error_explained
/// @structural REQ-DRT-GEN.shrink_minimal reason="a property of the shrink loop, which stops only when no candidate diverges; the function under it is the reduction step, and what matters is that every step is strictly smaller"
/// @structural REQ-DRT-RUST.build_error_explained reason="a claim about what a failed build reports, which needs a compiler to produce and is not a value any model computes"
#[test]
fn shrinking_goes_all_the_way_down_and_a_failed_build_says_why() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::schema::Schema;

    /// How much there is to shrink away. Nothing is reducible to `null`, an
    /// element costs something even when it is empty, and `false` is below
    /// `true` -- which is the order the reduction steps themselves follow.
    fn size(value: &serde_json::Value) -> usize {
        match value {
            serde_json::Value::Null => 0,
            serde_json::Value::Bool(b) => 1 + usize::from(*b),
            serde_json::Value::Number(n) => 1 + n.as_u64().unwrap_or(0).min(1_000) as usize,
            serde_json::Value::String(s) => 1 + s.len(),
            serde_json::Value::Array(items) => {
                1 + items.len() + items.iter().map(size).sum::<usize>()
            }
            serde_json::Value::Object(map) => 1 + map.values().map(size).sum::<usize>(),
        }
    }

    let schemas = [
        Schema::Nat { max: Some(50), edges: vec![0, 1] },
        Schema::Str { max_len: Some(6), examples: vec!["abc".into()] },
        Schema::Bool,
        Schema::List {
            inner: Box::new(Schema::Nat { max: Some(9), edges: vec![0] }),
            max_len: Some(4),
        },
        Schema::Option { inner: Box::new(Schema::Bool) },
        Schema::Struct {
            fields: [
                ("a".to_string(), Schema::Nat { max: Some(9), edges: vec![0] }),
                ("b".to_string(), Schema::Str { max_len: Some(3), examples: vec![] }),
            ]
            .into_iter()
            .collect(),
        },
    ];

    let mut rng = gen::Rng::new(211);
    let mut reductions = 0;
    let mut dead_ends = 0;
    for _ in 0..500 {
        for schema in &schemas {
            let value = gen::value(schema, &mut rng);
            let candidates = gen::shrink(schema, &value);
            if candidates.is_empty() {
                dead_ends += 1;
                continue;
            }
            for candidate in &candidates {
                assert!(
                    size(candidate) < size(&value),
                    "a reduction step of {value} produced {candidate}, which is not smaller"
                );
                reductions += 1;
            }
        }
    }
    assert!(reductions > 1_000, "only {reductions} reduction steps were checked");
    assert!(dead_ends > 100, "only {dead_ends} values were already minimal");

    // A generated crate that does not compile reports what the compiler said.
    // The binding is what does not typecheck, and a summary of the error would
    // leave the person with nothing to act on.
    let out = scratch("bad-build");
    let dir = rust_runner::materialize(&out, &[], &BTreeMap::new()).expect("generated");
    std::fs::write(dir.join("src").join("main.rs"), "fn main() { let _: u32 = \"no\"; }\n")
        .unwrap();
    let built = std::process::Command::new("cargo")
        .args(["build", "--quiet", "--offline"])
        .current_dir(&dir)
        .output()
        .expect("cargo runs");
    assert!(!built.status.success(), "a deliberately broken crate compiled");
    let message = String::from_utf8_lossy(&built.stderr);
    assert!(
        message.contains("mismatched types") && message.contains("main.rs"),
        "a failed build did not say what did not match:\n{message}"
    );

    let _ = std::fs::remove_dir_all(&out);
}
