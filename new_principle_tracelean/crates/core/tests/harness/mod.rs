#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracelean_core::drt::lean_runner::{self, LeanEntry};
use tracelean_core::drt::rust_runner;
use tracelean_core::drt::run::{run, DrtResult, RunOptions, RunnerSpec};
use tracelean_core::drt::{Binding, CallSpec};
use tracelean_core::drt::coverage::{level, verdict, Floor, Observed, Verdict};
use tracelean_core::drt::schema::Schema;
use tracelean_core::evidence::Level;

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

fn declared(op: &str) -> &'static Binding {
    declared_bindings()
        .iter()
        .find(|binding| binding.op() == op)
        .unwrap_or_else(|| panic!("no binding declares the op `{op}`"))
}

pub fn role() -> Schema {
    let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
    for nullary in ["plain", "path", "entry", "heading", "requirement", "added", "removed"] {
        variants.insert(nullary.to_string(), None);
    }
    variants.insert("mark".to_string(), Some(Box::new(mark_role())));
    let mut claim = BTreeMap::new();
    claim.insert(
        "role".to_string(),
        Schema::simple_enum(&["models", "implements", "tests", "drt", "proves", "pins"]),
    );
    variants.insert("claim".to_string(), Some(Box::new(Schema::Struct { fields: claim })));
    Schema::Enum { variants }
}

pub fn marks() -> Schema {
    let mut fields = BTreeMap::new();
    for mark in ["annotated", "judged", "tested", "proved", "structural"] {
        fields.insert(mark.to_string(), Schema::Bool);
    }
    Schema::Struct { fields }
}

pub fn mark_role() -> Schema {
    let mut fields = BTreeMap::new();
    fields.insert("mark".to_string(), Schema::simple_enum(&["annotated", "judged", "tested", "proved"]));
    fields.insert("held".to_string(), Schema::Bool);
    Schema::Struct { fields }
}

pub fn floors_met(counts: &[(&str, u64, u64)]) {
    let floors: Vec<Floor> = counts
        .iter()
        .map(|(s, at_least, _)| Floor { situation: (*s).into(), at_least: *at_least })
        .collect();
    let observed: Vec<Observed> = counts
        .iter()
        .map(|(s, _, reached)| Observed { situation: (*s).into(), reached: *reached })
        .collect();
    let reached = verdict(floors, observed, Vec::new());
    assert_eq!(reached, Verdict::Met, "the run did not reach its declared floor");
    assert_eq!(
        level(true, reached),
        Level::L3,
        "an agreeing run that met its floor is what L3 means"
    );
}

pub fn each_occurred(counts: &[(&str, u64)]) {
    let floors: Vec<(&str, u64, u64)> =
        counts.iter().map(|(s, reached)| (*s, 1, *reached)).collect();
    floors_met(&floors);
}

fn pending_dir() -> std::path::PathBuf {
    project_root().join(".tracelean").join("pending")
}


#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Half {
    #[serde(default)]
    agreed: std::collections::BTreeMap<String, Run>,
    covered: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lines: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Run {
    cases: u64,
    stamp: String,
}

fn current_stamp(op: &str) -> String {
    let index = tracelean_core::trace::index::build(&project_root());
    tracelean_core::trace::earn::drt_stamp(&index, &declared(op).clauses())
}

fn half_path(op: &str) -> std::path::PathBuf {
    pending_dir().join(format!("{}.json", op.replace('/', "_")))
}

fn read_half(op: &str) -> Half {
    std::fs::read_to_string(half_path(op))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_half(op: &str, half: &Half) {
    let path = half_path(op);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the pending dir");
    std::fs::write(path, serde_json::to_string_pretty(half).expect("a half serialises"))
        .expect("the pending dir is writable");
}

/// @implements REQ-DRT.falsification_only
pub fn agreed(result: &tracelean_core::drt::run::DrtResult) {
    assert!(
        result.agreed(),
        "model and implementation disagree on {}.\n  input: {}\n  model: {:?}\n  impl:  {:?}",
        result.op,
        result.divergence.as_ref().map(|d| d.input.to_string()).unwrap_or_default(),
        result.divergence.as_ref().map(|d| &d.model),
        result.divergence.as_ref().map(|d| &d.implementation),
    );
    every_class_reached(result);
    let waived = declared(&result.op).waive.iter().any(|w| w.situations.iter().any(|s| s == "unreadable input"));
    assert!(
        result.unreadable == 0 || waived,
        "{} of {} cases of `{}` were input the implementation could not read: the generator draws a shape the \
         types do not have",
        result.unreadable,
        result.cases,
        result.op
    );
    let stamp = current_stamp(&result.op);
    let mut half = read_half(&result.op);
    half.agreed.retain(|_, run| run.stamp == stamp);
    half.agreed.insert(result.seed.to_string(), Run { cases: result.cases, stamp: stamp.clone() });
    if declared(&result.op).floors.is_empty() {
        half.covered = Some(stamp);
    }
    write_half(&result.op, &half);
    compose(&result.op);
    if std::env::var_os("TRACELEAN_DRT_LINES").is_some_and(|v| !v.is_empty()) {
        report_lines(&result.op);
    }
}

fn report_lines(op: &str) {
    use tracelean_core::drt::coverage::{floors_of, line_reach, unused_waivers, verdict, Observed, Verdict};
    let root = project_root();
    let base = root.join("target").join("tracelean-drt-lines");
    let binary = tracelean_core::drt::rust_runner::package_dir(&root.join("target").join("tracelean-drt-rust-lines"))
        .join("target/release/tracelean-drt-runner");
    let profiled = match tracelean_core::drt::lines_run::profiled_lines(&root, &binary, &base.join(op)) {
        Ok(profiled) => profiled,
        Err(why) => {
            eprintln!("  lines of {op} not measured: {why}");
            return;
        }
    };
    let binding = declared(op);
    let index = tracelean_core::trace::index::build(&root);
    let mut measured = Vec::new();
    for qualified in binding.clauses() {
        let (req_id, clause) = { let (r, c) = tracelean_core::drt::config::split_op(&qualified); (r.to_string(), c.map(String::from)) };
        for link in index.links.iter().filter(|l| {
            l.role == tracelean_core::trace::annotation::Role::Implements && l.req_id == req_id && l.clause == clause
        }) {
            let anchor = &link.anchor;
            let Some((_, hits)) = profiled.iter().find(|(file, _)| *file == anchor.file) else { continue };
            let Ok(text) = std::fs::read_to_string(root.join(&anchor.file)) else { continue };
            let item: Vec<(u32, String)> = text
                .lines()
                .enumerate()
                .map(|(n, line)| (n as u32 + 1, line.to_string()))
                .filter(|(n, _)| *n > anchor.start_line && *n <= anchor.end_line + 1)
                .collect();
            let first = item.first().map(|(_, line)| line.clone()).unwrap_or_default();
            let observed = line_reach(hits.clone(), item);
            let missed: Vec<String> = match verdict(floors_of(&observed), observed.clone(), binding.waive.clone()) {
                Verdict::Unmet { gaps } => gaps.iter().map(|g| g.situation().to_string()).collect(),
                _ => Vec::new(),
            };
            measured.push((qualified.clone(), anchor.file.clone(), anchor.start_line + 1, first, observed, missed));
        }
    }

    let entry = &binding.implementation.entry;
    let (entry_file, entry_name) = entry.split_once("::").unwrap_or((entry.as_str(), ""));
    let is_entry = |m: &&(String, String, u32, String, Vec<Observed>, Vec<String>)| {
        m.1 == entry_file
            && m.3.split(|c: char| !c.is_alphanumeric() && c != '_').any(|word| word == entry_name)
            && m.3.contains("fn ")
    };
    let mut bound: Vec<_> = measured.iter().filter(is_entry).collect();
    if bound.is_empty() {
        bound = measured
            .iter()
            .filter(|m| m.1 == entry_file && m.0 == op && m.4.iter().any(|o| o.reached > 0))
            .collect();
    }
    assert!(
        !bound.is_empty(),
        "`{op}`: its bound entry `{entry}` is no item claiming the clause, and nothing in its file does: \
         nothing to hold to every line"
    );
    for (_, _, _, _, _, missed) in &bound {
        assert!(missed.is_empty(), "`{op}`: lines of `{entry}` its cases never run, and no waiver says why: {missed:#?}");
    }
    let line_waivers: Vec<_> = binding
        .waive
        .iter()
        .cloned()
        .filter_map(|mut w| {
            w.situations.retain(|s| s.starts_with("line: "));
            (!w.situations.is_empty()).then_some(w)
        })
        .collect();
    let observed: Vec<Observed> = bound.iter().flat_map(|m| m.4.clone()).collect();
    let unused = unused_waivers(floors_of(&observed), observed.clone(), line_waivers);
    assert!(unused.is_empty(), "`{op}` waives lines of `{entry}` its cases run, or that are not in it: {unused:?}");

    let items: Vec<serde_json::Value> = measured
        .iter()
        .map(|(clause, file, from, _, observed, missed)| {
            serde_json::json!({ "clause": clause, "file": file, "from": from, "lines": observed.len(), "missed": missed })
        })
        .collect();
    let report = serde_json::json!({ "op": op, "items": items });
    let _ = std::fs::write(base.join(format!("{op}.json")), serde_json::to_string_pretty(&report).unwrap_or_default());

    let mut half = read_half(op);
    half.lines = Some(current_stamp(op));
    write_half(op, &half);
    compose(op);
}

/// @implements REQ-DRT-COVER.classes_reached
/// @implements REQ-DRT-COVER.waiver_unused_reported
fn every_class_reached(result: &tracelean_core::drt::run::DrtResult) {
    use tracelean_core::drt::coverage::{floors_of, unused_waivers};
    let binding = declared(&result.op);
    let waive: Vec<_> = binding
        .waive
        .iter()
        .cloned()
        .filter_map(|mut w| {
            w.situations.retain(|s| !s.starts_with("line: ") && s != "unreadable input");
            (!w.situations.is_empty()).then_some(w)
        })
        .collect();
    let floors = floors_of(&result.reached);
    let reached = verdict(floors.clone(), result.reached.clone(), waive.clone());
    if let Verdict::Unmet { gaps } = &reached {
        let names: Vec<&str> = gaps.iter().map(|gap| gap.situation()).collect();
        panic!(
            "`{}` agreed, but no case reached these classes of its arguments, so the run \
             is not L3: {names:?}. Reach them in the generator or waive them with a reason \
             in the binding's `waive`.",
            result.op
        );
    }
    let unused = unused_waivers(floors, result.reached.clone(), waive);
    assert!(unused.is_empty(), "`{}` waives what the run reached or never measured: {unused:?}", result.op);
}

/// @implements REQ-DRT-COVER.floor_stated
/// @implements REQ-DRT-COVER.law_coverage
/// @implements REQ-DRT-COVER.vacuous_named
pub fn covered(op: &str, counts: &[(&str, u64)]) {
    let binding = declared(op);
    assert!(
        !binding.floors.is_empty(),
        "`{op}` declares no floors to count against; its coverage is its classes, \
         recorded by `agreed` (REQ-DRT-COVER.floor_stated)"
    );
    for (situation, _) in counts {
        assert!(
            binding.floors.iter().any(|floor| floor.situation == *situation),
            "`{op}` does not declare the situation `{situation}`; the count has nowhere to go"
        );
    }
    let observed: Vec<Observed> = counts
        .iter()
        .map(|(situation, reached)| Observed {
            situation: (*situation).to_string(),
            reached: *reached,
        })
        .collect();
    let reached = verdict(binding.floors.clone(), observed, binding.waive.clone());
    assert_eq!(reached, Verdict::Met, "`{op}` did not reach its declared floor");

    let stamp = current_stamp(op);
    let mut half = read_half(op);
    half.agreed.retain(|_, run| run.stamp == stamp);
    half.covered = Some(stamp);
    write_half(op, &half);
    compose(op);
}


/// @implements REQ-EVID.mark_per_bond
/// @implements REQ-DRT-COVER.floor_unmet_is_not_pass
/// @implements REQ-DRT-BIND.binding_is_the_bond
fn compose(op: &str) {
    let half = read_half(op);
    let root = project_root();
    let binding = declared(op);
    let current = current_stamp(op);
    let stamps: Vec<String> = half.agreed.values().map(|run| run.stamp.clone()).collect();
    let lines_in = binding.implementation.language != "rust" || half.lines.as_deref() == Some(current.as_str());
    if !tracelean_core::trace::earn::halves_compose(
        &stamps,
        half.covered.as_deref().filter(|_| lines_in),
        &current,
        binding.implementations().len(),
    ) {
        return;
    }
    let mut runs: Vec<(u64, u64)> = half
        .agreed
        .iter()
        .filter(|(_, run)| run.stamp == current)
        .filter_map(|(seed, run)| seed.parse::<u64>().ok().map(|seed| (seed, run.cases)))
        .collect();
    runs.sort();
    let Some((seed, cases)) = runs.first().copied() else { return };
    let index = tracelean_core::trace::index::build(&root);
    let established = level(true, Verdict::Met);
    assert_eq!(established, Level::L3, "an agreeing run that met its floor is what L3 means");

    for qualified in binding.clauses() {
        let (req_id, clause) = { let (r, c) = tracelean_core::drt::config::split_op(&qualified); (r.to_string(), c.map(String::from)) };
        match tracelean_core::trace::earn::drt_record(
            &index,
            &req_id,
            clause.as_deref(),
            established,
            seed,
            cases,
            op,
        ) {
            Ok(record) => {
                tracelean_core::trace::store::write(&root, &record).expect("the store is writable");
            }
            Err(why) => eprintln!("  no record for {qualified}: {why:?}"),
        }
    }
}

pub fn runners(op: &str, scratch: &Path) -> (RunnerSpec, RunnerSpec) {
    let binding = declared(op);
    let model = binding.model.as_ref().unwrap_or_else(|| panic!("`{op}` declares no model"));
    let arguments: Vec<&str> = model.arguments.iter().map(String::as_str).collect();
    let model = lean_runner(&model.import, &model.function, op, &arguments, scratch);
    let call = &binding.implementation;
    let clause = binding.clause.as_deref().unwrap_or_default();
    let implementation = match call.language.as_str() {
        "typescript" => ts_runner(&binding.req_id, clause, &call.entry, scratch),
        _ => {
            let params: Vec<(&str, &str)> = call.params.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            rust_runner_with_params(&binding.req_id, clause, &call.entry, &params, scratch)
        }
    };
    (model, implementation)
}

pub fn agree(op: &str, schema: &Schema, seed: u64, cases: u64) -> DrtResult {
    let scratch = scratch(&op.replace(['.', '/'], "-"));
    let (model, implementation) = runners(op, &scratch);
    let result = run(op, schema, &model, &implementation, RunOptions { seed, cases }).expect("both runners answer");
    agreed(&result);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

pub fn strukt(fields: &[(&str, Schema)]) -> Schema {
    Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

pub fn name(examples: &[&str]) -> Schema {
    Schema::Str { max_len: Some(0), examples: examples.iter().map(|s| s.to_string()).collect() }
}
