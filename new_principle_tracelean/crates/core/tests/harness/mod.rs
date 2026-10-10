//! Shared plumbing for differential tests: generate a runner for each side,
//! build it, and hand back how to start it.
//!
//! Every suite includes the whole module and uses a part of it, so the parts a
//! given binary does not reach are not dead code — they are the parts another
//! binary reaches.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracelean_core::drt::lean_runner::{self, LeanEntry};
use tracelean_core::drt::rust_runner;
use tracelean_core::drt::run::RunnerSpec;
use tracelean_core::drt::{Binding, CallSpec};

/// One builder of the shared Lean package at a time, across test processes.
///
/// A lock file rather than a mutex: the suites are separate processes, so
/// nothing in one of them can exclude the others.
pub struct LeanBuild(PathBuf);

impl LeanBuild {
    fn acquire(formal: &Path) -> LeanBuild {
        use std::io::Write;
        let lock = formal.join(".tracelean-build.lock");
        // Bounded: a suite killed mid-build would otherwise leave every later
        // run waiting forever, and building anyway is better than hanging —
        // the race it guards against is unlikely, not certain.
        for _ in 0..600 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
                Ok(mut file) => {
                    // Whose lock it is, so a later run can tell a build in
                    // progress from a build that was killed.
                    let _ = write!(file, "{}", std::process::id());
                    break;
                }
                Err(_) => {
                    if abandoned(&lock) {
                        // Reclaim rather than wait it out. A killed run never
                        // gets to drop its guard, and without this every suite
                        // after it pays the full five minutes for nothing.
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

/// Whether a lock is held by a process that no longer exists.
///
/// Read through `/proc`, which is the cheapest thing that is actually true on
/// the platform these suites run on. Anywhere without it, this says nothing is
/// abandoned and the bounded wait is the fallback it always was.
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

/// Build a lake package, holding the guard and retrying past lake's own lock.
///
/// One function because every caller needs the same two protections: the guard
/// serialises builds of the shared `formal/` package across suites, and lake's
/// configuration lock is a second, finer-grained one that a build started before
/// this test began may still hold.
pub fn lake_build(formal: &Path, package: &Path) -> std::process::Output {
    let _guard = LeanBuild::acquire(formal);
    lake_build_holding(package)
}

/// The build itself, for a caller that already holds the guard.
///
/// The guard is a lock file created with `create_new`, so it is not reentrant:
/// a caller that needs to generate the package *and* build it under one lock
/// has to call this rather than `lake_build`.
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

/// Build the shared model package itself, rather than a runner that uses it.
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

/// The Rust side for one binding, answered by one shared runner.
///
/// See `shared_rust_runner`: every declared binding's entry lives in one
/// generated crate, built once and kept, because sixty-seven crates that differ
/// only in which function they call are sixty-seven `cargo build --release`
/// runs of the same dependency graph.
pub fn rust_runner(req: &str, clause: &str, entry_path: &str, scratch: &Path) -> RunnerSpec {
    rust_runner_with_params(req, clause, entry_path, &[], scratch)
}

/// As `rust_runner`, with model field names mapped onto implementation
/// parameter names where the two spell them differently.
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

/// A binding in the shape the generator wants, for a call the file does not
/// declare.
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

/// Whether the differential runs measure the lines their implementations run
/// (`TRACELEAN_DRT_LINES=1`): the shared Rust runner is then built with
/// `-C instrument-coverage`, elsewhere, and each op's runner leaves its profile
/// in a directory of its own (`lines_dir`).
pub fn lines_wanted() -> bool {
    std::env::var_os("TRACELEAN_DRT_LINES").is_some_and(|v| !v.is_empty())
}

/// Where the instrumented runner for `op` leaves its profiles.
pub fn lines_dir(op: &str) -> std::path::PathBuf {
    project_root().join("target").join("tracelean-drt-lines").join(op)
}

/// The instrumented shared runner's executable.
pub fn lines_binary() -> std::path::PathBuf {
    rust_runner::package_dir(&project_root().join("target").join("tracelean-drt-rust-lines"))
        .join("target/release/tracelean-drt-runner")
}

/// Generate and build a Rust runner carrying the given bindings.
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

/// One Rust runner for every declared binding, built once and kept.
///
/// The Lean side's problem, in the other language and worse: every suite
/// generated a crate of its own, in a per-test scratch directory, and ran
/// `cargo build --release` on it. Sixty-seven crates, each depending on
/// `tracelean-core`, none reused. The crates differed only in which function
/// `main` calls.
///
/// So the generator is given every entry the binding file declares, at a path
/// all the suites share and which outlives the run. `materialize` only rewrites
/// what changed and cargo caches the rest, so the first suite to want it pays
/// for it, every later suite finds it done, and a second run finds it done.
///
/// Serialised behind a lock for the same reason the Lean one is: the directory
/// is shared between processes, and two cargo builds in it at once is a race
/// even when both would write the same bytes.
///
/// Returns `None` when the call is not a declared binding's own call — the
/// negative tests bind the wrong function deliberately, and a shared runner
/// that quietly answered them with the right one would turn a test of the
/// harness into a test of nothing.
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
    // Measuring lines, each op's runner runs in a directory of its own, where
    // the instrumented process leaves its profile on exit.
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
    // Which ops the one Rust process carries is `shared_runners`' decision, not
    // a filter written here: one process per language, every op of it once.
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

    // The same mark the Lean side keeps, for the same reason: without it every
    // one of fifty-six processes takes the lock and runs a `cargo build` with
    // nothing to do, one after another.
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

/// Generate the TypeScript side for one binding.
///
/// No build: Node strips the types and runs the project's own source, so what
/// is compared is what the browser loads.
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

/// Generate and build the Lean side for one binding.
///
/// Almost every call is answered by one shared runner that carries every op the
/// binding file declares; see `shared_lean_runner`. A call the binding file does
/// not describe — the negative tests bind the wrong function on purpose — still
/// gets a package of its own.
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

/// One Lean runner for every declared binding, built once and kept.
///
/// Every generated runner requires the one lake package in `formal/`, and cargo
/// runs the suites in parallel. Two lake builds writing that package's output at
/// once is a race in a shared directory — one has been seen to create
/// `build/lib/TraceLean` while the other was writing an olean into it — so the
/// builds are serialised behind a lock. Serialised builds are also the whole
/// cost of a full run: a package per *call*, in a per-test scratch directory,
/// meant a build per call and nothing ever reused. There were sixty-eight
/// calls.
///
/// So one package is generated from `.tracelean/drt.json`, importing every
/// module any binding names and carrying every op, at a path all the suites
/// share and which outlives the run. `main_lean` dispatches on the op and
/// `materialize` only rewrites what changed, so the first suite to want it pays
/// for it, every later suite finds it done, and a second run finds it done.
///
/// **One rather than thirty-two.** It used to be a package per module, because
/// eleven fully-qualified names in the model were defined twice in modules that
/// never import each other — `TraceLean.Role` was `Annotation`'s and `View`'s —
/// so importing all of them at once did not compile and importing one at a time
/// could not collide. Each module now carries its own namespace under
/// `TraceLean`, so `TraceLean.View.Role` and `TraceLean.Annotation.Role` are
/// different names and one package is enough.
///
/// Returns `None` when the call is not a declared binding's own call, so a test
/// that deliberately binds the wrong function cannot be quietly answered by the
/// right one.
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

/// The one built runner, generating and building it once.
///
/// The in-process cache is not enough. The suites are fifty-six separate
/// processes, so without a mark on disk each one takes the lock and runs a
/// `lake build` that has nothing to do — and because the lock serialises them,
/// those no-ops are the one thing in a warm run that cannot happen in parallel.
/// Measured: they were most of a warm suite's wall clock.
///
/// So what the runner was built from is written beside it. A process whose
/// inputs match the mark, and whose binary is there, has nothing to check and
/// takes no lock.
fn built_shared_lean() -> PathBuf {
    use std::sync::{Mutex, OnceLock};
    static BUILT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    let cache = BUILT.get_or_init(|| Mutex::new(None));
    // Held across the build, so two threads of one suite build it once rather
    // than racing into the same directory.
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

    // Generating and building under one lock, because the directory is shared
    // between processes: two suites writing `Main.lean` while a third's lake
    // reads it is a race, even though all three would write the same bytes.
    let _guard = LeanBuild::acquire(&formal);
    // Re-read under the lock: another process may have built it while this one
    // waited, and rebuilding what is already there is the cost this avoids.
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

/// What a shared runner was generated from, as text.
///
/// The inputs themselves rather than a hash of them: the list is small, a
/// mismatch is readable by a person looking at why a rebuild happened, and
/// there is no digest to be wrong about.
fn mark_of(imports: &[&str], entries: &[LeanEntry]) -> String {
    let mut out = imports.join("\n");
    for entry in entries {
        out.push_str(&format!("\n{} {} {}", entry.op, entry.function, entry.arguments.join(",")));
    }
    // And what the model said, not only which parts of it were asked for.
    //
    // Without this the mark moved only when a *binding* changed, so editing a
    // model and running the suites compared the new implementation against a
    // runner built from the old one — and it did not error, it disagreed, which
    // is worse. It was found the day `Role` gained a payload: every suite
    // reported the model failing to parse a role the model itself now emits.
    // A project whose subject is stale links had one in its own harness.
    out.push_str(&format!("\nmodel {}", source_digest(&project_root().join("formal"), "lean")));
    out
}

/// A digest of every source a runner would be built from.
///
/// Read rather than stat: a checkout, a branch switch or a rebuild can all move
/// a timestamp without changing a byte, and the question here is whether the
/// runner would come out the same.
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
            // Build output, which changes on every build — including the build
            // this mark is deciding whether to run.
            if path.file_name().is_some_and(|name| name == ".lake" || name == "target") {
                continue;
            }
            collect(&path, extension, into);
        } else if path.extension().is_some_and(|ext| ext == extension) {
            into.push(path);
        }
    }
}

/// The same, for the bindings a Rust runner carries.
///
/// And the implementation's own sources, for the reason `mark_of` gives. The
/// gap was worse on this side: a stale Rust runner and a stale Lean runner both
/// fail to read a value whose shape has changed, and two runners that fail
/// identically *agree* — so the suite went green while neither side had been
/// asked the question.
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

/// Whether the runner at a shared path was built from exactly these inputs and
/// is still there to run.
fn current(shared: &Path, wanted: &str, binary: &Path) -> bool {
    binary.is_file()
        && std::fs::read_to_string(shared.join(".tracelean-built-from"))
            .is_ok_and(|found| found == wanted)
}

/// Record what the runner at a shared path was built from.
fn mark(shared: &Path, wanted: &str) {
    let _ = std::fs::write(shared.join(".tracelean-built-from"), wanted);
}

/// Every binding the file declares, read once.
///
/// Through `drt::config`, which now describes both halves of a binding. It used
/// to describe the implementation only — the `model` key existed in the file
/// and had no field to land in — so this was a private JSON parse that nothing
/// validated. `bindings_are_real` keeps the file honest about both sides.
fn declared_bindings() -> &'static [Binding] {
    use std::sync::OnceLock;
    static BINDINGS: OnceLock<Vec<Binding>> = OnceLock::new();
    BINDINGS.get_or_init(|| {
        tracelean_core::drt::config::read(&project_root()).expect("the binding file reads")
    })
}

/// Every model call the binding file declares, with the module it lives in.
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

/// One builder of a shared generated package at a time, across test processes.
///
/// The same problem the Lean guard solves, for any directory the suites share:
/// two processes generating and building in it at once is a race even when both
/// would write the same bytes.
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
