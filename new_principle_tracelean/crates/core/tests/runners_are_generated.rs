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
        clause: Some("mark_per_bond".into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "rust".into(),
            entry: "crates/core/src/evidence.rs::marks".into(),
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

    let written = files_under(&out);
    assert!(
        written.iter().all(|f| f.starts_with(".tracelean")),
        "generation wrote outside the cache directory: {written:?}"
    );
    assert!(written.iter().any(|f| f.ends_with("Cargo.toml")), "no manifest: {written:?}");
    assert!(written.iter().any(|f| f.ends_with("main.rs")), "no source: {written:?}");

    let manifest =
        std::fs::read_to_string(rust_runner::package_dir(&out).join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("path ="),
        "the generated crate does not reach the implementation by path:\n{manifest}"
    );

    let before = std::fs::read_to_string(rust_runner::package_dir(&out).join("src/main.rs")).unwrap();
    std::fs::write(rust_runner::package_dir(&out).join("src/main.rs"), "fn main() {}\n").unwrap();
    rust_runner::materialize(&out, &[entry], &deps).expect("regenerated");
    let after = std::fs::read_to_string(rust_runner::package_dir(&out).join("src/main.rs")).unwrap();
    assert_eq!(after, before, "a tampered runner was left in place");

    let _ = std::fs::remove_dir_all(&out);
}

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
        op: "REQ-EVID.mark_per_bond".to_string(),
        function: "TraceLean.Evidence.marks".to_string(),
        arguments: vec!["records".to_string(), "annotated".to_string()],
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

    assert!(
        !written.iter().any(|f| f.contains("formal")),
        "generation wrote into the project's own Lean package: {written:?}"
    );

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

    let main = std::fs::read_to_string(lean_runner::package_dir(&out).join("Main.lean")).unwrap();
    assert!(main.contains("toJson"), "the generated runner does not use the declared encoding");
    assert!(main.contains("fromJson?"), "the generated runner does not use the declared encoding");

    let missing = lean_runner::toolchain_on(std::ffi::OsStr::new("")).expect_err("nothing on an empty PATH");
    assert!(missing.contains("`lean`") && missing.contains("elan"), "explains nothing: {missing}");
    let empty = scratch("no-lean");
    let _ = std::fs::create_dir_all(&empty);
    assert!(lean_runner::toolchain_on(empty.as_os_str()).is_err(), "a directory without Lean has Lean");
    let _ = std::fs::remove_dir_all(&empty);

    let _ = std::fs::remove_dir_all(&out);
}

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
    let index = tracelean_core::trace::index::build(&root);
    let files = tracelean_core::observe::workspace::snapshot(&root).files;
    let merged = tracelean_core::drt::auto::bindings(&root, &index, &files).expect("the bindings read");
    let config = serde_json::json!({ "bindings": merged });

    const BINDING_KEYS: &[&str] = &[
        "req_id",
        "clause",
        "op",
        "also_checks",
        "model",
        "implementation",
        "also_implemented_by",
        "floors",
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

    let entry = rust_runner::resolve(
        &root,
        &Binding {
            req_id: "REQ-EVID".into(),
            clause: Some("mark_per_bond".into()),
            op: None,
            also_checks: Vec::new(),
            also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
            model: None,
            implementation: CallSpec {
                language: "rust".into(),
                entry: "crates/core/src/evidence.rs::marks".into(),
                params: BTreeMap::new(),
            },
        },
    )
    .expect("resolves");
    let generated = rust_runner::main_rs(&[entry]);
    assert!(
        generated.contains("marks(records, annotated)"),
        "the generated dispatch does not call the entry point directly:\n{generated}"
    );
}

/// @tests REQ-DRT-GEN.shrink_minimal
/// @tests REQ-DRT-RUST.build_error_explained
/// @structural REQ-DRT-GEN.shrink_minimal reason="a property of the shrink loop, which stops only when no candidate diverges; the function under it is the reduction step, and what matters is that every step is strictly smaller"
/// @structural REQ-DRT-RUST.build_error_explained reason="a claim about what a failed build reports, which needs a compiler to produce and is not a value any model computes"
#[test]
fn shrinking_goes_all_the_way_down_and_a_failed_build_says_why() {
    use tracelean_core::drt::gen;
    use tracelean_core::drt::schema::Schema;

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
