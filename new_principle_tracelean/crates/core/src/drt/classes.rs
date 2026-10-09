//! The classes of a binding's arguments, derived from their declared shapes.
//!
//! A hand-written list of situations can always miss a case. The classes of a
//! shape cannot: below, at and above zero; empty and not; absent and present;
//! each truth; each enum case; and the same, recursively, for every field,
//! element and payload. Every differential run must reach all of them
//! (`REQ-DRT-COVER.classes_reached`); a binding's named situations are counted
//! on top, never instead.
//!
//! Only classes the declared bounds can produce are derived — a `Nat` capped at
//! zero has no positive class — so a class not reached is the generator's
//! fault, never the schema's. Pure.

use std::collections::BTreeSet;

use serde_json::Value;

use super::coverage::Observed;
use super::gen;
use super::schema::Schema;

/// A class's name: the path to the value, then what it is.
fn name(path: &str, label: &str) -> String {
    if path.is_empty() {
        label.to_string()
    } else {
        format!("{path} {label}")
    }
}

/// The path to a member of the value at `path`.
fn member(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Every class a value of `schema` at `path` can fall in.
fn classes_at(path: &str, schema: &Schema, out: &mut Vec<String>) {
    match schema {
        Schema::Nat { max, edges } => {
            out.push(name(path, "zero"));
            if max.unwrap_or(1_000) > 0 || edges.iter().any(|e| *e > 0) {
                out.push(name(path, "positive"));
            }
        }
        Schema::Int { min, max } => {
            let lo = i128::from(min.unwrap_or(-1_000));
            let top = lo + (i128::from(max.unwrap_or(1_000)) - lo).abs();
            if lo < 0 {
                out.push(name(path, "negative"));
            }
            if lo <= 0 && 0 <= top {
                out.push(name(path, "zero"));
            }
            if 0 < top {
                out.push(name(path, "positive"));
            }
        }
        Schema::Bool => {
            out.push(name(path, "true"));
            out.push(name(path, "false"));
        }
        Schema::Str { max_len, examples } => {
            out.push(name(path, "empty"));
            if max_len.unwrap_or(12) > 0 || examples.iter().any(|e| !e.is_empty()) {
                out.push(name(path, "not empty"));
            }
        }
        Schema::Option { inner } => {
            out.push(name(path, "absent"));
            out.push(name(path, "present"));
            classes_at(&format!("{path}?"), inner, out);
        }
        Schema::List { inner, max_len } => {
            out.push(name(path, "empty"));
            if max_len.unwrap_or(6) > 0 {
                out.push(name(path, "not empty"));
                classes_at(&format!("{path}[]"), inner, out);
            }
        }
        Schema::Struct { fields } => {
            for (key, field) in fields {
                classes_at(&member(path, key), field, out);
            }
        }
        Schema::Tuple { items } => {
            for (i, item) in items.iter().enumerate() {
                classes_at(&member(path, &i.to_string()), item, out);
            }
        }
        Schema::Enum { variants } => {
            for (variant, payload) in variants {
                out.push(name(path, &format!("is {variant}")));
                if let Some(payload) = payload {
                    classes_at(&member(path, variant), payload, out);
                }
            }
        }
    }
}

/// Every class of a schema, in the order they are derived.
pub fn classes(schema: &Schema) -> Vec<String> {
    let mut out = Vec::new();
    classes_at("", schema, &mut out);
    out
}

/// The class of a number, by its sign.
fn sign(path: &str, value: &Value) -> Option<String> {
    if let Some(n) = value.as_u64() {
        return Some(name(path, if n == 0 { "zero" } else { "positive" }));
    }
    let n = value.as_i64()?;
    Some(name(path, if n < 0 { "negative" } else if n == 0 { "zero" } else { "positive" }))
}

/// Which classes one value of `schema` at `path` is in.
fn classes_of_at(path: &str, schema: &Schema, value: &Value, out: &mut Vec<String>) {
    match schema {
        Schema::Nat { .. } | Schema::Int { .. } => out.extend(sign(path, value)),
        Schema::Bool => out.push(name(path, if value.as_bool() == Some(true) { "true" } else { "false" })),
        Schema::Str { .. } => {
            out.push(name(path, if value.as_str() == Some("") { "empty" } else { "not empty" }))
        }
        Schema::Option { inner } => {
            if value.is_null() {
                out.push(name(path, "absent"));
            } else {
                out.push(name(path, "present"));
                classes_of_at(&format!("{path}?"), inner, value, out);
            }
        }
        Schema::List { inner, .. } => match value.as_array() {
            Some(items) if items.is_empty() => out.push(name(path, "empty")),
            Some(items) => {
                out.push(name(path, "not empty"));
                for item in items {
                    classes_of_at(&format!("{path}[]"), inner, item, out);
                }
            }
            None => out.push(name(path, "not empty")),
        },
        Schema::Struct { fields } => {
            for (key, field) in fields {
                classes_of_at(&member(path, key), field, value.get(key).unwrap_or(&Value::Null), out);
            }
        }
        Schema::Tuple { items } => {
            for (i, item) in items.iter().enumerate() {
                classes_of_at(&member(path, &i.to_string()), item, value.get(i).unwrap_or(&Value::Null), out);
            }
        }
        Schema::Enum { variants } => match value {
            Value::String(variant) => out.push(name(path, &format!("is {variant}"))),
            Value::Object(map) => {
                let Some((variant, payload)) = map.iter().next() else { return };
                out.push(name(path, &format!("is {variant}")));
                if let Some(Some(schema)) = variants.get(variant) {
                    classes_of_at(&member(path, variant), schema, payload, out);
                }
            }
            _ => {}
        },
    }
}

/// Which classes one value of `schema` is in.
pub fn classes_of(schema: &Schema, value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    classes_of_at("", schema, value, &mut out);
    out
}

/// How many of the first `cases` cases a seed draws reach each class of the
/// schema, in the order the classes are derived — the same stream a run with
/// that seed asks, so the count is of the cases actually asked.
///
pub fn reached(schema: Schema, seed: u64, cases: u64) -> Vec<Observed> {
    let hits: Vec<BTreeSet<String>> = gen::value_stream(seed, schema.clone(), cases)
        .iter()
        .map(|value| classes_of(&schema, value).into_iter().collect())
        .collect();
    classes(&schema)
        .into_iter()
        .map(|class| {
            let reached = hits.iter().filter(|h| h.contains(&class)).count() as u64;
            Observed { situation: class, reached }
        })
        .collect()
}

/// `reached` over the `shape`-th of the shapes a seeded stream is checked over
/// (`gen::SHAPES`, counted round): every class of every kind of schema, reached
/// or not, from the same stream a run draws.
///
/// @implements REQ-DRT-COVER.classes_reached
/// @drt REQ-DRT-COVER.classes_reached
pub fn reached_shape(shape: u64, seed: u64, cases: u64) -> Vec<Observed> {
    match gen::shape_at(shape) {
        Some(schema) => reached(schema, seed, cases),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn strukt(fields: &[(&str, Schema)]) -> Schema {
        Schema::Struct { fields: fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
    }

    /// @tests REQ-DRT-COVER.classes_reached
    #[test]
    fn classes_come_from_the_bounds_and_reach_into_payloads() {
        let mut variants: BTreeMap<String, Option<Box<Schema>>> = BTreeMap::new();
        variants.insert("met".into(), None);
        variants.insert("short".into(), Some(Box::new(strukt(&[("reached", Schema::Nat { max: Some(0), edges: vec![] })]))));
        let schema = strukt(&[
            ("n", Schema::Int { min: Some(0), max: Some(5) }),
            ("o", Schema::Option { inner: Box::new(Schema::Bool) }),
            ("v", Schema::Enum { variants }),
            ("xs", Schema::List { inner: Box::new(Schema::Str { max_len: Some(0), examples: vec![] }), max_len: Some(2) }),
        ]);
        assert_eq!(
            classes(&schema),
            [
                "n zero", "n positive", "o absent", "o present", "o? true", "o? false", "v is met",
                "v is short", "v.short.reached zero", "xs empty", "xs not empty", "xs[] empty",
            ]
        );
        let value = serde_json::json!({"n": 3, "o": null, "v": {"short": {"reached": 0}}, "xs": ["", ""]});
        assert_eq!(
            classes_of(&schema, &value),
            ["n positive", "o absent", "v is short", "v.short.reached zero", "xs not empty", "xs[] empty", "xs[] empty"]
        );
    }

    /// The case this exists for: a class the generator can produce only
    /// rarely, which a hand-named list would never have mentioned.
    ///
    /// @tests REQ-DRT-COVER.classes_reached
    #[test]
    fn a_class_the_generator_almost_never_draws_is_counted_as_missed() {
        let schema = strukt(&[("x", Schema::Int { min: Some(-1), max: Some(100_000) })]);
        let counts = reached(schema, 1, 50);
        let negative = counts.iter().find(|o| o.situation == "x negative").expect("derived");
        assert_eq!(negative.reached, 0, "{counts:?}");
        assert!(counts.iter().any(|o| o.situation == "x positive" && o.reached == 50));
    }
}
