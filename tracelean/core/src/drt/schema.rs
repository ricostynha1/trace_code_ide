//! The declared shape of a binding's input.
//!
//! Lean has no runtime type reflection: extracting a schema from the model
//! would take a `MetaM` metaprogram and is undecidable once type parameters or
//! dependent types appear. So the shape is *declared* in `.tracelean/drt.json`
//! over a small whitelisted grammar, and anything outside that grammar is a
//! clear error rather than a silently wrong generator.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The whitelisted type grammar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Schema {
    /// Non-negative integer. `max` bounds generation, not validity.
    Nat {
        #[serde(default)]
        max: Option<u64>,
        /// Extra boundary values to sample heavily, alongside the generator's
        /// own.
        ///
        /// Without these a model that branches at 5000 and 20000 is tested
        /// almost entirely above both: an unbounded `Nat` draws uniformly from
        /// the whole range, so the low bands are visited a handful of times in
        /// a run of thousands and the coverage floor still reports "met". A
        /// run that never reached a branch has not tested it, and saying
        /// otherwise is the failure this project exists to prevent.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edges: Vec<u64>,
    },
    Int {
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
    },
    Bool,
    Str {
        #[serde(default)]
        max_len: Option<usize>,
        /// Optional pool of interesting values to draw from, in addition to
        /// generated ones.
        #[serde(default)]
        examples: Vec<String>,
    },
    Option {
        inner: Box<Schema>,
    },
    List {
        inner: Box<Schema>,
        #[serde(default)]
        max_len: Option<usize>,
    },
    Struct {
        fields: BTreeMap<String, Schema>,
    },
    /// A sum type. Each variant carries an optional payload, matching how Lean
    /// derives `ToJson`: a nullary constructor is the bare string
    /// `"weakPassword"`, a payload-carrying one is `{"C": payload}`.
    Enum {
        variants: BTreeMap<String, Option<Schema>>,
    },
}

impl Schema {
    /// Names of the constructors this schema can produce, for coverage
    /// accounting. Structures and scalars have exactly one.
    pub fn constructors(&self) -> Vec<String> {
        match self {
            Schema::Enum { variants } => variants.keys().cloned().collect(),
            Schema::Option { .. } => vec!["some".into(), "none".into()],
            Schema::Bool => vec!["true".into(), "false".into()],
            Schema::List { .. } => vec!["empty".into(), "nonempty".into()],
            _ => vec!["value".into()],
        }
    }

    /// Which constructor a concrete value took — the input half of coverage.
    pub fn constructor_of(&self, value: &serde_json::Value) -> Option<String> {
        use serde_json::Value;
        match (self, value) {
            (Schema::Enum { .. }, Value::String(s)) => Some(s.clone()),
            (Schema::Enum { .. }, Value::Object(map)) => map.keys().next().cloned(),
            (Schema::Option { .. }, Value::Null) => Some("none".into()),
            (Schema::Option { .. }, _) => Some("some".into()),
            (Schema::Bool, Value::Bool(b)) => Some(b.to_string()),
            (Schema::List { .. }, Value::Array(items)) => {
                Some(if items.is_empty() { "empty".into() } else { "nonempty".into() })
            }
            _ => Some("value".into()),
        }
    }
}

/// The output half of coverage: which variant a reply took.
///
/// Derived from the shape of the observed value rather than a declared output
/// schema, because the interesting question is only "have we seen every shape
/// the model can produce", and requiring users to declare the output type as
/// well would double the setup cost for no extra signal.
pub fn output_variant(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::String(s) => s.clone(),
        Value::Object(map) => map.keys().next().cloned().unwrap_or_else(|| "object".into()),
        Value::Array(_) => "array".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(_) => "number".into(),
        Value::Null => "null".into(),
    }
}
