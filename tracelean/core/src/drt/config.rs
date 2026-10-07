//! `.tracelean/drt.json` — how a requirement's model and implementation are
//! driven, and what shape their input has.
//!
//! This file is versioned with the project, and it is normally the *only*
//! thing a project writes: a binding says which function the model corresponds
//! to and how the model's field names are spelled on the implementation side,
//! and TraceLean's own runner does the calling. A hand-written adapter process
//! remains available for a datatype the binding cannot describe, but it is the
//! exception rather than the price of admission.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::protocol::RunnerSpec;
use super::run::CoverageFloor;
use super::schema::Schema;

/// One (requirement, clause) bound to a model runner and an implementation
/// runner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub req_id: String,
    #[serde(default)]
    pub clause: Option<String>,
    /// Entry point name, sent as `op`. `"default"` when there is only one.
    #[serde(default = "default_op")]
    pub op: String,
    pub model: RunnerSpec,
    pub implementation: CallSpec,
    pub input: Schema,
    #[serde(default)]
    pub coverage_floor: CoverageFloor,
}

fn default_op() -> String {
    "default".into()
}

/// The `op` that selects one binding's entry point in the shared model runner.
///
/// One runner binary serves every binding in the project, dispatching on `op`,
/// so an op has to be unique project-wide. It used to default to `"default"`
/// for every binding, which meant the runner's `match` had several arms with
/// the same literal: the first one won and every other requirement was silently
/// answered by the wrong model function. Qualifying it by requirement and
/// clause makes collisions impossible and the dispatch readable.
pub fn qualified_op(req_id: &str, clause: Option<&str>) -> String {
    match clause {
        Some(c) => format!("{req_id}.{c}"),
        None => req_id.to_string(),
    }
}

/// How the implementation side of a binding is driven: a function to call,
/// named the way the project spells it.
///
/// There is deliberately no second form. An earlier version also allowed a
/// hand-written adapter process, and the argument against it is not that it was
/// duplication -- it is that an adapter is an untrusted participant in the very
/// comparison it exists to enable. Arbitrary code sitting between the
/// implementation and the comparator can make a divergence disappear, and that
/// is the one place in this system where hiding one would be both easy and
/// invisible. A binding describes the call and nothing more, so the only code
/// that runs is the implementation's own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSpec {
    /// `"python"` today. The runner for a language is shipped with TraceLean.
    pub language: String,
    /// `path/to/file.py::symbol`, relative to the project root. A path rather
    /// than a module name because this project has no import conventions to
    /// rely on: the implementation may live anywhere.
    pub entry: String,
    /// Model field name -> implementation parameter name, for the fields where
    /// the two disagree. Absent keys pass through unchanged.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// An optional `path::symbol` that reshapes the implementation's result
    /// into the model's JSON. Needed only when a return type is genuinely
    /// unmatchable; the runner already unwraps dataclasses, named tuples and
    /// plain objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convert: Option<String>,
}

/// Where a language's shipped runner is written, and what starts it.
///
/// The runner is TraceLean's code, not the project's, so it goes under
/// `.tracelean/` -- a cache directory -- and is rewritten whenever it differs
/// from the version built into this binary. Putting it in the project tree
/// would make it look like something a person should maintain.
pub fn materialize_runner(root: &Path, language: &str) -> Result<PathBuf, String> {
    let (name, source) = match language {
        "python" => ("tracelean_drt_runner.py", include_str!("runners/python_runner.py")),
        other => {
            return Err(format!(
                "no shipped runner for {other}; bind it with a `cmd` adapter instead"
            ))
        }
    };
    let dir = root.join(".tracelean").join("drt");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(name);
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current != source {
        std::fs::write(&path, source).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(path)
}

impl CallSpec {
    /// Resolve to something spawnable, writing the shipped runner if needed.
    ///
    /// `bindings` are every binding in the project: one runner process serves
    /// all of them, dispatching on `op`, exactly as the generated Lean runner
    /// does. Loading them all up front also means a binding that cannot load is
    /// reported per case instead of killing the run.
    pub fn spec(&self, root: &Path, bindings: &[Binding]) -> Result<RunnerSpec, String> {
        let runner = materialize_runner(root, &self.language)?;
        let table = write_binding_table(root, &self.language, bindings)?;
        Ok(RunnerSpec {
            cmd: vec![
                interpreter_for(&self.language).to_string(),
                runner.to_string_lossy().into_owned(),
                "--root".into(),
                root.to_string_lossy().into_owned(),
                "--bindings".into(),
                table.to_string_lossy().into_owned(),
            ],
            cwd: Some(root.to_path_buf()),
            env: BTreeMap::new(),
        })
    }
}

fn interpreter_for(language: &str) -> &'static str {
    match language {
        "python" => "python3",
        _ => "sh",
    }
}

/// The op table the shipped runner reads: only what it needs, so the runner
/// never has to understand the rest of `drt.json`.
fn write_binding_table(
    root: &Path,
    language: &str,
    bindings: &[Binding],
) -> Result<PathBuf, String> {
    let entries: Vec<serde_json::Value> = bindings
        .iter()
        .filter(|b| b.implementation.language == language)
        .map(|b| {
            serde_json::json!({
                "op": b.op,
                "entry": b.implementation.entry,
                "params": b.implementation.params,
                "convert": b.implementation.convert,
            })
        })
        .collect();
    let dir = root.join(".tracelean").join("drt");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(format!("bindings.{language}.json"));
    let text = serde_json::to_string_pretty(&entries)
        .map_err(|e| format!("serializing the binding table: {e}"))?;
    std::fs::write(&path, text + "\n").map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DrtConfig {
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

impl DrtConfig {
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(".tracelean").join("drt.json")
    }

    pub fn load(root: &Path) -> Result<DrtConfig, String> {
        let path = Self::path_for(root);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(DrtConfig::default());
        };
        serde_json::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))
    }

    pub fn save(&self, root: &Path) -> Result<PathBuf, String> {
        let path = Self::path_for(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serializing drt config: {e}"))?;
        std::fs::write(&path, text + "\n")
            .map_err(|e| format!("writing {}: {e}", path.display()))?;
        Ok(path)
    }

    pub fn binding(&self, req_id: &str, clause: Option<&str>) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|b| b.req_id == req_id && b.clause.as_deref() == clause)
            // A requirement-level binding also serves a clause that has none of
            // its own, so a single-clause requirement needs no duplication.
            .or_else(|| {
                self.bindings
                    .iter()
                    .find(|b| b.req_id == req_id && b.clause.is_none())
            })
    }
}

/// A starting-point binding, written for a requirement that has none.
///
/// Scaffolding matters more than it looks: writing the adapter is the one
/// piece of friction this approach imposes, so the tool should get the user as
/// close to running as it can.
pub fn scaffold_binding(req_id: &str, clause: Option<&str>, lang: &str) -> Binding {
    let implementation = CallSpec {
        language: lang.to_string(),
        entry: format!("path/to/implementation.{}::function", extension_for(lang)),
        params: BTreeMap::new(),
        convert: None,
    };

    Binding {
        req_id: req_id.to_string(),
        clause: clause.map(|c| c.to_string()),
        op: default_op(),
        model: RunnerSpec {
            cmd: vec![".tracelean/drt/.lake/build/bin/drtRunner".into()],
            cwd: None,
            env: BTreeMap::new(),
        },
        implementation,
        input: Schema::Struct {
            fields: BTreeMap::from([(
                "value".to_string(),
                Schema::Str { max_len: Some(32), examples: vec![] },
            )]),
        },
        coverage_floor: CoverageFloor::default(),
    }
}

fn extension_for(lang: &str) -> &'static str {
    match lang {
        "python" => "py",
        "rust" => "rs",
        _ => "txt",
    }
}
