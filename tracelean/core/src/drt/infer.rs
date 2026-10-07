//! Proposing a differential-testing input schema from the Lean model's types.
//!
//! `.tracelean/drt.json` declares the shape of a binding's input because Lean
//! has no runtime type reflection we could ask (see `schema.rs`). Declaring it
//! *by hand*, though, was never the point — the types are right there in the
//! model, and re-typing them into JSON is exactly the kind of clerical step that
//! stops people binding a model at all. Which is how three clauses in the
//! example project ended up `Unbound`: a model and an implementation that
//! nobody checks agree.
//!
//! So this module reads the model source and proposes a schema. It is a
//! proposal, not an oracle: the result is shown before anything is written, and
//! a type outside the whitelisted grammar produces a sentence naming that type
//! rather than a guess. A wrong generator is worse than no generator, because a
//! passing differential run is evidence.

use std::collections::BTreeMap;

use super::schema::Schema;

/// Why a schema could not be proposed. Every variant names the thing that
/// blocked it, so the message is actionable.
#[derive(Debug, Clone, PartialEq)]
pub enum InferError {
    /// No `def`/`abbrev` with that name in the source.
    NoSuchDeclaration(String),
    /// The declaration takes no arguments, so there is nothing to generate.
    NoArguments(String),
    /// A binder type outside the whitelisted grammar.
    Unsupported { declaration: String, argument: String, ty: String },
    /// A named type that is not defined in the sources given.
    Unknown { ty: String },
}

impl std::fmt::Display for InferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferError::NoSuchDeclaration(name) => {
                write!(f, "no `def {name}` in the model source")
            }
            InferError::NoArguments(name) => write!(
                f,
                "`{name}` takes no arguments, so there is nothing to generate cases from"
            ),
            InferError::Unsupported { declaration, argument, ty } => write!(
                f,
                "cannot infer a schema: `{declaration}` takes `{argument} : {ty}`, and the \
                 generator has no rule for `{ty}`. Declare this binding's input by hand in \
                 .tracelean/drt.json, or simplify the model's signature."
            ),
            InferError::Unknown { ty } => write!(
                f,
                "`{ty}` is not defined in the model source that was scanned — if it lives in \
                 another file, include that file's `@models` link too"
            ),
        }
    }
}

impl std::error::Error for InferError {}

/// Structures and inductives found in some Lean source.
#[derive(Debug, Clone, Default)]
pub struct LeanTypes {
    /// `structure X where` → ordered `(field, type)`.
    pub structures: BTreeMap<String, Vec<(String, String)>>,
    /// `inductive X` → ordered `(constructor, payload type)`.
    pub inductives: BTreeMap<String, Vec<(String, Option<String>)>>,
}

/// Strip a trailing `deriving ...` clause and any comment from a line.
fn clean(line: &str) -> &str {
    let line = line.split("--").next().unwrap_or(line);
    line.trim_end()
}

/// Collect the structure and inductive declarations in a Lean source file.
///
/// A deliberately shallow parse over indentation, not a Lean front end. It
/// recognises the shapes a first-order, total reference model is written in and
/// declines everything else, which is the right trade: the alternative is a
/// metaprogram, and the failure mode of a half-right parser is a generator that
/// produces the wrong values while looking like it worked.
pub fn scan_types(source: &str) -> LeanTypes {
    let mut types = LeanTypes::default();
    let mut current: Option<(String, bool)> = None; // (name, is_structure)

    for raw in source.lines() {
        let line = clean(raw);
        let trimmed = line.trim_start();

        if let Some(rest) = trimmed.strip_prefix("structure ") {
            let name = rest.split_whitespace().next().unwrap_or("").to_string();
            if !name.is_empty() {
                types.structures.entry(name.clone()).or_default();
                current = Some((name, true));
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("inductive ") {
            let name = rest
                .split(|c: char| c.is_whitespace() || c == ':')
                .next()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                types.inductives.entry(name.clone()).or_default();
                current = Some((name, false));
            }
            // `inductive X where | a | b` on one line.
            if let Some(after) = rest.find('|') {
                for ctor in rest[after..].split('|') {
                    add_constructor(&mut types, current.as_ref(), ctor);
                }
            }
            continue;
        }

        // A non-indented, non-empty line ends the declaration we were in.
        if !trimmed.is_empty() && !raw.starts_with(' ') && !trimmed.starts_with('|') {
            current = None;
            continue;
        }
        let Some((name, is_structure)) = current.clone() else { continue };

        if is_structure {
            // `  field : Type`
            let Some((field, ty)) = trimmed.split_once(':') else { continue };
            let field = field.trim();
            if field.is_empty() || field.contains(' ') || trimmed.starts_with("deriving") {
                continue;
            }
            types
                .structures
                .entry(name)
                .or_default()
                .push((field.to_string(), ty.trim().to_string()));
        } else if let Some(ctor) = trimmed.strip_prefix('|') {
            add_constructor(&mut types, Some(&(name, false)), ctor);
        }
    }

    types
}

fn add_constructor(types: &mut LeanTypes, current: Option<&(String, bool)>, ctor: &str) {
    let Some((name, false)) = current.map(|(n, s)| (n.clone(), *s)).map(|(n, s)| (n, s)) else {
        return;
    };
    let ctor = ctor.trim();
    if ctor.is_empty() {
        return;
    }
    let (head, payload) = match ctor.split_once(':') {
        Some((h, p)) => (h.trim(), Some(p.trim().to_string())),
        None => (ctor, None),
    };
    let Some(head) = head.split_whitespace().next() else { return };
    types
        .inductives
        .entry(name)
        .or_default()
        .push((head.to_string(), payload));
}

/// `(name, type)` for each binder of a declaration.
///
/// Handles `(a b : T)` as two arguments of `T`, the way Lean reads it. Implicit
/// binders (`{...}`, `[...]`) are skipped: the caller does not supply them, so
/// they are not part of the input.
pub fn parse_binders(source: &str, declaration: &str) -> Option<Vec<(String, String)>> {
    for line in source.lines() {
        let trimmed = clean(line).trim_start();
        // `continue`, not `?`: a `?` here abandoned the whole search on the
        // first line that was not a definition, which in a real file is line 1.
        let Some(rest) = ["def ", "abbrev ", "partial def "]
            .iter()
            .find_map(|kw| trimmed.strip_prefix(kw))
        else {
            continue;
        };
        let end = rest
            .char_indices()
            .find(|(_, c)| c.is_whitespace() || *c == '(' || *c == ':' || *c == '{')
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        if rest[..end].trim() != declaration {
            continue;
        }

        let mut out = Vec::new();
        let mut remaining = &rest[end..];
        loop {
            let Some(open) = remaining.find(['(', '{', '[']) else { break };
            // A `:` before the next binder is the return type, and there are no
            // more arguments after it.
            if remaining[..open].contains(':') {
                break;
            }
            let opener = remaining.as_bytes()[open];
            let closer = match opener {
                b'(' => ')',
                b'{' => '}',
                _ => ']',
            };
            let Some(close) = remaining[open..].find(closer) else { break };
            let group = &remaining[open + 1..open + close];
            if opener == b'(' {
                if let Some((names, ty)) = group.split_once(':') {
                    for n in names.split_whitespace() {
                        out.push((n.to_string(), ty.trim().to_string()));
                    }
                }
            }
            remaining = &remaining[open + close + 1..];
        }
        return Some(out);
    }
    None
}

/// Every numeric literal in a model source, with its neighbours, as boundary
/// values for `Nat` generation.
///
/// A reference model is small and written on purpose: each literal in it is a
/// threshold somebody chose, and the case that matters is the one either side
/// of it. Without them an unbounded `Nat` is drawn from the whole 32-bit range,
/// so a model branching at 5000 and 20000 is exercised above both in nearly
/// every case -- thousands of cases, one band tested, and a coverage floor that
/// still reports "met".
///
/// Whole-file rather than per-declaration: a model's entry point often branches
/// only through the helpers it calls, as `price` does through `discountCents`,
/// and its own body carries no literal at all.
pub fn boundaries(source: &str) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for raw in source.lines() {
        let line = clean(raw);
        for token in line.split(|c: char| !c.is_ascii_digit()) {
            if token.is_empty() || token.len() > 12 {
                continue;
            }
            let Ok(value) = token.parse::<u64>() else { continue };
            for candidate in [value.saturating_sub(1), value, value.saturating_add(1)] {
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
    }
    out.sort_unstable();
    // A bound, so one pathological file cannot swamp the generator's own edges
    // and turn every case into a boundary case.
    out.truncate(64);
    out
}

/// Map one Lean type onto the whitelisted generator grammar.
fn schema_for(ty: &str, types: &LeanTypes, edges: &[u64], depth: usize) -> Result<Schema, String> {
    let ty = ty.trim();
    // A model deep enough to recurse this far is not the small, first-order
    // thing differential testing wants; refusing beats generating forever.
    if depth > 6 {
        return Err(format!("{ty} (nested too deeply to generate)"));
    }

    if let Some(inner) = ty.strip_prefix("Option ").or_else(|| ty.strip_prefix("Option(")) {
        let inner = inner.trim_end_matches(')');
        return Ok(Schema::Option { inner: Box::new(schema_for(inner, types, edges, depth + 1)?) });
    }
    if let Some(inner) = ty.strip_prefix("List ").or_else(|| ty.strip_prefix("Array ")) {
        return Ok(Schema::List {
            inner: Box::new(schema_for(inner.trim_end_matches(')'), types, edges, depth + 1)?),
            // Bounded, because a differential run executes this millions of
            // times and an unbounded list is how a run becomes a hang.
            max_len: Some(8),
        });
    }

    match ty {
        "Nat" => return Ok(Schema::Nat { max: None, edges: edges.to_vec() }),
        "Int" => return Ok(Schema::Int { min: None, max: None }),
        "Bool" => return Ok(Schema::Bool),
        "String" | "Char" => {
            return Ok(Schema::Str { max_len: Some(32), examples: Vec::new() })
        }
        _ => {}
    }

    if let Some(fields) = types.structures.get(ty) {
        let mut out = BTreeMap::new();
        for (field, field_ty) in fields {
            out.insert(field.clone(), schema_for(field_ty, types, edges, depth + 1)?);
        }
        return Ok(Schema::Struct { fields: out });
    }

    if let Some(constructors) = types.inductives.get(ty) {
        let mut out = BTreeMap::new();
        for (ctor, payload) in constructors {
            let payload = match payload {
                Some(p) => Some(schema_for(p, types, edges, depth + 1)?),
                None => None,
            };
            out.insert(ctor.clone(), payload);
        }
        return Ok(Schema::Enum { variants: out });
    }

    Err(ty.to_string())
}

/// Propose the input schema for one model entry point.
///
/// A single argument whose type is a structure becomes that structure — the
/// natural reading, and what a hand-written binding does anyway. Several
/// arguments become a struct keyed by the argument names, because the protocol
/// carries one JSON value per case.
pub fn infer_input(source: &str, declaration: &str) -> Result<Schema, InferError> {
    let binders = parse_binders(source, declaration)
        .ok_or_else(|| InferError::NoSuchDeclaration(declaration.to_string()))?;
    if binders.is_empty() {
        return Err(InferError::NoArguments(declaration.to_string()));
    }

    let types = scan_types(source);
    let edges = boundaries(source);
    let mut fields = BTreeMap::new();
    for (name, ty) in &binders {
        let schema = schema_for(ty, &types, &edges, 0).map_err(|bad| {
            if bad == *ty {
                InferError::Unsupported {
                    declaration: declaration.to_string(),
                    argument: name.clone(),
                    ty: bad,
                }
            } else {
                InferError::Unknown { ty: bad }
            }
        })?;
        fields.insert(name.clone(), schema);
    }

    if binders.len() == 1 {
        if let Some(schema) = fields.values().next() {
            if matches!(schema, Schema::Struct { .. }) {
                return Ok(schema.clone());
            }
        }
    }
    Ok(Schema::Struct { fields })
}
