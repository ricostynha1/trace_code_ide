use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// @implements REQ-DRT-SCHEMA.whitelisted
/// @implements REQ-DRT-SCHEMA.declared_not_reflected
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Schema {
    Nat {
        #[serde(default)]
        max: Option<u64>,
        /// @implements REQ-DRT-GEN.edges_sampled
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    Tuple {
        items: Vec<Schema>,
    },
    /// @implements REQ-DRT-SCHEMA.json_shape_matches
    Enum {
        variants: BTreeMap<String, Option<Box<Schema>>>,
    },
}

impl Schema {
    pub fn kind(&self) -> &'static str {
        match self {
            Schema::Nat { .. } => "nat",
            Schema::Int { .. } => "int",
            Schema::Bool => "bool",
            Schema::Str { .. } => "str",
            Schema::Option { .. } => "option",
            Schema::List { .. } => "list",
            Schema::Struct { .. } => "struct",
            Schema::Tuple { .. } => "tuple",
            Schema::Enum { .. } => "enum",
        }
    }

    pub fn simple_enum(names: &[&str]) -> Schema {
        Schema::Enum {
            variants: names.iter().map(|n| (n.to_string(), None)).collect(),
        }
    }
}

#[cfg(test)]
mod zz_probe {
    use super::*;
    #[test]
    fn zz_print() {
        let s = Schema::List {
            inner: Box::new(Schema::Str { max_len: Some(2), examples: vec!["a".into()] }),
            max_len: Some(3),
        };
        println!("{}", serde_json::to_string(&s).unwrap());
        let mut v = BTreeMap::new();
        v.insert("a".to_string(), None);
        v.insert("b".to_string(), Some(Box::new(Schema::Bool)));
        println!("{}", serde_json::to_string(&Schema::Enum { variants: v }).unwrap());
        let mut f = BTreeMap::new();
        f.insert("x".to_string(), Schema::Nat { max: Some(4), edges: vec![0, 4] });
        println!("{}", serde_json::to_string(&Schema::Struct { fields: f }).unwrap());
        println!("{}", serde_json::to_string(&Schema::Tuple { items: vec![Schema::Bool] }).unwrap());
        println!("{}", serde_json::to_string(&Schema::Option { inner: Box::new(Schema::Bool) }).unwrap());
        println!("{}", serde_json::to_string(&Schema::Int { min: None, max: Some(3) }).unwrap());
    }
}
