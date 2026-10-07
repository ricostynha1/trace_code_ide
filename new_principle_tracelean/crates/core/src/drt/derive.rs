//! Deriving a differential test from the two signatures it is between.
//!
//! A clause with a Lean model (`@models`, a `def`) and a Rust implementation
//! (`@implements`, a `fn`) can be tested without anybody writing a binding: the
//! arguments pair by position, and each pair of types gives the generator its
//! schema. Only types both sides plainly agree on are paired — integers,
//! naturals, booleans, strings, lists, options and structures whose fields do
//! too. Anything else is reported as the mismatch it is, never approximated:
//! a guessed schema is a test of something nobody wrote.
//!
//! Pure: the shell reads the declarations and hands their text in.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::schema::Schema;

/// A type, as far as pairing needs to know it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Ty {
    /// A signed integer of `bits` bits; 0 is Lean's unbounded `Int`.
    Int { bits: u32 },
    /// A natural number of `bits` bits; 0 is Lean's unbounded `Nat`.
    Nat { bits: u32 },
    Bool,
    Str,
    Float,
    List { inner: Box<Ty> },
    Option { inner: Box<Ty> },
    /// A structure, by name, looked up in the side's own declarations.
    Named { name: String },
    /// Anything else, as written.
    Other { text: String },
}

/// A structure's fields as JSON spells them, in order.
pub type Structs = Vec<(String, Vec<(String, Ty)>)>;

/// The schema a pair of types generates, or why there is none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Paired {
    Ok(Schema),
    Err(String),
}

/// How a type reads in a report.
pub fn show(ty: &Ty) -> String {
    match ty {
        Ty::Int { bits: 0 } => "Int".into(),
        Ty::Int { bits } => format!("i{bits}"),
        Ty::Nat { bits: 0 } => "Nat".into(),
        Ty::Nat { bits } => format!("u{bits}"),
        Ty::Bool => "Bool".into(),
        Ty::Str => "String".into(),
        Ty::Float => "Float".into(),
        Ty::List { inner } => format!("List ({})", show(inner)),
        Ty::Option { inner } => format!("Option ({})", show(inner)),
        Ty::Named { name } => name.clone(),
        Ty::Other { text } => text.clone(),
    }
}

/// Generation bounds: small enough that the arithmetic in a conversion cannot
/// overflow the narrower side and that zero is drawn often, wide enough to
/// cross the boundaries rounding cares about.
fn int_bound(bits: u32) -> i64 {
    if bits == 8 {
        10
    } else {
        100
    }
}

fn nat_bound(bits: u32) -> u64 {
    if bits == 8 {
        15
    } else {
        1000
    }
}

fn fields_of<'a>(structs: &'a Structs, name: &str) -> Option<&'a Vec<(String, Ty)>> {
    structs.iter().find(|(n, _)| n == name).map(|(_, f)| f)
}

/// The schema for a Lean type against a Rust type.
///
/// `fuel` bounds the walk through structures, which may refer to each other.
///
/// @implements REQ-DRT-SCHEMA.derived_from_both
/// @implements REQ-DRT-SCHEMA.outside_is_error
/// @drt REQ-DRT-SCHEMA.derived_from_both
/// @drt REQ-DRT-SCHEMA.outside_is_error
pub fn pair(lean: Ty, rust: Ty, lean_structs: Structs, rust_structs: Structs, fuel: u32) -> Paired {
    let mismatch = || Paired::Err(format!("Lean `{}` against Rust `{}`", show(&lean), show(&rust)));
    if fuel == 0 {
        return Paired::Err(format!("`{}` nests too deeply to generate", show(&lean)));
    }
    match (&lean, &rust) {
        (Ty::Float, _) | (_, Ty::Float) => {
            Paired::Err("Float has no exact comparison; test it with a binding of your own".into())
        }
        (Ty::Int { bits: 0 }, Ty::Int { bits }) => {
            let b = int_bound(*bits);
            Paired::Ok(Schema::Int { min: Some(-b), max: Some(b) })
        }
        (Ty::Nat { bits: 0 }, Ty::Nat { bits }) | (Ty::Nat { bits: 0 }, Ty::Int { bits }) => {
            Paired::Ok(Schema::Nat { max: Some(nat_bound(*bits)), edges: vec![0, 1] })
        }
        (Ty::Bool, Ty::Bool) => Paired::Ok(Schema::Bool),
        (Ty::Str, Ty::Str) => Paired::Ok(Schema::Str { max_len: Some(8), examples: vec![String::new()] }),
        (Ty::List { inner: a }, Ty::List { inner: b }) => {
            match pair((**a).clone(), (**b).clone(), lean_structs, rust_structs, fuel - 1) {
                Paired::Ok(inner) => Paired::Ok(Schema::List { inner: Box::new(inner), max_len: Some(5) }),
                err => err,
            }
        }
        (Ty::Option { inner: a }, Ty::Option { inner: b }) => {
            match pair((**a).clone(), (**b).clone(), lean_structs, rust_structs, fuel - 1) {
                Paired::Ok(inner) => Paired::Ok(Schema::Option { inner: Box::new(inner) }),
                err => err,
            }
        }
        (Ty::Named { name: a }, Ty::Named { name: b }) => {
            let (Some(theirs), Some(ours)) = (fields_of(&lean_structs, a), fields_of(&rust_structs, b)) else {
                return Paired::Err(format!("no structure `{a}` in the model, or `{b}` in the code, to pair"));
            };
            if theirs.len() != ours.len() {
                return Paired::Err(format!("`{a}` has {} fields and `{b}` {}", theirs.len(), ours.len()));
            }
            let mut fields = BTreeMap::new();
            for (field, ty) in theirs {
                let Some((_, other)) = ours.iter().find(|(f, _)| f == field) else {
                    return Paired::Err(format!("`{b}` has no field `{field}` as JSON spells it"));
                };
                match pair(ty.clone(), other.clone(), lean_structs.clone(), rust_structs.clone(), fuel - 1) {
                    Paired::Ok(schema) => {
                        fields.insert(field.clone(), schema);
                    }
                    Paired::Err(why) => return Paired::Err(format!("{a}.{field}: {why}")),
                }
            }
            Paired::Ok(Schema::Struct { fields })
        }
        _ => mismatch(),
    }
}

/// Split at commas not inside brackets.
fn top_level(text: &str, separator: char) -> Vec<String> {
    let (mut depth, mut part, mut out) = (0i32, String::new(), Vec::new());
    for c in text.chars() {
        match c {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            _ => {}
        }
        if c == separator && depth == 0 {
            out.push(std::mem::take(&mut part));
        } else {
            part.push(c);
        }
    }
    if !part.trim().is_empty() {
        out.push(part);
    }
    out
}

/// A Lean type as written.
pub fn lean_type(text: &str) -> Ty {
    let text = text.trim();
    let text = text.strip_prefix('(').and_then(|t| t.strip_suffix(')')).unwrap_or(text).trim();
    match text {
        "Int" => return Ty::Int { bits: 0 },
        "Nat" => return Ty::Nat { bits: 0 },
        "Bool" => return Ty::Bool,
        "String" => return Ty::Str,
        "Float" => return Ty::Float,
        _ => {}
    }
    for (head, list) in [("List ", true), ("Array ", true), ("Option ", false)] {
        if let Some(rest) = text.strip_prefix(head) {
            let inner = Box::new(lean_type(rest));
            return if list { Ty::List { inner } } else { Ty::Option { inner } };
        }
    }
    if text.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '_') && !text.is_empty() {
        return Ty::Named { name: text.rsplit('.').next().unwrap_or(text).to_string() };
    }
    Ty::Other { text: text.to_string() }
}

/// A Rust type as written.
pub fn rust_type(text: &str) -> Ty {
    let text = text.trim();
    if let Some(bits) = ["i8", "i16", "i32", "i64", "i128", "isize"].iter().position(|t| *t == text) {
        return Ty::Int { bits: [8, 16, 32, 64, 128, 64][bits] };
    }
    if let Some(bits) = ["u8", "u16", "u32", "u64", "u128", "usize"].iter().position(|t| *t == text) {
        return Ty::Nat { bits: [8, 16, 32, 64, 128, 64][bits] };
    }
    match text {
        "bool" => return Ty::Bool,
        "String" => return Ty::Str,
        "f32" | "f64" => return Ty::Float,
        _ => {}
    }
    for (head, list) in [("Vec<", true), ("Option<", false)] {
        if let Some(rest) = text.strip_prefix(head).and_then(|r| r.strip_suffix('>')) {
            let inner = Box::new(rust_type(rest));
            return if list { Ty::List { inner } } else { Ty::Option { inner } };
        }
    }
    if text.chars().all(|c| c.is_alphanumeric() || c == ':' || c == '_') && !text.is_empty() {
        return Ty::Named { name: text.rsplit("::").next().unwrap_or(text).to_string() };
    }
    Ty::Other { text: text.to_string() }
}

/// A Lean `def`'s explicit arguments and result, from its text.
pub fn lean_signature(declaration: &str) -> Option<(Vec<(String, Ty)>, Ty)> {
    let at = declaration.find("def ")?;
    let header = &declaration[at + 4..];
    let header = &header[..header.find(":=").unwrap_or(header.len())];
    let (mut depth, mut group, mut params, mut result) = (0i32, String::new(), Vec::new(), None);
    for (i, c) in header.char_indices() {
        match c {
            '(' | '{' | '[' => {
                if depth == 0 {
                    group.clear();
                }
                depth += 1;
                if depth > 1 {
                    group.push(c);
                }
            }
            ')' | '}' | ']' => {
                depth -= 1;
                if depth == 0 && c == ')' {
                    let (names, ty) = group.split_once(':')?;
                    for name in names.split_whitespace() {
                        params.push((name.to_string(), lean_type(ty)));
                    }
                } else if depth > 0 {
                    group.push(c);
                }
            }
            ':' if depth == 0 => {
                result = Some(lean_type(&header[i + 1..]));
                break;
            }
            _ if depth > 0 => group.push(c),
            _ => {}
        }
    }
    Some((params, result?))
}

/// A Rust `fn`'s parameters and result, from the source holding it.
pub fn rust_signature(source: &str, symbol: &str) -> Option<(Vec<(String, Ty)>, Ty)> {
    let at = source.find(&format!("fn {symbol}("))?;
    let rest = &source[at + symbol.len() + 4..];
    let (mut depth, mut close) = (1i32, None);
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let mut params = Vec::new();
    for param in top_level(&rest[..close], ',') {
        let (name, ty) = param.split_once(':')?;
        let name = name.trim().trim_start_matches("mut ").to_string();
        params.push((name, rust_type(ty)));
    }
    let after = &rest[close + 1..];
    let body = after.find(['{', ';']).unwrap_or(after.len());
    let returns = after[..body].split(" where ").next().unwrap_or("").trim();
    let result = match returns.strip_prefix("->") {
        Some(ty) => rust_type(ty),
        None => Ty::Other { text: "()".into() },
    };
    Some((params, result))
}

/// `snake_case` as `camelCase`.
fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Every `structure` in Lean text that derives its JSON encoding, with fields.
pub fn lean_structs(text: &str) -> Structs {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (n, line) in lines.iter().enumerate() {
        let Some(rest) = line.trim_start().strip_prefix("structure ") else { continue };
        let name = rest.split_whitespace().next().unwrap_or("").to_string();
        let mut fields = Vec::new();
        let mut json = false;
        for field in &lines[n + 1..] {
            let trimmed = field.trim();
            if trimmed.starts_with("deriving") {
                json = trimmed.contains("ToJson") && trimmed.contains("FromJson");
                break;
            }
            if !field.starts_with(' ') || trimmed.is_empty() {
                break;
            }
            if let Some((names, ty)) = trimmed.split_once(" : ") {
                for name in names.split_whitespace() {
                    fields.push((name.to_string(), lean_type(ty)));
                }
            }
        }
        if json {
            out.push((name, fields));
        }
    }
    out
}

/// Every `struct` in Rust source that derives `Serialize` and `Deserialize`,
/// with its fields named as serde spells them.
pub fn rust_structs(source: &str) -> Structs {
    let mut out = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("pub struct ").or_else(|| trimmed.strip_prefix("struct ")) else {
            continue;
        };
        let name = rest.split(|c: char| !c.is_alphanumeric() && c != '_').next().unwrap_or("").to_string();
        let attributes: String = lines[..n].iter().rev().take_while(|l| l.trim_start().starts_with("#[") || l.trim_start().starts_with("///")).copied().collect();
        if !(attributes.contains("Serialize") && attributes.contains("Deserialize")) {
            continue;
        }
        let camel_case = attributes.contains("rename_all = \"camelCase\"");
        let mut fields = Vec::new();
        for field in &lines[n + 1..] {
            let f = field.trim();
            if f.starts_with('}') {
                break;
            }
            if f.starts_with("//") || f.starts_with("#[") || f.is_empty() {
                continue;
            }
            let f = f.trim_start_matches("pub ").trim_end_matches(',');
            if let Some((field, ty)) = f.split_once(':') {
                let field = field.trim();
                fields.push((if camel_case { camel(field) } else { field.to_string() }, rust_type(ty)));
            }
        }
        out.push((name, fields));
    }
    out
}

/// The situations a derived test must reach, as its floors: each argument in
/// each of its classes — below, at and above zero; empty and not; absent and
/// present; both truths.
pub fn situations(schema: &Schema) -> Vec<String> {
    let Schema::Struct { fields } = schema else { return Vec::new() };
    let mut out = Vec::new();
    for (name, field) in fields {
        let classes: &[&str] = match field {
            Schema::Int { .. } => &["negative", "zero", "positive"],
            Schema::Nat { .. } => &["zero", "positive"],
            Schema::Bool => &["true", "false"],
            Schema::Str { .. } | Schema::List { .. } => &["empty", "not empty"],
            Schema::Option { .. } => &["absent", "present"],
            _ => &[],
        };
        out.extend(classes.iter().map(|class| format!("{name} {class}")));
    }
    out
}

/// Which of `situations` one generated input is in.
pub fn situation_of(schema: &Schema, input: &serde_json::Value) -> Vec<String> {
    let Schema::Struct { fields } = schema else { return Vec::new() };
    let mut out = Vec::new();
    for (name, field) in fields {
        let value = &input[name];
        let class = match field {
            Schema::Int { .. } | Schema::Nat { .. } => match value.as_i64() {
                Some(n) if n < 0 => "negative",
                Some(0) => "zero",
                Some(_) => "positive",
                None => continue,
            },
            Schema::Bool => if value.as_bool() == Some(true) { "true" } else { "false" },
            Schema::Str { .. } => if value.as_str().is_some_and(str::is_empty) { "empty" } else { "not empty" },
            Schema::List { .. } => if value.as_array().is_some_and(Vec::is_empty) { "empty" } else { "not empty" },
            Schema::Option { .. } => if value.is_null() { "absent" } else { "present" },
            _ => continue,
        };
        out.push(format!("{name} {class}"));
    }
    out
}

/// What a derived test needs: the schema of its input, the model's argument
/// names in order, and which Rust parameter reads which of them.
#[derive(Debug, Clone, PartialEq)]
pub struct Derived {
    pub schema: Schema,
    pub arguments: Vec<String>,
    /// Model argument name -> Rust parameter name, where they differ.
    pub params: BTreeMap<String, String>,
}

/// Pair a model with an implementation, or say every reason they cannot be.
pub fn derive(
    model: (Vec<(String, Ty)>, Ty),
    implementation: (Vec<(String, Ty)>, Ty),
    lean: &Structs,
    rust: &Structs,
) -> Result<Derived, Vec<String>> {
    let (theirs, their_result) = model;
    let (ours, our_result) = implementation;
    let mut problems = Vec::new();
    if theirs.len() != ours.len() {
        problems.push(format!("the model takes {} arguments and the code {}", theirs.len(), ours.len()));
        return Err(problems);
    }
    let mut fields = BTreeMap::new();
    let mut params = BTreeMap::new();
    for ((name, ty), (param, other)) in theirs.iter().zip(&ours) {
        match pair(ty.clone(), other.clone(), lean.clone(), rust.clone(), 6) {
            Paired::Ok(schema) => {
                fields.insert(name.clone(), schema);
            }
            Paired::Err(why) => problems.push(format!("argument `{name}`: {why}")),
        }
        if name != param {
            params.insert(name.clone(), param.clone());
        }
    }
    if let Paired::Err(why) = pair(their_result, our_result, lean.clone(), rust.clone(), 6) {
        problems.push(format!("result: {why}"));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Derived { schema: Schema::Struct { fields }, arguments: theirs.into_iter().map(|(n, _)| n).collect(), params })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_signatures_are_read() {
        let lean = lean_signature("/-- doc -/\ndef toF (c : Int) (xs ys : List Nat) : Option Int := none").unwrap();
        assert_eq!(lean.0[0], ("c".to_string(), Ty::Int { bits: 0 }));
        assert_eq!(lean.0[2], ("ys".to_string(), Ty::List { inner: Box::new(Ty::Nat { bits: 0 }) }));
        assert_eq!(lean.1, Ty::Option { inner: Box::new(Ty::Int { bits: 0 }) });
        let rust = rust_signature("/// x\npub fn to_f(degrees: i64, mut v: Vec<u32>) -> Option<i64> {\n}", "to_f").unwrap();
        assert_eq!(rust.0[0], ("degrees".to_string(), Ty::Int { bits: 64 }));
        assert_eq!(rust.0[1], ("v".to_string(), Ty::List { inner: Box::new(Ty::Nat { bits: 32 }) }));
        assert_eq!(rust.1, Ty::Option { inner: Box::new(Ty::Int { bits: 64 }) });
    }

    #[test]
    fn matching_signatures_give_a_schema_and_the_parameter_names() {
        let derived = derive(
            lean_signature("def toF (c : Int) : Int := c").unwrap(),
            rust_signature("fn to_f(degrees: i64) -> i64 { degrees }", "to_f").unwrap(),
            &vec![],
            &vec![],
        )
        .unwrap();
        assert_eq!(derived.arguments, vec!["c".to_string()]);
        assert_eq!(derived.params.get("c").map(String::as_str), Some("degrees"));
        assert!(matches!(&derived.schema, Schema::Struct { fields } if matches!(fields.get("c"), Some(Schema::Int { .. }))));
    }

    #[test]
    fn a_mismatch_is_reported_not_guessed() {
        let problems = derive(
            lean_signature("def toF (c : Int) : Int := c").unwrap(),
            rust_signature("fn to_f(degrees: f64) -> u8 { 0 }", "to_f").unwrap(),
            &vec![],
            &vec![],
        )
        .unwrap_err();
        assert!(problems[0].contains("Float"), "{problems:?}");
        assert!(problems[1].contains("Lean `Int` against Rust `u8`"), "{problems:?}");
    }

    #[test]
    fn structures_pair_by_their_json_field_names() {
        let lean = lean_structs("structure Point where\n  xPos : Int\n  label : String\n  deriving Repr, ToJson, FromJson\n");
        let rust = rust_structs(
            "#[derive(Serialize, Deserialize)]\n#[serde(rename_all = \"camelCase\")]\npub struct Point {\n    pub x_pos: i64,\n    pub label: String,\n}\n",
        );
        let paired = pair(Ty::Named { name: "Point".into() }, Ty::Named { name: "Point".into() }, lean.clone(), rust, 6);
        assert!(matches!(paired, Paired::Ok(Schema::Struct { .. })), "{paired:?}");
        let plain = rust_structs("#[derive(Serialize, Deserialize)]\nstruct Point {\n    x_pos: i64,\n    label: String,\n}\n");
        let paired = pair(Ty::Named { name: "Point".into() }, Ty::Named { name: "Point".into() }, lean, plain, 6);
        assert!(matches!(&paired, Paired::Err(why) if why.contains("xPos")), "{paired:?}");
    }
}
