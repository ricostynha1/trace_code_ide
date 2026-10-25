#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracelean_core::drt::lean_runner::{self, LeanEntry};
use tracelean_core::drt::rust_runner;
use tracelean_core::drt::run::RunnerSpec;
use tracelean_core::drt::{Binding, CallSpec};

pub struct LeanBuild(PathBuf);

impl LeanBuild {
    fn acquire(formal: &Path) -> LeanBuild {
        use std::io::Write;
        let lock = formal.join(".tracelean-build.lock");
        for _ in 0..600 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(mut file) => {
                    let _ = write!(file, "{}", std::process::id());
                    break;
                }
                Err(_) => {
                    if abandoned(&lock) {
                        let _ = std::fs::remove_file(&lock);
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
        LeanBuild(lock)
    }
}

fn abandoned(lock: &Path) -> bool {
    let Ok(held_by) = std::fs::read_to_string(lock) else { return false };
    let Ok(pid) = held_by.trim().parse::<u32>() else { return false };
    if pid == std::process::id() {
        return false;
    }
    Path::new("/proc").is_dir() && !Path::new(&format!("/proc/{pid}")).exists()
}

impl Drop for LeanBuild {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub fn lake_build(formal: &Path, package: &Path) -> std::process::Output {
    let _guard = LeanBuild::acquire(formal);
    lake_build_holding(package)
}

fn lake_build_holding(package: &Path) -> std::process::Output {
    let build =
        || Command::new("lake").arg("build").current_dir(package).output().expect("lake runs");
    let mut built = build();
    for _ in 0..30 {
        if built.status.success()
            || !String::from_utf8_lossy(&built.stderr).contains("configuration lock")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
        built = build();
    }
    built
}

pub fn build_formal(formal: &Path) -> std::process::Output {
    lake_build(formal, formal)
}

pub fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf()
}

pub fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("tracelean-drt-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    path
}

pub fn rust_runner(req: &str, clause: &str, entry_path: &str, scratch: &Path) -> RunnerSpec {
    rust_runner_with_params(req, clause, entry_path, &[], scratch)
}

pub fn rust_runner_with_params(
    req: &str,
    clause: &str,
    entry_path: &str,
    params: &[(&str, &str)],
    scratch: &Path,
) -> RunnerSpec {
    let wanted = CallSpec {
        language: "rust".into(),
        entry: entry_path.into(),
        params: params.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
    };
    if let Some(shared) = shared_rust_runner(req, clause, &wanted) {
        return shared;
    }
    build_rust_runner(&[binding_for(req, clause, wanted)], scratch)
}

fn binding_for(req: &str, clause: &str, implementation: CallSpec) -> Binding {
    Binding {
        req_id: req.into(),
        clause: Some(clause.into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
        model: None,
        implementation,
    }
}

pub fn lines_wanted() -> bool {
    std::env::var_os("TRACELEAN_DRT_LINES").is_some_and(|v| !v.is_empty())
}

pub fn lines_dir(op: &str) -> std::path::PathBuf {
    project_root().join("target").join("tracelean-drt-lines").join(op)
}

pub fn lines_binary() -> std::path::PathBuf {
    rust_runner::package_dir(&project_root().join("target").join("tracelean-drt-rust-lines"))
        .join("target/release/tracelean-drt-runner")
}

fn build_rust_runner(bindings: &[Binding], at: &Path) -> RunnerSpec {
    let root = project_root();
    let entries: Vec<rust_runner::Entry> = bindings
        .iter()
        .map(|binding| rust_runner::resolve(&root, binding).expect("binding resolves"))
        .collect();

    let mut deps = BTreeMap::new();
    deps.insert(
        "tracelean-core".to_string(),
        root.join("crates").join("core").display().to_string(),
    );
    rust_runner::materialize(at, &entries, &deps).expect("generated");

    let dir = rust_runner::package_dir(at);
    let mut build = Command::new("cargo");
    build.args(["build", "--release", "--quiet"]).current_dir(&dir);
    if lines_wanted() {
        build.env("RUSTFLAGS", "-C instrument-coverage");
    }
    let built = build.output().expect("cargo runs");
    assert!(
        built.status.success(),
        "the generated Rust runner did not compile:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    RunnerSpec {
        cmd: vec![dir.join("target/release/tracelean-drt-runner").display().to_string()],
        cwd: None,
    }
}

fn shared_rust_runner(req: &str, clause: &str, wanted: &CallSpec) -> Option<RunnerSpec> {
    let op = tracelean_core::drt::config::qualified_op(req, Some(clause));
    let declared = declared_bindings();
    let binding = declared.iter().find(|b| b.op() == op)?;
    if &binding.implementation != wanted {
        return None;
    }

    use std::sync::OnceLock;
    static BUILT: OnceLock<std::sync::Mutex<Option<RunnerSpec>>> = OnceLock::new();
    let cache = BUILT.get_or_init(|| std::sync::Mutex::new(None));
    let mut cache = cache.lock().expect("the runner cache is not poisoned");
    let placed = |spec: &RunnerSpec| {
        if !lines_wanted() {
            return spec.clone();
        }
        let dir = lines_dir(&op);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the profile directory can be made");
        RunnerSpec { cmd: spec.cmd.clone(), cwd: Some(dir) }
    };
    if let Some(spec) = cache.as_ref() {
        return Some(placed(spec));
    }

    let shared = project_root()
        .join("target")
        .join(if lines_wanted() { "tracelean-drt-rust-lines" } else { "tracelean-drt-rust" });
    use tracelean_core::drt::protocol::{shared_runners, Placed};
    let plan = shared_runners(
        declared
            .iter()
            .map(|b| Placed { op: b.op(), language: b.implementation.language.clone() })
            .collect(),
    );
    let ops: Vec<String> =
        plan.into_iter().find(|s| s.language == "rust").map(|s| s.ops).unwrap_or_default();
    let rust: Vec<Binding> = ops
        .iter()
        .filter_map(|op| declared.iter().find(|b| &b.op() == op))
        .cloned()
        .collect();
    let wanted = rust_mark_of(&rust);
    let binary = rust_runner::package_dir(&shared).join("target/release/tracelean-drt-runner");

    let spec = if current(&shared, &wanted, &binary) {
        RunnerSpec { cmd: vec![binary.display().to_string()], cwd: None }
    } else {
        let _guard = SharedBuild::acquire(&shared);
        if current(&shared, &wanted, &binary) {
            RunnerSpec { cmd: vec![binary.display().to_string()], cwd: None }
        } else {
            let built = build_rust_runner(&rust, &shared);
            mark(&shared, &wanted);
            built
        }
    };
    *cache = Some(spec.clone());
    Some(placed(&spec))
}

pub fn ts_runner(req: &str, clause: &str, entry_path: &str, scratch: &Path) -> RunnerSpec {
    use tracelean_core::drt::ts_runner as generator;

    let root = project_root();
    let binding = Binding {
        req_id: req.into(),
        clause: Some(clause.into()),
        op: None,
        also_checks: Vec::new(),
        also_implemented_by: Vec::new(),
        floors: Vec::new(),
        waive: Vec::new(),
        model: None,
        implementation: CallSpec {
            language: "typescript".into(),
            entry: entry_path.into(),
            params: BTreeMap::new(),
        },
    };
    let spec = binding.implementation.clone();
    let entry = generator::resolve(&root, &binding, &spec).expect("binding resolves");
    generator::materialize(scratch, &[entry]).expect("generated");

    let dir = generator::package_dir(scratch);
    RunnerSpec {
        cmd: vec!["node".to_string(), dir.join("runner.ts").display().to_string()],
        cwd: None,
    }
}

pub fn lean_runner(
    import: &str,
    function: &str,
    op: &str,
    arguments: &[&str],
    scratch: &Path,
) -> RunnerSpec {
    lean_runner::toolchain().expect("a Lean toolchain");
    if let Some(shared) = shared_lean_runner(import, function, op, arguments) {
        return shared;
    }
    let root = project_root();
    let entries = [LeanEntry {
        op: op.to_string(),
        function: function.to_string(),
        arguments: arguments.iter().map(|a| a.to_string()).collect(),
    }];
    lean_runner::materialize(
        scratch,
        &[import],
        "tracelean",
        &root.join("formal").display().to_string(),
        &entries,
    )
    .expect("generated");

    let dir = lean_runner::package_dir(scratch);
    let built = lake_build(&root.join("formal"), &dir);
    assert!(
        built.status.success(),
        "the generated Lean runner did not build:\n{}\n{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    RunnerSpec {
        cmd: vec![dir.join(".lake/build/bin/drtRunner").display().to_string()],
        cwd: None,
    }
}

fn shared_lean_runner(
    import: &str,
    function: &str,
    op: &str,
    arguments: &[&str],
) -> Option<RunnerSpec> {
    let declared = declared_models();
    let entry = declared.iter().find(|(entry, _)| entry.op == op)?;
    let wanted: Vec<String> = arguments.iter().map(|a| a.to_string()).collect();
    if entry.1 != import || entry.0.function != function || entry.0.arguments != wanted {
        return None;
    }

    Some(RunnerSpec {
        cmd: vec![built_shared_lean().join(".lake/build/bin/drtRunner").display().to_string()],
        cwd: None,
    })
}

fn built_shared_lean() -> PathBuf {
    use std::sync::{Mutex, OnceLock};
    static BUILT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    let cache = BUILT.get_or_init(|| Mutex::new(None));
    let mut cache = cache.lock().expect("the runner cache is not poisoned");
    if let Some(dir) = cache.as_ref() {
        return dir.clone();
    }

    let root = project_root();
    let formal = root.join("formal");
    let shared = root.join("target").join("tracelean-drt-lean");
    let dir = lean_runner::package_dir(&shared);
    let binary = dir.join(".lake/build/bin/drtRunner");

    let declared = declared_models();
    let mut imports: Vec<&str> = declared.iter().map(|(_, module)| *module).collect();
    imports.sort_unstable();
    imports.dedup();
    let entries: Vec<LeanEntry> = declared.into_iter().map(|(entry, _)| entry).collect();
    let wanted = mark_of(&imports, &entries);

    if current(&shared, &wanted, &binary) {
        *cache = Some(dir.clone());
        return dir;
    }

    let _guard = LeanBuild::acquire(&formal);
    if !current(&shared, &wanted, &binary) {
        lean_runner::materialize(
            &shared,
            &imports,
            "tracelean",
            &formal.display().to_string(),
            &entries,
        )
        .expect("the shared Lean runner is generated");
        let built = lake_build_holding(&dir);
        assert!(
            built.status.success(),
            "the shared Lean runner did not build:\n{}\n{}",
            String::from_utf8_lossy(&built.stdout),
            String::from_utf8_lossy(&built.stderr)
        );
        mark(&shared, &wanted);
    }
    *cache = Some(dir.clone());
    dir
}

fn mark_of(imports: &[&str], entries: &[LeanEntry]) -> String {
    let mut out = imports.join("\n");
    for entry in entries {
        out.push_str(&format!("\n{} {} {}", entry.op, entry.function, entry.arguments.join(",")));
    }
    out.push_str(&format!("\nmodel {}", source_digest(&project_root().join("formal"), "lean")));
    out
}

fn source_digest(root: &Path, extension: &str) -> String {
    let mut sources: Vec<PathBuf> = Vec::new();
    collect(root, extension, &mut sources);
    sources.sort();
    let mut all = String::new();
    for path in sources {
        all.push_str(&path.display().to_string());
        all.push('\u{1}');
        all.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
        all.push('\u{2}');
    }
    tracelean_core::trace::hash::body(&all, &[], &[])
}

fn collect(dir: &Path, extension: &str, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == ".lake" || name == "target") {
                continue;
            }
            collect(&path, extension, into);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            into.push(path);
        }
    }
}

fn rust_mark_of(bindings: &[Binding]) -> String {
    let mut out: String = bindings
        .iter()
        .map(|b| {
            let params: Vec<String> =
                b.implementation.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{} {} {}", b.op(), b.implementation.entry, params.join(","))
        })
        .collect::<Vec<_>>()
        .join("\n");
    out.push_str(&format!(
        "\nimplementation {}",
        source_digest(&project_root().join("crates"), "rs")
    ));
    out
}

fn current(shared: &Path, wanted: &str, binary: &Path) -> bool {
    binary.is_file()
        && std::fs::read_to_string(shared.join(".tracelean-built-from"))
            .is_ok_and(|found| found == wanted)
}

fn mark(shared: &Path, wanted: &str) {
    let _ = std::fs::write(shared.join(".tracelean-built-from"), wanted);
}

fn declared_bindings() -> &'static [Binding] {
    use std::sync::OnceLock;
    static BINDINGS: OnceLock<Vec<Binding>> = OnceLock::new();
    BINDINGS.get_or_init(|| {
        tracelean_core::drt::config::read(&project_root()).expect("the binding file reads")
    })
}

fn declared_models() -> Vec<(LeanEntry, &'static str)> {
    declared_bindings()
        .iter()
        .filter_map(|binding| {
            let model = binding.model.as_ref()?;
            Some((
                LeanEntry {
                    op: binding.op(),
                    function: model.function.clone(),
                    arguments: model.arguments.clone(),
                },
                model.import.as_str(),
            ))
        })
        .collect()
}

struct SharedBuild(PathBuf);

impl SharedBuild {
    fn acquire(at: &Path) -> SharedBuild {
        let _ = std::fs::create_dir_all(at);
        let lock = at.join(".tracelean-build.lock");
        for _ in 0..600 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = write!(file, "{}", std::process::id());
                    break;
                }
                Err(_) => {
                    if abandoned(&lock) {
                        let _ = std::fs::remove_file(&lock);
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        }
        SharedBuild(lock)
    }
}

impl Drop for SharedBuild {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
