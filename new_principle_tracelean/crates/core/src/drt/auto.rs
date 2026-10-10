//! Differential tests nobody had to write: `tracelean-trace . --drt`.
//!
//! For every clause a Lean `def` models and a Rust `fn` implements, and that
//! no binding in `.tracelean/drt.json` already covers: read both signatures,
//! pair them (`derive`), generate a runner for each side in a scratch
//! directory, ask both the same generated cases, and — when they agree and the
//! generator reached every class of every argument — record L3 for the bond.
//!
//! Works on any project: the Lean side requires the nearest Lake package by
//! path, or packages the model's directory when there is none; the Rust side
//! depends on the enclosing library crate, or includes the file as a module
//! when there is none.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::classes;
use super::coverage::{level, verdict, Floor, Observed, Verdict};
use super::derive::{self, Derived};
use super::lean_runner::{self, LeanEntry};
use super::rerun::{rerun, HeldRun};
use super::run::{run, RunOptions, RunnerSpec};
use super::rust_runner::{self, Entry};
use crate::trace::anchor::AnchorKind;
use crate::trace::annotation::Role;
use crate::trace::index::Index;

/// How many cases, from which seed. Fixed, so a run is reproducible from the
/// record it leaves.
const SEED: u64 = 1;
const CASES: u64 = 2_000;

/// One clause that can be tested.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub op: String,
    pub req_id: String,
    pub clause: Option<String>,
    /// The model's file, and its Lean name.
    pub model: (String, String),
    /// The implementation's file, and its function name.
    pub implementation: (String, String),
    pub derived: Result<Derived, Vec<String>>,
}

/// What became of one clause.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Agreed on every case and reached every class: L3 was recorded.
    Agreed { cases: u64 },
    /// Agreed, but some class of some argument was never generated.
    Uncovered { missing: Vec<String> },
    Diverged { input: String, model: String, implementation: String },
    /// The signatures do not pair.
    Mismatch(Vec<String>),
    /// A runner did not build or did not answer.
    Failed(String),
    /// Agreed before, and the model, implementation and requirement it ran
    /// against hash as they did: nothing was run.
    Cached,
}

/// The runs `held` keeps for this candidate's clause, as `rerun` reads them.
fn held_runs(index: &Index, held: &[crate::trace::record::Evidence], c: &Candidate) -> Vec<HeldRun> {
    held_for(index, held, &c.req_id, &c.clause)
}

/// Each file holding a `@drt` claim — a differential suite — with the clauses
/// it claims that no current agreed run holds: a suite with none due need not
/// run (`tools/differential-all.sh`). The op is the one the held record names,
/// since a binding that also checks a clause records it under its own.
pub fn suites_due(root: &Path, index: &Index) -> BTreeMap<String, Vec<String>> {
    let held = crate::trace::earn::merge(
        crate::trace::lockfile::read(root).map(|l| l.evidence).unwrap_or_default(),
        crate::trace::store::read_all(root),
    );
    let live: Vec<String> = index.links.iter().map(|l| l.link_hash.clone()).collect();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for link in index.links.iter().filter(|l| l.role == Role::Drt) {
        let runs = held_for(index, &held, &link.req_id, &link.clause);
        let current = runs.iter().any(|h| !rerun(vec![h.clone()], h.op.clone(), live.clone(), false));
        let due = out.entry(link.anchor.file.clone()).or_default();
        if !current {
            due.push(super::qualified_op(&link.req_id, link.clause.as_deref()));
        }
    }
    for due in out.values_mut() {
        due.sort();
        due.dedup();
    }
    out
}

fn held_for(index: &Index, held: &[crate::trace::record::Evidence], req_id: &str, clause: &Option<String>) -> Vec<HeldRun> {
    use crate::evidence::{Bond, Level};
    use crate::trace::record::{Detail, StalenessInput};
    held.iter()
        .filter(|r| r.key.req_id == req_id && &r.key.clause == clause && r.key.bond == Bond::ModelImpl)
        .filter_map(|r| match &r.detail {
            Detail::Drt { op, .. } => Some(HeldRun {
                op: op.clone(),
                agreed: r.effective_level() >= Level::L3,
                record: StalenessInput { link_hash: r.link_hash.clone(), inputs: r.inputs.clone() },
                current: crate::trace::earn::current_inputs(index, r),
            }),
            _ => None,
        })
        .collect()
}

fn declaration(files: &BTreeMap<String, String>, file: &str, start: u32, end: u32) -> String {
    files
        .get(file)
        .map(|t| t.lines().skip(start as usize).take((end.saturating_sub(start) + 1) as usize).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// Every clause with a Lean function modelling it and a Rust function
/// implementing it, minus those a binding already declares.
pub fn candidates(root: &Path, index: &Index, files: &BTreeMap<String, String>) -> Vec<Candidate> {
    let bound: Vec<String> = super::config::read(root).unwrap_or_default().iter().map(|b| b.op()).collect();
    let mut clauses: Vec<(String, Option<String>)> =
        index.links.iter().filter(|l| l.role == Role::Models).map(|l| (l.req_id.clone(), l.clause.clone())).collect();
    clauses.sort();
    clauses.dedup();
    let mut out = Vec::new();
    for (req_id, clause) in clauses {
        let op = super::qualified_op(&req_id, clause.as_deref());
        if bound.contains(&op) {
            continue;
        }
        let here: Vec<_> = index.links.iter().filter(|l| l.req_id == req_id && l.clause == clause).collect();
        let model = here.iter().filter(|l| l.role == Role::Models && l.anchor.file.ends_with(".lean")).find_map(|l| {
            let AnchorKind::Decl { symbol_path } = &l.anchor.kind else { return None };
            let text = declaration(files, &l.anchor.file, l.anchor.start_line, l.anchor.end_line);
            let signature = derive::lean_signature(&text)?;
            // A specification is a predicate, not something to run.
            (signature.1 != derive::Ty::Named { name: "Prop".into() })
                .then(|| (l.anchor.file.clone(), symbol_path.replace("::", "."), signature))
        });
        let implementation = here.iter().filter(|l| l.role == Role::Implements && l.anchor.file.ends_with(".rs")).find_map(|l| {
            let AnchorKind::Decl { symbol_path } = &l.anchor.kind else { return None };
            let symbol = symbol_path.rsplit("::").next()?.to_string();
            let signature = derive::rust_signature(files.get(&l.anchor.file)?, &symbol)?;
            Some((l.anchor.file.clone(), symbol, signature))
        });
        let (Some((model_file, model_name, theirs)), Some((impl_file, symbol, ours))) = (model, implementation) else {
            continue;
        };
        let lean = files.get(&model_file).map(|t| derive::lean_structs(t)).unwrap_or_default();
        let rust = files.get(&impl_file).map(|t| derive::rust_structs(t)).unwrap_or_default();
        out.push(Candidate {
            op,
            req_id: req_id.clone(),
            clause: clause.clone(),
            derived: derive::derive(theirs, ours, &lean, &rust),
            model: (model_file, model_name),
            implementation: (impl_file, symbol),
        });
    }
    out
}

/// The nearest directory at or above `file`'s, within `root`, with a lakefile.
fn lake_root(root: &Path, file: &Path) -> Option<PathBuf> {
    let mut at = file.parent();
    while let Some(dir) = at {
        if dir.join("lakefile.lean").is_file() || dir.join("lakefile.toml").is_file() {
            return Some(dir.to_path_buf());
        }
        if dir == root {
            return None;
        }
        at = dir.parent();
    }
    None
}

/// A Lake package's name, from its lakefile.
fn package_name(dir: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(dir.join("lakefile.lean")) {
        let line = text.lines().find_map(|l| l.trim().strip_prefix("package "))?;
        return Some(line.split_whitespace().next()?.trim_matches(['«', '»']).to_string());
    }
    let text = std::fs::read_to_string(dir.join("lakefile.toml")).ok()?;
    let line = text.lines().find_map(|l| l.trim().strip_prefix("name"))?;
    Some(line.trim_start().strip_prefix('=')?.trim().trim_matches('"').to_string())
}

fn module_of(rel: &Path) -> String {
    rel.with_extension("").iter().filter_map(|c| c.to_str()).collect::<Vec<_>>().join(".")
}

/// Every `.lean` file under `dir`, relative to it.
fn lean_files(dir: &Path, under: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            lean_files(&path, under, out);
        } else if path.extension().is_some_and(|e| e == "lean") {
            if let Ok(rel) = path.strip_prefix(under) {
                out.push(rel.to_path_buf());
            }
        }
    }
}

/// Build the Lean runner for `candidates`, all of whose models share one home.
fn lean_side(root: &Path, scratch: &Path, chosen: &[&Candidate]) -> Result<RunnerSpec, String> {
    lean_runner::toolchain()?;
    let first = root.join(&chosen[0].model.0);
    let (package, path, imports): (String, PathBuf, Vec<String>) = match lake_root(root, &first) {
        Some(dir) => {
            let name = package_name(&dir).ok_or("a lakefile without a package name")?;
            let imports = chosen
                .iter()
                .filter_map(|c| root.join(&c.model.0).strip_prefix(&dir).ok().map(module_of))
                .collect();
            (name, dir, imports)
        }
        None => {
            // No package: make one of the model's directory, copied.
            let home = first.parent().ok_or("a model with no directory")?.to_path_buf();
            let model = scratch.join("model");
            let mut files = Vec::new();
            lean_files(&home, &home, &mut files);
            for rel in &files {
                let to = model.join(rel);
                std::fs::create_dir_all(to.parent().unwrap_or(&model)).map_err(|e| e.to_string())?;
                std::fs::copy(home.join(rel), to).map_err(|e| e.to_string())?;
            }
            let roots: Vec<String> = files.iter().map(|rel| format!("`{}", module_of(rel))).collect();
            let lakefile = format!(
                "-- Generated by TraceLean.\nimport Lake\nopen Lake DSL\n\npackage model\n\n@[default_target]\nlean_lib Model where\n  roots := #[{}]\n",
                roots.join(", ")
            );
            std::fs::write(model.join("lakefile.lean"), lakefile).map_err(|e| e.to_string())?;
            let imports = chosen
                .iter()
                .filter_map(|c| root.join(&c.model.0).strip_prefix(&home).ok().map(module_of))
                .collect();
            ("model".to_string(), model, imports)
        }
    };
    let entries: Vec<LeanEntry> = chosen
        .iter()
        .filter_map(|c| {
            let derived = c.derived.as_ref().ok()?;
            Some(LeanEntry { op: c.op.clone(), function: c.model.1.clone(), arguments: derived.arguments.clone() })
        })
        .collect();
    let imports: Vec<&str> = imports.iter().map(String::as_str).collect();
    lean_runner::materialize(scratch, &imports, &package, &path.display().to_string(), &entries).map_err(|e| e.to_string())?;
    let dir = lean_runner::package_dir(scratch);
    let built = Command::new("lake").arg("build").current_dir(&dir).output().map_err(|e| e.to_string())?;
    if !built.status.success() {
        return Err(format!("the Lean runner did not build:\n{}", String::from_utf8_lossy(&built.stdout)));
    }
    Ok(RunnerSpec { cmd: vec![dir.join(".lake/build/bin/drtRunner").display().to_string()], cwd: None })
}

/// A crate's package name, from its `Cargo.toml`.
fn crate_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package {
            if let Some(value) = line.strip_prefix("name") {
                return Some(value.trim_start().strip_prefix('=')?.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

/// Build the Rust runner for `candidates`.
fn rust_side(root: &Path, scratch: &Path, chosen: &[&Candidate]) -> Result<RunnerSpec, String> {
    let mut deps = BTreeMap::new();
    let mut included = String::new();
    let mut entries = Vec::new();
    for (n, c) in chosen.iter().enumerate() {
        let Ok(derived) = &c.derived else { continue };
        let rel = Path::new(&c.implementation.0);
        // A library crate holding the file is depended on; anything else is
        // included as a module of the runner.
        let components: Vec<&str> = rel.iter().filter_map(|p| p.to_str()).collect();
        let crate_dir = components.iter().rposition(|p| *p == "src").map(|at| root.join(components[..at].iter().collect::<PathBuf>()));
        let call_path = match (crate_dir, rust_runner::module_path(root, rel)) {
            (Some(dir), Some(module)) if dir.join("src/lib.rs").is_file() => {
                let name = crate_name(&dir).ok_or("a Cargo.toml without a package name")?;
                deps.insert(name, dir.display().to_string());
                format!("{module}::{}", c.implementation.1)
            }
            _ => {
                included.push_str(&format!("#[path = {:?}]\n#[allow(dead_code)]\nmod implementation{n};\n", root.join(rel).display().to_string()));
                format!("implementation{n}::{}", c.implementation.1)
            }
        };
        let parameters: Vec<String> = derived
            .arguments
            .iter()
            .map(|a| derived.params.get(a).cloned().unwrap_or_else(|| a.clone()))
            .collect();
        entries.push(Entry { op: c.op.clone(), call_path, parameters, sources: derived.arguments.clone() });
    }
    let dir = rust_runner::materialize(scratch, &entries, &deps).map_err(|e| e.to_string())?;
    if !included.is_empty() {
        let main = std::fs::read_to_string(dir.join("src/main.rs")).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("src/main.rs"), included + &main).map_err(|e| e.to_string())?;
    }
    let built = Command::new("cargo").args(["build", "--release", "--quiet"]).current_dir(&dir).output().map_err(|e| e.to_string())?;
    if !built.status.success() {
        return Err(format!("the Rust runner did not compile:\n{}", String::from_utf8_lossy(&built.stderr)));
    }
    Ok(RunnerSpec { cmd: vec![dir.join("target/release/tracelean-drt-runner").display().to_string()], cwd: None })
}

/// Run one candidate against built runners.
fn exercise(c: &Candidate, model: &RunnerSpec, implementation: &RunnerSpec) -> Outcome {
    let Ok(derived) = &c.derived else { return Outcome::Mismatch(c.derived.clone().err().unwrap_or_default()) };
    let result = match run(&c.op, &derived.schema, model, implementation, RunOptions { seed: SEED, cases: CASES, shrink_rounds: 100 }) {
        Ok(result) => result,
        Err(error) => return Outcome::Failed(format!("{error:?}")),
    };
    if let Some(d) = result.divergence {
        let said = |r: &super::Reply| match (&r.output, &r.error) {
            (Some(out), _) => out.to_string(),
            (None, Some(error)) => format!("error: {error}"),
            _ => "nothing".into(),
        };
        return Outcome::Diverged { input: d.input.to_string(), model: said(&d.model), implementation: said(&d.implementation) };
    }
    // The same stream the run drew, counted by class: every class of every
    // argument must be reached (REQ-DRT-COVER.classes_reached).
    let observed: Vec<Observed> = classes::reached(derived.schema.clone(), SEED, result.cases);
    let floors: Vec<Floor> =
        observed.iter().map(|o| Floor { situation: o.situation.clone(), at_least: 1 }).collect();
    let missing: Vec<String> =
        observed.iter().filter(|o| o.reached == 0).map(|o| o.situation.clone()).collect();
    let judged = verdict(floors, observed, Vec::new());
    if judged != Verdict::Met {
        return Outcome::Uncovered { missing };
    }
    let _ = level(true, judged);
    Outcome::Agreed { cases: result.cases }
}

/// Test every candidate; record L3 where it was earned; say what happened.
///
/// A candidate already agreed against inputs that hash as they do now is not
/// run again (`Cached`), and no runner is built when none is left; `again`
/// runs every one regardless.
pub fn run_all(root: &Path, index: &Index, files: &BTreeMap<String, String>, again: bool) -> Vec<(Candidate, Outcome)> {
    let found = candidates(root, index, files);
    let held = crate::trace::earn::merge(
        crate::trace::lockfile::read(root).map(|l| l.evidence).unwrap_or_default(),
        crate::trace::store::read_all(root),
    );
    let live: Vec<String> = index.links.iter().map(|l| l.link_hash.clone()).collect();
    let cached = |c: &Candidate| c.derived.is_ok() && !rerun(held_runs(index, &held, c), c.op.clone(), live.clone(), again);
    let scratch = std::env::temp_dir().join(format!("tracelean-drt-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&scratch);
    let ready: Vec<&Candidate> = found.iter().filter(|c| c.derived.is_ok() && !cached(c)).collect();
    let runners = if ready.is_empty() {
        Err(String::new())
    } else {
        // Lake and cargo build side by side; neither reads the other's output.
        let (model, implementation) =
            std::thread::scope(|s| {
                let lean = s.spawn(|| lean_side(root, &scratch, &ready));
                let rust = rust_side(root, &scratch, &ready);
                (lean.join().unwrap_or_else(|_| Err("the Lean build panicked".into())), rust)
            });
        model.and_then(|m| implementation.map(|i| (m, i)))
    };
    // Each run starts runners of its own, so clauses run side by side.
    let outcomes = super::run::par_map(&found, |c| match (&c.derived, &runners) {
        _ if cached(c) => Outcome::Cached,
        (Err(problems), _) => Outcome::Mismatch(problems.clone()),
        (Ok(_), Err(why)) => Outcome::Failed(why.clone()),
        (Ok(_), Ok((model, implementation))) => exercise(c, model, implementation),
    });
    let mut out = Vec::new();
    for (c, outcome) in found.iter().zip(outcomes) {
        if let Outcome::Agreed { cases } = &outcome {
            let established = level(true, Verdict::Met);
            if let Ok(record) =
                crate::trace::earn::derived_drt_record(index, &c.req_id, c.clause.as_deref(), established, SEED, *cases, &c.op)
            {
                let _ = crate::trace::store::write(root, &record);
            }
        }
        out.push((c.clone(), outcome));
    }
    let _ = std::fs::remove_dir_all(&scratch);
    out
}
