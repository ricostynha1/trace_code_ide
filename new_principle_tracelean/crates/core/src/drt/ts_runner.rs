//! @implements REQ-DRT-TS.generated

use super::lean_runner::write_if_changed;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::Binding;

/// @implements REQ-DRT-TS.project_untouched
pub fn package_dir(root: &Path) -> PathBuf {
    root.join(".tracelean").join("drt-ts")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub op: String,
    pub module: String,
    pub symbol: String,
    pub parameters: Vec<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    MalformedEntry(String),
    MissingFile(PathBuf),
    NoSuchFunction { file: PathBuf, symbol: String },
    AmbiguousFunction { file: PathBuf, symbol: String, count: usize },
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
            ResolveError::AmbiguousFunction { file, symbol, count } => write!(
                f,
                "{}: {count} functions are named `{symbol}`; a binding must name exactly one",
                file.display()
            ),
        }
    }
}

const TS_QUOTES: &[char] = &['"', '\'', '`'];

pub fn ts_parameters_of(source: &str, symbol: &str) -> Option<Vec<String>> {
    use super::signature::{blanked, is_identifier, matching, split_top_level};

    let source = &blanked(source, TS_QUOTES, false);
    if declarations_in(source, symbol) > 1 {
        return None;
    }
    let open = signature_open_paren(source, symbol)?;
    let close = matching(source, open, '(', ')')?;

    let mut out = Vec::new();
    for part in split_top_level(&source[open + 1..close], ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let named = part.split([':', '=']).next().unwrap_or("").trim();
        if named.is_empty() || !is_identifier(named) {
            return None;
        }
        if named != "this" {
            out.push(named.to_string());
        }
    }
    Some(out)
}

/// @implements REQ-DRT-TS.params_from_source
/// @drt REQ-DRT-TS.params_from_source
pub fn ts_parameters_owned(source: String, symbol: String) -> Option<Vec<String>> {
    ts_parameters_of(&source, &symbol)
}

pub fn ts_declarations(source: &str, symbol: &str) -> usize {
    declarations_in(&super::signature::blanked(source, TS_QUOTES, false), symbol)
}

fn declarations_in(source: &str, symbol: &str) -> usize {
    let mut count = 0;
    let mut from = 0usize;
    while let Some(open) = signature_open_paren(&source[from..], symbol) {
        count += 1;
        from += open + 1;
    }
    count
}

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
        let skip_space = |mut cursor: usize| {
            while source[cursor..].starts_with([' ', '\t', '\r', '\n']) {
                cursor += 1;
            }
            cursor
        };
        let mut cursor = skip_space(at + 9 + name_len);
        if source[cursor..].starts_with('<') {
            match super::signature::matching(source, cursor, '<', '>') {
                Some(end) => cursor = skip_space(end + 1),
                None => cursor = source.len(),
            }
        }
        if source[cursor..].starts_with('(') {
            return Some(cursor);
        }
    }
    None
}

/// @implements REQ-DRT-TS.params_from_source
/// @implements REQ-DRT-TS.runs_the_shipped_source
pub fn resolve(root: &Path, binding: &Binding, spec: &super::CallSpec) -> Result<Entry, ResolveError> {
    let (rel, symbol) = spec
        .split_entry()
        .ok_or_else(|| ResolveError::MalformedEntry(spec.entry.clone()))?;
    let file = root.join(rel);
    let source =
        std::fs::read_to_string(&file).map_err(|_| ResolveError::MissingFile(file.clone()))?;

    let declared = ts_declarations(&source, symbol);
    if declared > 1 {
        return Err(ResolveError::AmbiguousFunction {
            file: file.clone(),
            symbol: symbol.to_string(),
            count: declared,
        });
    }

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
        module: file.display().to_string(),
        symbol: symbol.to_string(),
        parameters,
        sources,
    })
}

/// @implements REQ-DRT-TS.generated
/// @implements REQ-DRT-TS.failure_is_an_answer
/// @implements REQ-DRT-PROTO.reply_one_line
/// @implements REQ-DRT-PROTO.case_names_op
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

/// @implements REQ-DRT-TS.generated
/// @implements REQ-DRT-TS.project_untouched
pub fn materialize(root: &Path, entries: &[Entry]) -> std::io::Result<PathBuf> {
    let dir = package_dir(root);
    std::fs::create_dir_all(&dir)?;
    let manifest = "{\n  \"private\": true,\n  \"type\": \"module\"\n}\n";
    write_if_changed(&dir.join("package.json"), manifest)?;
    write_if_changed(&dir.join("runner.ts"), &runner_source(entries))?;
    Ok(dir)
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
        assert_eq!(ts_parameters_of(source, "nothing"), Some(vec![]));
        assert_eq!(ts_parameters_of(source, "absent"), None);

        assert_eq!(
            ts_parameters_of("export function f(a: Map<string, number>, b: number) {}", "f"),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(ts_parameters_of("export function f(a: number, b?: number) {}", "f"), None);
        assert_eq!(ts_parameters_of("export function f({a, b}: Pair) {}", "f"), None);
    }

    /// @tests REQ-DRT-TS.params_from_source
    #[test]
    fn a_comment_this_and_a_second_declaration_do_not_reorder_the_arguments() {
        let two = || vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            ts_parameters_of("// was: function f(b, a)\nexport function f(a: number, b: number) {}", "f"),
            Some(two())
        );
        assert_eq!(
            ts_parameters_of("/* function f(b, a) */ export function f(a, b) {}", "f"),
            Some(two())
        );
        assert_eq!(
            ts_parameters_of("const s = \"function f(b, a)\";\nexport function f(a, b) {}", "f"),
            Some(two())
        );
        assert_eq!(ts_parameters_of("export function f(this: Window, a, b) {}", "f"), Some(two()));
        assert_eq!(ts_parameters_of("function f(a) {}\nexport function f(a, b) {}", "f"), None);
        assert_eq!(ts_declarations("function f(a) {}\n// function f(b)\nfunction f(a, b) {}", "f"), 2);
    }

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
        assert!(!source.contains("JSON.parse(input"), "{source}");
        assert!(source.contains("catch (e)"), "a throw must become this case's error");
    }
}
