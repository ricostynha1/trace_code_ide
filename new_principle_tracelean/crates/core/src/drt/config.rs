use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// @implements REQ-DRT-PROTO.ops_unique
pub fn qualified_op(req_id: &str, clause: Option<&str>) -> String {
    match clause {
        Some(c) => format!("{req_id}.{c}"),
        None => req_id.to_string(),
    }
}

/// @implements REQ-DRT-BIND.call_only
pub const LANGUAGES: &[&str] = &["rust", "typescript"];

/// @implements REQ-DRT-BIND.no_adapter
/// @implements REQ-DRT-BIND.call_only
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallSpec {
    pub language: String,
    pub entry: String,
    /// @implements REQ-DRT-BIND.rename_only
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
}

impl CallSpec {
    pub fn split_entry(&self) -> Option<(&str, &str)> {
        self.entry.split_once("::")
    }
}

/// @implements REQ-DRT-BIND.call_only
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub import: String,
    pub function: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub req_id: String,
    #[serde(default)]
    pub clause: Option<String>,
    #[serde(default)]
    pub op: Option<String>,
    /// @implements REQ-DRT-BIND.binding_is_the_bond
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSpec>,
    pub implementation: CallSpec,
    /// @implements REQ-DRT-BIND.binding_is_the_bond
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_implemented_by: Vec<CallSpec>,
    /// @implements REQ-DRT-COVER.floor_stated
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floors: Vec<crate::drt::coverage::Floor>,
    /// @implements REQ-DRT-COVER.waiver_reasoned
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waive: Vec<crate::drt::coverage::Waiver>,
}

impl Binding {
    pub fn clauses(&self) -> Vec<String> {
        let mut out = vec![qualified_op(&self.req_id, self.clause.as_deref())];
        out.extend(self.also_checks.iter().map(|c| {
            if c.contains('.') {
                c.clone()
            } else {
                format!("{}.{c}", self.req_id)
            }
        }));
        out
    }

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

pub fn path_in(root: &Path) -> PathBuf {
    root.join(".tracelean").join("drt.json")
}

pub fn read(root: &Path) -> Result<Vec<Binding>, String> {
    let path = path_in(root);
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(Vec::new()) };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("not readable JSON: {e}"))?;
    let Some(array) = value.get("bindings") else { return Ok(Vec::new()) };
    serde_json::from_value(array.clone()).map_err(|e| format!("not a binding list: {e}"))
}
