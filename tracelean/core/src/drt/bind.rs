//! Binding a clause to a differential test, as one action.
//!
//! Before this, binding a model to an implementation meant hand-writing three
//! things in three places — a `.tracelean/drt.json` entry, an adapter file, and
//! a `@drt` annotation — with no help from the tool. The predictable result is
//! visible in any project that tries: clauses sit `Unbound`, meaning a model and
//! an implementation exist and *nothing checks they agree*, which is the single
//! most important finding the checker produces.
//!
//! So the work is proposed rather than demanded. Everything derivable is
//! derived: the input schema from the Lean signature (`infer.rs`), the
//! implementation's entry point from its `@implements` anchor, the adapter from
//! both. What cannot be derived is *said*, not guessed.
//!
//! Two rules the flow keeps:
//!
//! * **Nothing is written before the user has seen it.** `propose` returns the
//!   whole plan — schema, adapter source, binding JSON — and writes nothing.
//! * **What is written is written as one `Command::Batch`.** A binding that
//!   exists in two of its three places is worse than one that exists in none,
//!   and going through the command stream means it undoes like any other edit.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::config::{Binding, CallSpec, DrtConfig};
use super::protocol::RunnerSpec;
use super::run::CoverageFloor;
use super::schema::Schema;
use crate::commands::Command;
use crate::trace::{Role, TraceIndex};

/// Everything the flow intends to do, before it does any of it.
#[derive(Debug, Clone, Serialize)]
pub struct BindProposal {
    pub req_id: String,
    pub clause: Option<String>,

    /// The Lean file and declaration the schema was read from.
    pub model_file: PathBuf,
    pub model_declaration: String,

    /// The implementation this will be driven against.
    pub impl_file: PathBuf,
    pub impl_symbol: String,
    pub language: String,

    pub binding: Binding,
    /// `.tracelean/drt.json` as it will read afterwards.
    pub config_after: String,

    /// Decisions the tool could not make. Shown, never silently defaulted.
    pub problems: Vec<String>,
}

fn language_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("py") => "python",
        Some("rs") => "rust",
        Some("ts") | Some("js") => "javascript",
        _ => "other",
    }
}

/// Field names the adapter will unpack from a case payload.
fn field_names(schema: &Schema) -> Vec<String> {
    match schema {
        Schema::Struct { fields } => fields.keys().cloned().collect(),
        _ => vec!["value".to_string()],
    }
}

/// Parameter names of `symbol` in an implementation source.
///
/// A shallow scan, in the spirit of `infer`: enough for the one question asked
/// of it, and `None` rather than a guess for anything else.
fn implementation_parameters(source: &str, symbol: &str, language: &str) -> Option<Vec<String>> {
    let keyword = match language {
        "python" => "def ",
        "rust" => "fn ",
        _ => return None,
    };
    let needle = format!("{keyword}{symbol}(");
    let start = source.find(&needle)? + needle.len();
    let rest = &source[start..];

    // Balanced scan: a default value or a generic argument can contain parens.
    let mut depth = 0usize;
    let mut end = None;
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' | '[' | '<' => depth += 1,
            ')' if depth == 0 => {
                end = Some(i);
                break;
            }
            ')' | ']' | '>' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    let list = &rest[..end?];

    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    for ch in list.chars() {
        match ch {
            '(' | '[' | '<' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '>' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if depth == 0 => {
                push_parameter(&mut out, &current);
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    push_parameter(&mut out, &current);
    Some(out)
}

fn push_parameter(out: &mut Vec<String>, raw: &str) {
    let name = raw.split(':').next().unwrap_or(raw).split('=').next().unwrap_or(raw);
    let name = name.trim().trim_start_matches(['&', '*']).trim();
    // `self`, `mut x`, and the bare `/` `*` markers of a Python signature are
    // not arguments a case can supply.
    let name = name.rsplit(char::is_whitespace).next().unwrap_or(name);
    if name.is_empty() || name == "self" || name == "/" || name == "*" {
        return;
    }
    out.push(name.to_string());
}

/// Work out what binding this clause should have, without writing anything.
pub fn propose(
    root: &Path,
    index: &TraceIndex,
    req_id: &str,
    clause: Option<&str>,
) -> Result<BindProposal, String> {
    let links = index.links_for_clause(req_id, clause);

    let model = links
        .iter()
        .find(|l| l.role == Role::Models)
        .ok_or_else(|| {
            format!(
                "{req_id}{} has no `@models` link — there is no model to bind. \
                 Write the Lean model first; differential testing compares two things.",
                clause.map(|c| format!(".{c}")).unwrap_or_default()
            )
        })?;

    let implementation = links
        .iter()
        .find(|l| l.role == Role::Implements)
        .ok_or_else(|| {
            format!(
                "{req_id}{} has no `@implements` link — there is nothing to compare the model \
                 against yet.",
                clause.map(|c| format!(".{c}")).unwrap_or_default()
            )
        })?;

    let mut problems = Vec::new();
    let mut params: BTreeMap<String, String> = BTreeMap::new();

    let model_source = std::fs::read_to_string(root.join(&model.anchor.file))
        .map_err(|e| format!("reading {}: {e}", model.anchor.file.display()))?;
    let declaration = model
        .anchor
        .symbol_path()
        .map(|s| s.rsplit("::").next().unwrap_or(s).to_string())
        .ok_or_else(|| {
            format!(
                "the `@models` annotation for {req_id} is not attached to a declaration, so \
                 there is no signature to read a schema from"
            )
        })?;

    let input = match super::infer::infer_input(&model_source, &declaration) {
        Ok(schema) => schema,
        Err(e) => {
            problems.push(e.to_string());
            // A placeholder the user must replace. It is in `problems`, so it
            // cannot pass for an inferred answer.
            Schema::Struct { fields: BTreeMap::new() }
        }
    };

    let impl_symbol = implementation
        .anchor
        .symbol_path()
        .map(|s| s.rsplit("::").next().unwrap_or(s).to_string())
        .unwrap_or_else(|| {
            problems.push(
                "the `@implements` annotation covers a whole file rather than a declaration, so \
                 the adapter does not know which function to call"
                    .to_string(),
            );
            "TODO".to_string()
        });

    let language = language_for(&implementation.anchor.file).to_string();
    let fields = field_names(&input);

    // The adapter calls the implementation with the *model's* binder names as
    // keyword arguments, because those are the only names the schema carries.
    // When the implementation spells them differently -- `subtotal` in Lean and
    // `subtotal_cents` in Python -- the generated call raises on the first case.
    // Saying so here turns a runtime crash into a sentence naming both lists;
    // the mapping itself stays the author's, since matching them positionally
    // would silently swap two arguments of the same type.
    if !fields.is_empty() {
        if let Ok(impl_source) = std::fs::read_to_string(root.join(&implementation.anchor.file)) {
            if let Some(parameters) =
                implementation_parameters(&impl_source, &impl_symbol, &language)
            {
                let missing: Vec<&String> =
                    fields.iter().filter(|f| !parameters.contains(f)).collect();
                if !missing.is_empty() {
                    // The two sides spell the same arguments differently, which
                    // is ordinary -- `subtotalCents` in Lean, `subtotal_cents`
                    // in Python. When the counts agree, position is the only
                    // available correspondence and it is almost always right;
                    // it is recorded in the binding where a person can read it,
                    // and reported here rather than assumed silently.
                    if fields.len() == parameters.len() {
                        for (field, parameter) in fields.iter().zip(parameters.iter()) {
                            if field != parameter {
                                params.insert(field.clone(), parameter.clone());
                            }
                        }
                        problems.push(format!(
                            "the model names its arguments {:?} and `{impl_symbol}` takes {:?} — \
                             they were paired in declaration order, which the binding's `params` \
                             now records. Check it.",
                            fields, parameters
                        ));
                    } else {
                        problems.push(format!(
                            "the model names its arguments {:?} and `{impl_symbol}` takes {:?}, \
                             and there are different numbers of them — fill in the binding's \
                             `params` by hand, or bind a `convert` function",
                            fields, parameters
                        ));
                    }
                }
            }
        }
    }

    // No adapter, in any language. A binding describes the call; TraceLean's
    // shipped runner makes it. The runner for a compiled language has to be
    // generated and built rather than shipped as a script, which is why Rust
    // is a stated gap rather than a half-working template -- a binding that
    // cannot run is better than one that appears to.
    if language != "python" {
        return Err(format!(
            "differential testing has no runner for {language} implementations yet -- only \
             Python. The Lean model side is generated and built for you; the same has to \
             happen for a compiled implementation, and it does not yet."
        ));
    }
    let implementation_binding = CallSpec {
        language: "python".into(),
        entry: format!("{}::{}", implementation.anchor.file.display(), impl_symbol),
        params: params.clone(),
        convert: None,
    };

    let binding = Binding {
        req_id: req_id.to_string(),
        clause: clause.map(|c| c.to_string()),
        op: crate::drt::config::qualified_op(req_id, clause),
        model: RunnerSpec {
            cmd: vec![".tracelean/drt/.lake/build/bin/drtRunner".into()],
            cwd: None,
            env: BTreeMap::new(),
        },
        implementation: implementation_binding,
        input,
        coverage_floor: CoverageFloor::default(),
    };

    let mut config = DrtConfig::load(root)?;
    config
        .bindings
        .retain(|b| !(b.req_id == req_id && b.clause.as_deref() == clause));
    config.bindings.push(binding.clone());
    let config_after = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("serializing drt config: {e}"))?
        + "\n";

    Ok(BindProposal {
        req_id: req_id.to_string(),
        clause: clause.map(|c| c.to_string()),
        model_file: model.anchor.file.clone(),
        model_declaration: declaration,
        impl_file: implementation.anchor.file.clone(),
        impl_symbol,
        language,
        binding,
        config_after,
        problems,
    })
}


/// Turn a proposal into commands. Nothing touches disk here either — the caller
/// applies these through the same path a hand edit takes.
///
/// One `Batch` on purpose: the adapter, the config entry and the annotation
/// inside the adapter are one fact about the project, and a half-applied
/// binding is a worse state than an unbound clause.
pub fn apply(root: &Path, proposal: &BindProposal) -> Result<Command, String> {
    let mut commands = Vec::new();

    let config_path = PathBuf::from(".tracelean").join("drt.json");
    let existing = std::fs::read_to_string(root.join(&config_path)).ok();
    match existing {
        Some(current) => commands.push(Command::Replace {
            file: config_path,
            at: 0,
            old: current,
            new: proposal.config_after.clone(),
        }),
        None => {
            commands.push(Command::CreateFile { path: config_path.clone() });
            commands.push(Command::Replace {
                file: config_path,
                at: 0,
                old: String::new(),
                new: proposal.config_after.clone(),
            });
        }
    }

    Ok(Command::Batch { commands })
}
