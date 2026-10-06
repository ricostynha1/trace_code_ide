//! Generating the TypeScript side of a differential run.
//!
//! The web frontend implements the functions that read a buffer, so it is a
//! second implementation of the same clauses — and the question that matters
//! about it is whether it answers what the *model* answers. This generates the
//! process that asks it.
//!
//! Cheaper than the Rust runner in one way and dearer in another. Node strips
//! the types and runs the file, so there is no build and no build output that
//! could differ from the source the browser loads; but nothing typechecks the
//! call, so a binding that does not fit is a caught exception rather than a
//! compile error. That is why every call is wrapped.
//!
//! @implements REQ-DRT-TS.generated

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::Binding;

/// Where the generated runner lives. Under the cache directory, because it is
/// TraceLean's code and not something a person maintains.
///
/// @implements REQ-DRT-TS.project_untouched
pub fn package_dir(root: &Path) -> PathBuf {
    root.join(".tracelean").join("drt-ts")
}

/// What a binding resolved to, once its source has been read.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub op: String,
    /// The module to import, relative to the generated runner.
    pub module: String,
    /// The exported name to call.
    pub symbol: String,
    /// Implementation parameter names, in declaration order.
    pub parameters: Vec<String>,
    /// Implementation parameter name -> model field name to read it from.
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    MalformedEntry(String),
    MissingFile(PathBuf),
    /// No such export, or a parameter this cannot name — a destructuring
    /// pattern, an optional one. Refused rather than guessed at: supplying the
    /// wrong argument produces agreement about the wrong thing.
    NoSuchFunction { file: PathBuf, symbol: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::MalformedEntry(e) => {
                write!(f, "entry {e:?} must be of the form path/to/file.ts::symbol")
            }
            ResolveError::MissingFile(p) => write!(f, "{}: no such file", p.display()),
            ResolveError::NoSuchFunction { file, symbol } => write!(
                f,
                "{}: no `function {symbol}` found, or a parameter of it is a pattern rather \
                 than a name",
                file.display()
            ),
        }
    }
}

/// The parameters of a TypeScript function, in the order it declares them.
///
/// A name, then optionally a type after `:` or a default after `=`. Anything
/// else answers `None`, because a parameter this cannot name is one the runner
/// cannot supply.
///
/// @implements REQ-DRT-TS.params_from_source
/// @drt REQ-DRT-TS.params_from_source
pub fn ts_parameters_of(source: &str, symbol: &str) -> Option<Vec<String>> {
    use super::signature::{is_identifier, matching, split_top_level};

    let open = signature_open_paren(source, symbol)?;
    let close = matching(source, open, '(', ')')?;

    let mut out = Vec::new();
    for part in split_top_level(&source[open + 1..close], ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        // A name, then optionally a type or a default. A destructuring pattern
        // or an optional `name?` leaves something that is not an identifier,
        // and that is refused rather than guessed at.
        let named = part.split([':', '=']).next().unwrap_or("").trim();
        if named.is_empty() || !is_identifier(named) {
            return None;
        }
        out.push(named.to_string());
    }
    Some(out)
}

/// The `(` that opens `function <symbol>`'s parameter list.
///
/// Separate from the Rust one rather than sharing a keyword argument: the two
/// only look alike until one language's declarations change.
fn signature_open_paren(source: &str, symbol: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(found) = source[from..].find("function ") {
        let at = from + found;
        from = at + 9;
        let rest = &source[at + 9..];
        let name_len = rest
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .count();
        if &rest[..name_len] != symbol {
            continue;
        }
        let after = &rest[name_len..];
        let spaces = after.len() - after.trim_start().len();
        if after.trim_start().starts_with('(') {
            return Some(at + 9 + name_len + spaces);
        }
    }
    None
}

/// Resolve one binding's TypeScript implementation against the project's source.
///
/// The module recorded is the project's own file, so what the runner executes
/// is what the browser loads — there is no build output in between that could
/// differ from it.
///
/// @implements REQ-DRT-TS.params_from_source
/// @implements REQ-DRT-TS.runs_the_shipped_source
pub fn resolve(root: &Path, binding: &Binding, spec: &super::CallSpec) -> Result<Entry, ResolveError> {
    let (rel, symbol) = spec
        .split_entry()
        .ok_or_else(|| ResolveError::MalformedEntry(spec.entry.clone()))?;
    let file = root.join(rel);
    let source =
        std::fs::read_to_string(&file).map_err(|_| ResolveError::MissingFile(file.clone()))?;

    let parameters = ts_parameters_of(&source, symbol).ok_or_else(|| {
        ResolveError::NoSuchFunction { file: file.clone(), symbol: symbol.to_string() }
    })?;

    let mut by_parameter: BTreeMap<&str, &str> = BTreeMap::new();
    for (field, parameter) in &spec.params {
        by_parameter.insert(parameter, field);
    }
    let sources = parameters
        .iter()
        .map(|p| by_parameter.get(p.as_str()).map(|f| f.to_string()).unwrap_or_else(|| p.clone()))
        .collect();

    Ok(Entry {
        op: binding.op(),
        // Absolute, because the generated runner is written to a cache
        // directory that is not under the project. The file imported is the
        // project's own source, which is what `runs_the_shipped_source` means.
        module: file.display().to_string(),
        symbol: symbol.to_string(),
        parameters,
        sources,
    })
}

/// The generated runner.
///
/// One import per module, one case per op, and every call wrapped: nothing this
/// generates decides anything about a value, which is the same promise the Rust
/// runner makes by not naming a type.
///
/// @implements REQ-DRT-TS.generated
/// @implements REQ-DRT-TS.failure_is_an_answer
/// @implements REQ-DRT-PROTO.line_delimited
/// @implements REQ-DRT-PROTO.runner_shared
pub fn runner_source(entries: &[Entry]) -> String {
    let mut modules: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for entry in entries {
        modules.entry(entry.module.as_str()).or_default().push(entry);
    }

    let mut out = String::from("// Generated by TraceLean. Edits are overwritten.\n");
    for (index, (module, _)) in modules.iter().enumerate() {
        out.push_str(&format!("import * as m{index} from {module:?};\n"));
    }
    out.push_str("\nfunction answer(op, input) {\n  switch (op) {\n");
    for (index, (_, group)) in modules.iter().enumerate() {
        for entry in group {
            let args: Vec<String> =
                entry.sources.iter().map(|source| format!("input[{source:?}]")).collect();
            out.push_str(&format!(
                "    case {:?}:\n      return m{index}.{}({});\n",
                entry.op,
                entry.symbol,
                args.join(", ")
            ));
        }
    }
    out.push_str(
        "    default:\n      throw new Error(`no binding for op ${JSON.stringify(op)}`);\n\
         \x20 }\n}\n\n\
         let held = \"\";\n\
         process.stdin.on(\"data\", (chunk) => {\n\
         \x20 held += chunk;\n\
         \x20 let at;\n\
         \x20 while ((at = held.indexOf(\"\\n\")) >= 0) {\n\
         \x20   const line = held.slice(0, at);\n\
         \x20   held = held.slice(at + 1);\n\
         \x20   if (line.trim() === \"\") continue;\n\
         \x20   let asked;\n\
         \x20   try {\n\
         \x20     asked = JSON.parse(line);\n\
         \x20   } catch (e) {\n\
         \x20     process.stderr.write(`unreadable case: ${e}\\n`);\n\
         \x20     continue;\n\
         \x20   }\n\
         \x20   // A thrown error is an answer, not an abort: the model may\n\
         \x20   // reject this input too, and both rejecting is agreement.\n\
         \x20   let reply;\n\
         \x20   try {\n\
         \x20     reply = { case: asked.case ?? 0, output: answer(asked.op, asked.input ?? {}) };\n\
         \x20   } catch (e) {\n\
         \x20     reply = { case: asked.case ?? 0, error: String(e && e.message ? e.message : e) };\n\
         \x20   }\n\
         \x20   process.stdout.write(JSON.stringify(reply) + \"\\n\");\n\
         \x20 }\n\
         });\n",
    );
    out
}

/// Write the generated runner, if it differs from what would be generated now.
///
/// @implements REQ-DRT-TS.generated
/// @implements REQ-DRT-TS.project_untouched
pub fn materialize(root: &Path, entries: &[Entry]) -> std::io::Result<PathBuf> {
    let dir = package_dir(root);
    std::fs::create_dir_all(&dir)?;
    // A package that is not a module would make `import` a syntax error, and
    // the project's own package.json is not this runner's to read.
    let manifest = "{\n  \"private\": true,\n  \"type\": \"module\"\n}\n";
    write_if_changed(&dir.join("package.json"), manifest)?;
    write_if_changed(&dir.join("runner.ts"), &runner_source(entries))?;
    Ok(dir)
}

fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).map(|c| c == contents).unwrap_or(false) {
        return Ok(());
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-DRT-TS.params_from_source
    #[test]
    fn parameters_come_from_the_signature_and_a_pattern_is_refused() {
        let source = "export function plainText(buffer: Buffer): string[] {\n  return [];\n}\n\
                      export function actionsAt(buffer: Buffer, offset: number): string[] {}\n\
                      export function nothing() {}\n";
        assert_eq!(ts_parameters_of(source, "plainText"), Some(vec!["buffer".to_string()]));
        assert_eq!(
            ts_parameters_of(source, "actionsAt"),
            Some(vec!["buffer".to_string(), "offset".to_string()])
        );
        // A function with no parameters is not a missing one.
        assert_eq!(ts_parameters_of(source, "nothing"), Some(vec![]));
        assert_eq!(ts_parameters_of(source, "absent"), None);

        // A comma inside a type does not start a parameter.
        assert_eq!(
            ts_parameters_of("export function f(a: Map<string, number>, b: number) {}", "f"),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        // And a parameter this cannot name is refused.
        assert_eq!(ts_parameters_of("export function f(a: number, b?: number) {}", "f"), None);
        assert_eq!(ts_parameters_of("export function f({a, b}: Pair) {}", "f"), None);
    }

    /// The generated runner imports each module once and dispatches by op.
    ///
    /// @tests REQ-DRT-TS.generated
    /// @tests REQ-DRT-TS.failure_is_an_answer
    #[test]
    fn the_generated_runner_calls_the_entry_point_and_catches_what_it_throws() {
        let entries = vec![
            Entry {
                op: "REQ-VIEW.text_is_the_content".into(),
                module: "/w/web/src/view.ts".into(),
                symbol: "plainText".into(),
                parameters: vec!["buffer".into()],
                sources: vec!["buffer".into()],
            },
            Entry {
                op: "REQ-VIEW.affordances_named".into(),
                module: "/w/web/src/view.ts".into(),
                symbol: "actionsAt".into(),
                parameters: vec!["buffer".into(), "offset".into()],
                sources: vec!["buffer".into(), "offset".into()],
            },
        ];
        let source = runner_source(&entries);
        assert_eq!(source.matches("import * as").count(), 1, "one import per module:\n{source}");
        assert!(source.contains("m0.plainText(input[\"buffer\"])"), "{source}");
        assert!(
            source.contains("m0.actionsAt(input[\"buffer\"], input[\"offset\"])"),
            "{source}"
        );
        // Nothing stands between the input and the call: the arguments are the
        // input's own fields, named by the signature.
        assert!(!source.contains("JSON.parse(input"), "{source}");
        assert!(source.contains("catch (e)"), "a throw must become this case's error");
    }
}
