//! `.tracelean/drt.json` — how a clause's model and implementation are driven.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The op that selects one binding's entry point in the shared runner.
///
/// Qualified by requirement and clause so that collisions are impossible: a
/// shared runner dispatching on a name two bindings both use answers one of
/// them with the other's function, and nothing reports it.
///
/// @implements REQ-DRT-PROTO.ops_unique
pub fn qualified_op(req_id: &str, clause: Option<&str>) -> String {
    match clause {
        Some(c) => format!("{req_id}.{c}"),
        None => req_id.to_string(),
    }
}

/// The languages an implementation may be written in.
///
/// A closed list rather than a free string: a runner has to be generated for
/// each, so a language nothing can generate a runner for is a binding that
/// cannot run.
///
/// @implements REQ-DRT-BIND.call_only
pub const LANGUAGES: &[&str] = &["rust", "typescript"];

/// How the implementation side is driven: a function to call, named the way the
/// project spells it.
///
/// There is deliberately no adapter form. An adapter is an untrusted
/// participant in the comparison it exists to enable — arbitrary code between
/// the implementation and the comparator can make a divergence disappear.
///
/// @implements REQ-DRT-BIND.no_adapter
/// @implements REQ-DRT-BIND.call_only
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallSpec {
    /// `"rust"` or `"python"`.
    pub language: String,
    /// `path/to/file.rs::symbol`, relative to the project root. A path rather
    /// than a module name because this project has no import conventions.
    pub entry: String,
    /// Model field name -> implementation parameter name, for the fields where
    /// the two disagree. Renames only; it may not compute a value.
    ///
    /// @implements REQ-DRT-BIND.rename_only
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl CallSpec {
    /// Split `path::symbol`.
    pub fn split_entry(&self) -> Option<(&str, &str)> {
        self.entry.split_once("::")
    }
}

/// How the model side is driven: a module to import, a function to call, and
/// the field names its arguments are taken from, in order.
///
/// The same shape as `CallSpec` and deliberately not the same type: an
/// implementation is named by a path into the tree because this project has no
/// import conventions, and a model is named by the module system its language
/// does have. Collapsing the two would mean one of them lying about how it is
/// reached.
///
/// This existed in `.tracelean/drt.json` long before it existed here — the key
/// was in the file and had no field to land in, so the only reader was a
/// private parse in the test harness and nothing in Rust checked the model half
/// of a binding at all. It lands here so both halves get the same treatment.
///
/// @implements REQ-DRT-BIND.call_only
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    /// The module a runner imports to reach the function, as its language
    /// spells it.
    pub import: String,
    /// The function, fully qualified.
    pub function: String,
    /// The fields of the generated input, in the order the function takes them.
    pub arguments: Vec<String>,
}

/// One clause bound to a model runner and an implementation runner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub req_id: String,
    #[serde(default)]
    pub clause: Option<String>,
    #[serde(default)]
    pub op: Option<String>,
    /// Other clauses of the same requirement that this one call checks.
    ///
    /// One function often realises several clauses at once, and running the
    /// same comparison once per clause would be the same evidence four times.
    /// Listing them is a claim that a disagreement about the named clause would
    /// show up in this call's output — not that the clause is mentioned
    /// nearby.
    ///
    /// @implements REQ-DRT-BIND.binding_is_the_bond
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_checks: Vec<String>,
    /// How the model side is driven.
    ///
    /// Optional because a binding may be built in memory by a test that is
    /// checking the implementation side alone, not because a binding in the
    /// file may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSpec>,
    pub implementation: CallSpec,
    /// Further implementations of the same clause, each compared against the
    /// same model.
    ///
    /// A second frontend is a second implementation of the same functions, and
    /// the question that matters about it is whether it answers what the model
    /// answers. Comparing it to the *model* rather than to the first
    /// implementation is what keeps one oracle: two implementations checked
    /// against each other agree perfectly when both are wrong.
    ///
    /// @implements REQ-DRT-BIND.binding_is_the_bond
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_implemented_by: Vec<CallSpec>,
    /// The coverage this binding's runs must reach.
    ///
    /// Here rather than in the suite that measures it, because a floor written
    /// beside the code that counts is a number nobody outside that file can
    /// read, argue with, or diff. A situation is still a predicate the suite
    /// owns — a generated value either reached it or did not, and no JSON can
    /// say that — so the binding names the situation and the minimum, and the
    /// suite reports how often the name was reached.
    ///
    /// Empty means *undeclared*, which is not a floor of zero: one is a claim
    /// somebody made and the other is the absence of one, and `coverage::level`
    /// refuses L3 to both.
    ///
    /// @implements REQ-DRT-COVER.floor_stated
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floors: Vec<crate::drt::coverage::Floor>,
    /// Classes or lines this binding's runs are excused from reaching, each
    /// with its reason (ADR-0017).
    ///
    /// Every class of the arguments and every line of the implementing item is
    /// a floor whether or not `floors` names it; this is the one place a
    /// person says one of them cannot be reached and why. It sits beside the
    /// floors for the same reason they are here: where it can be read, argued
    /// with and diffed. A waiver that excuses nothing is reported.
    ///
    /// @implements REQ-DRT-COVER.waiver_reasoned
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waive: Vec<crate::drt::coverage::Waiver>,
}

impl Binding {
    /// Every clause this binding establishes, as `REQ.clause`.
    pub fn clauses(&self) -> Vec<String> {
        let mut out = vec![qualified_op(&self.req_id, self.clause.as_deref())];
        // A bare name is a clause of this binding's own requirement; a
        // qualified one names a clause anywhere. One call can establish clauses
        // of several requirements — the sandbox's copy law is simultaneously
        // about observation and about containment — and forcing that into one
        // requirement would be filing by mechanism rather than by claim.
        out.extend(self.also_checks.iter().map(|c| {
            if c.contains('.') {
                c.clone()
            } else {
                format!("{}.{c}", self.req_id)
            }
        }));
        out
    }

    /// Every implementation this clause has, the first one first.
    ///
    /// @implements REQ-DRT-BIND.binding_is_the_bond
    pub fn implementations(&self) -> Vec<&CallSpec> {
        let mut out = vec![&self.implementation];
        out.extend(self.also_implemented_by.iter());
        out
    }

    pub fn op(&self) -> String {
        self.op
            .clone()
            .unwrap_or_else(|| qualified_op(&self.req_id, self.clause.as_deref()))
    }
}

/// Where a project keeps its binding file.
pub fn path_in(root: &Path) -> PathBuf {
    root.join(".tracelean").join("drt.json")
}

/// Read the binding file.
///
/// The one place that turns those bytes into bindings, so that a reader and a
/// runner are looking at the same declarations. A project without one has no
/// bindings, which is a state worth being able to say out loud rather than an
/// error — so the empty case is `Ok(vec![])` and only unreadable JSON is an
/// error.
pub fn read(root: &Path) -> Result<Vec<Binding>, String> {
    let path = path_in(root);
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("not readable JSON: {e}"))?;
    let Some(array) = value.get("bindings") else { return Ok(Vec::new()) };
    serde_json::from_value(array.clone()).map_err(|e| format!("not a binding list: {e}"))
}
