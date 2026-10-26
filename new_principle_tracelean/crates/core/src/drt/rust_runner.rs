use std::collections::BTreeMap;
use super::lean_runner::write_if_changed;
use std::path::{Path, PathBuf};

use super::config::Binding;
use super::signature;

/// @implements REQ-DRT-RUST.project_untouched
pub fn package_dir(root: &Path) -> PathBuf {
    root.join(".tracelean").join("drt-rust")
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub op: String,
    pub call_path: String,
    pub parameters: Vec<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    MalformedEntry(String),
    MissingFile(PathBuf),
    NoSuchFunction { file: PathBuf, symbol: String },
    AmbiguousFunction { file: PathBuf, symbol: String, count: usize },
    NotInACrate(PathBuf),
    UnmappedField { op: String, parameter: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::MalformedEntry(e) => {
                write!(f, "entry {e:?} must be of the form path/to/file.rs::symbol")
            }
            ResolveError::MissingFile(p) => write!(f, "{}: no such file", p.display()),
            ResolveError::NoSuchFunction { file, symbol } => write!(
                f,
                "{}: no `fn {symbol}` found, or its parameters are patterns rather than names",
                file.display()
            ),
            ResolveError::AmbiguousFunction { file, symbol, count } => write!(
                f,
                "{}: {count} functions are named `{symbol}`; a binding must name exactly one",
                file.display()
            ),
            ResolveError::NotInACrate(p) => write!(
                f,
                "{}: not inside a crate with a src/ directory and a Cargo.toml",
                p.display()
            ),
            ResolveError::UnmappedField { op, parameter } => write!(
                f,
                "binding {op}: parameter `{parameter}` is not supplied by the model's input; \
                 add it to the binding's `params` map"
            ),
        }
    }
}

/// @implements REQ-DRT-RUST.params_from_source
pub fn resolve(root: &Path, binding: &Binding) -> Result<Entry, ResolveError> {
    let spec = &binding.implementation;
    let (rel, symbol) = spec
        .split_entry()
        .ok_or_else(|| ResolveError::MalformedEntry(spec.entry.clone()))?;

    let file = root.join(rel);
    let source = std::fs::read_to_string(&file)
        .map_err(|_| ResolveError::MissingFile(file.clone()))?;

    let declared = signature::declarations(&source, symbol);
    if declared > 1 {
        return Err(ResolveError::AmbiguousFunction {
            file: file.clone(),
            symbol: symbol.to_string(),
            count: declared,
        });
    }

    let parameters = signature::parameters(&source, symbol).ok_or_else(|| {
        ResolveError::NoSuchFunction { file: file.clone(), symbol: symbol.to_string() }
    })?;

    let module = module_path(root, Path::new(rel))
        .ok_or_else(|| ResolveError::NotInACrate(PathBuf::from(rel)))?;

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
        call_path: format!("{module}::{symbol}"),
        parameters,
        sources,
    })
}

pub fn module_path(root: &Path, rel: &Path) -> Option<String> {
    let components: Vec<&str> = rel.iter().filter_map(|c| c.to_str()).collect();
    let src_at = components.iter().rposition(|c| *c == "src")?;

    let crate_dir = root.join(components[..src_at].iter().collect::<PathBuf>());
    let lib = library_name(&crate_dir.join("Cargo.toml"))?;

    let mut segments = vec![lib];
    for part in &components[src_at + 1..] {
        let stem = part.strip_suffix(".rs").unwrap_or(part);
        if stem == "lib" || stem == "mod" || stem == "main" {
            continue;
        }
        segments.push(stem.to_string());
    }
    Some(segments.join("::"))
}

fn library_name(cargo_toml: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cargo_toml).ok()?;
    let mut section = "";
    let mut package_name = None;
    let mut lib_name = None;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = if line.starts_with("[lib]") { "lib" } else { "" };
            continue;
        }
        if let Some(value) = line.strip_prefix("name") {
            let value = value.trim_start().strip_prefix('=')?.trim().trim_matches('"');
            if section == "lib" {
                lib_name = Some(value.to_string());
            } else if package_name.is_none() {
                package_name = Some(value.to_string());
            }
        }
    }
    lib_name.or(package_name).map(|n| n.replace('-', "_"))
}

/// @implements REQ-DRT-RUST.path_dependency
pub fn cargo_toml(crate_deps: &BTreeMap<String, String>) -> String {
    let mut out = String::from(
        "# Generated by TraceLean. Edits are overwritten.\n\
         [workspace]\n\n\
         [package]\nname = \"tracelean-drt-runner\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nserde_json = { version = \"1\", features = [\"raw_value\"] }\n",
    );
    for (name, path) in crate_deps {
        out.push_str(&format!("{name} = {{ path = {path:?} }}\n"));
    }
    out
}

fn literal(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            ' '..='~' => out.push(c),
            other => out.push_str(&format!("\\u{{{:x}}}", other as u32)),
        }
    }
    out.push('"');
    out
}

/// @implements REQ-DRT-RUST.types_inferred
/// @drt REQ-DRT-RUST.types_inferred
pub fn call_site(entry: Entry) -> String {
    let mut out = format!("        {} => {{\n", literal(&entry.op));
    for (parameter, source) in entry.parameters.iter().zip(&entry.sources) {
        out.push_str(&format!(
            "            let {parameter} = serde_json::from_str(field(input, {}))\n\
             \x20               .map_err(|e| format!(\"argument `{parameter}`: {{e}}\"))?;\n",
            literal(source)
        ));
    }
    out.push_str(&format!(
        "            let out = {}({});\n\
         \x20           serde_json::to_value(out).map_err(|e| e.to_string())\n\
         \x20       }}\n",
        entry.call_path,
        entry.parameters.join(", ")
    ));
    out
}

/// @implements REQ-DRT-PROTO.reply_one_line
/// @implements REQ-DRT-PROTO.case_names_op
/// @implements REQ-DRT-PROTO.runner_shared
pub fn main_rs(entries: &[Entry]) -> String {
    let mut out = String::from(
        "// Generated by TraceLean. Edits are overwritten.\n\
         use std::collections::BTreeMap;\n\
         use std::io::{BufRead, Write};\n\
         use serde_json::value::RawValue;\n\n\
         type Fields<'a> = BTreeMap<String, &'a RawValue>;\n\n\
         /// The raw JSON text of one input field, or `null` when absent.\n\
         ///\n\
         /// Arguments are deserialized from the *raw slice* rather than from an\n\
         /// owned value so that a borrowed parameter -- `&str`, `&[u8]` -- can\n\
         /// borrow from the case line, which outlives the call. Deserializing\n\
         /// from an owned value would restrict every implementation to owned\n\
         /// parameters, which is not how Rust is written.\n\
         fn field<'a>(input: &Fields<'a>, name: &str) -> &'a str {\n\
         \x20   input.get(name).map(|v| v.get()).unwrap_or(\"null\")\n\
         }\n\n\
         fn answer(op: &str, input: &Fields<'_>) -> Result<serde_json::Value, String> {\n\
         \x20   match op {\n",
    );
    for entry in entries {
        out.push_str(&call_site(entry.clone()));
    }
    out.push_str(
        "        other => Err(format!(\"no binding for op {other:?}\")),\n\
         \x20   }\n}\n\n\
         fn main() {\n\
         \x20   let stdin = std::io::stdin();\n\
         \x20   let mut stdout = std::io::stdout();\n\
         \x20   for line in stdin.lock().lines() {\n\
         \x20       let Ok(line) = line else { break };\n\
         \x20       if line.trim().is_empty() { continue }\n\
         \x20       let case: Fields = match serde_json::from_str(&line) {\n\
         \x20           Ok(v) => v,\n\
         \x20           Err(e) => { eprintln!(\"unreadable case: {e}\"); continue }\n\
         \x20       };\n\
         \x20       let number: u64 = serde_json::from_str(field(&case, \"case\")).unwrap_or(0);\n\
         \x20       let op: String = serde_json::from_str(field(&case, \"op\")).unwrap_or_default();\n\
         \x20       let input: Fields =\n\
         \x20           serde_json::from_str(field(&case, \"input\")).unwrap_or_default();\n\
         \x20       // A panic is an answer, not an abort: the model may reject this\n\
         \x20       // input too, and both rejecting is agreement.\n\
         \x20       let caught = std::panic::catch_unwind(\n\
         \x20           std::panic::AssertUnwindSafe(|| answer(&op, &input)));\n\
         \x20       let reply = match caught {\n\
         \x20           Ok(Ok(output)) => serde_json::json!({\"case\": number, \"output\": output}),\n\
         \x20           Ok(Err(e)) => serde_json::json!({\"case\": number, \"error\": e}),\n\
         \x20           Err(_) => serde_json::json!({\"case\": number, \"error\": \"panic\"}),\n\
         \x20       };\n\
         \x20       let _ = writeln!(stdout, \"{reply}\");\n\
         \x20       let _ = stdout.flush();\n\
         \x20   }\n}\n",
    );
    out
}

/// @implements REQ-DRT-RUST.generated
/// @implements REQ-DRT-RUST.rewritten_when_stale
pub fn materialize(
    root: &Path,
    entries: &[Entry],
    crate_deps: &BTreeMap<String, String>,
) -> std::io::Result<PathBuf> {
    let dir = package_dir(root);
    std::fs::create_dir_all(dir.join("src"))?;
    write_if_changed(&dir.join("Cargo.toml"), &cargo_toml(crate_deps))?;
    write_if_changed(&dir.join("src").join("main.rs"), &main_rs(entries))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(entry: &str) -> Binding {
        Binding {
            req_id: "REQ-X".into(),
            clause: Some("c".into()),
            also_checks: Vec::new(),
            also_implemented_by: Vec::new(),
            floors: Vec::new(),
            waive: Vec::new(),
            op: None,
            model: None,
            implementation: super::super::CallSpec {
                language: "rust".into(),
                entry: entry.into(),
                params: BTreeMap::new(),
            },
        }
    }

    /// @tests REQ-DRT-RUST.path_dependency
    #[test]
    fn module_path_from_file_path() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
        assert_eq!(
            module_path(root, Path::new("crates/core/src/drt/signature.rs")).as_deref(),
            Some("tracelean_core::drt::signature")
        );
        assert_eq!(
            module_path(root, Path::new("crates/core/src/lib.rs")).as_deref(),
            Some("tracelean_core")
        );
        assert_eq!(
            module_path(root, Path::new("crates/core/src/drt/mod.rs")).as_deref(),
            Some("tracelean_core::drt")
        );
    }

    /// @tests REQ-DRT-RUST.params_from_source
    #[test]
    fn resolves_against_real_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
        let entry = resolve(root, &binding("crates/core/src/drt/signature.rs::parameters")).unwrap();
        assert_eq!(entry.op, "REQ-X.c");
        assert_eq!(entry.call_path, "tracelean_core::drt::signature::parameters");
        assert_eq!(entry.parameters, vec!["source".to_string(), "symbol".to_string()]);
        assert_eq!(entry.sources, entry.parameters);
    }

    #[test]
    fn missing_function_is_named() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
        let err = resolve(root, &binding("crates/core/src/drt/signature.rs::nope")).unwrap_err();
        assert!(matches!(err, ResolveError::NoSuchFunction { .. }), "{err}");
    }

    /// @tests REQ-DRT-BIND.rename_only
    #[test]
    fn params_map_renames_the_field_read_for_a_parameter() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
        let mut b = binding("crates/core/src/drt/signature.rs::parameters");
        b.implementation.params.insert("code".into(), "source".into());
        let entry = resolve(root, &b).unwrap();
        assert_eq!(entry.parameters, vec!["source".to_string(), "symbol".to_string()]);
        assert_eq!(entry.sources, vec!["code".to_string(), "symbol".to_string()]);
    }

    /// @tests REQ-DRT-RUST.types_inferred
    #[test]
    fn generated_code_names_no_types() {
        let entry = Entry {
            op: "REQ-X.c".into(),
            call_path: "k::f".into(),
            parameters: vec!["a".into()],
            sources: vec!["a".into()],
        };
        let code = main_rs(&[entry]);
        assert!(code.contains("serde_json::from_str(field(input, \"a\"))"));
        assert!(!code.contains("::<"), "generated code named a type:\n{code}");
    }

    /// @tests REQ-DRT-RUST.rewritten_when_stale
    #[test]
    fn regeneration_is_a_no_op_when_unchanged() {
        let dir = std::env::temp_dir().join(format!("tl-drt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let entries = [Entry {
            op: "REQ-X.c".into(),
            call_path: "k::f".into(),
            parameters: vec![],
            sources: vec![],
        }];
        let deps = BTreeMap::new();
        materialize(&dir, &entries, &deps).unwrap();
        let main = package_dir(&dir).join("src").join("main.rs");
        let before = std::fs::metadata(&main).unwrap().modified().unwrap();
        materialize(&dir, &entries, &deps).unwrap();
        assert_eq!(before, std::fs::metadata(&main).unwrap().modified().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
