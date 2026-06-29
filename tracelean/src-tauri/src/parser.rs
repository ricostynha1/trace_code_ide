//! Tree-sitter based parsing: syntax highlighting data and symbol extraction.
//! Runs natively in Rust for code model building and graph construction.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Parser, Tree};

/// Supported languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Lang {
    Rust,
    Python,
    Cpp,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Lang::Rust),
            "py" => Some(Lang::Python),
            "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" => Some(Lang::Cpp),
            _ => None,
        }
    }

    pub fn tree_sitter_language(&self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        }
    }
}

/// A symbol extracted from source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Module,
    Trait,
    Impl,
    Variable,
}

/// Per-file parse result
#[derive(Debug, Clone)]
pub struct FileParseResult {
    pub path: PathBuf,
    pub lang: Lang,
    pub symbols: Vec<Symbol>,
    pub tree: Option<Tree>,
}

/// Symbol table: file path → symbols
#[derive(Debug, Default)]
pub struct SymbolTable {
    pub files: HashMap<PathBuf, Vec<Symbol>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self { files: HashMap::new() }
    }

    /// Parse a single file, extract symbols
    pub fn parse_file(&mut self, path: &Path, content: &str) -> Option<Vec<Symbol>> {
        let ext = path.extension()?.to_str()?;
        let lang = Lang::from_extension(ext)?;

        let mut parser = Parser::new();
        parser.set_language(&lang.tree_sitter_language()).ok()?;
        let tree = parser.parse(content, None)?;

        let symbols = extract_symbols(&tree, content, path, lang);
        self.files.insert(path.to_path_buf(), symbols.clone());
        Some(symbols)
    }

    /// Remove symbols for a file (before re-parse)
    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(path);
    }

    /// Get symbols for a file
    pub fn get_symbols(&self, path: &Path) -> Option<&Vec<Symbol>> {
        self.files.get(path)
    }

    /// Get all symbols across all files
    pub fn all_symbols(&self) -> Vec<&Symbol> {
        self.files.values().flat_map(|s| s.iter()).collect()
    }
}

/// Parse multiple files in parallel using Rayon
pub fn parse_files_parallel(files: &[(PathBuf, String)]) -> Vec<FileParseResult> {
    files.par_iter().filter_map(|(path, content)| {
        let ext = path.extension()?.to_str()?;
        let lang = Lang::from_extension(ext)?;

        let mut parser = Parser::new();
        parser.set_language(&lang.tree_sitter_language()).ok()?;
        let tree = parser.parse(content, None)?;

        let symbols = extract_symbols(&tree, content, path, lang);

        Some(FileParseResult {
            path: path.clone(),
            lang,
            symbols,
            tree: Some(tree),
        })
    }).collect()
}

/// Extract symbols from a parsed tree
fn extract_symbols(tree: &Tree, source: &str, path: &Path, lang: Lang) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    // Walk top-level children
    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            let kind = node.kind();

            let symbol = match lang {
                Lang::Rust => extract_rust_symbol(kind, &node, source, path),
                Lang::Python => extract_python_symbol(kind, &node, source, path),
                Lang::Cpp => extract_cpp_symbol(kind, &node, source, path),
            };

            if let Some(sym) = symbol {
                symbols.push(sym);
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    symbols
}

fn extract_rust_symbol(
    kind: &str,
    node: &tree_sitter::Node,
    source: &str,
    path: &Path,
) -> Option<Symbol> {
    let (sym_kind, name_field) = match kind {
        "function_item" => (SymbolKind::Function, "name"),
        "struct_item" => (SymbolKind::Struct, "name"),
        "enum_item" => (SymbolKind::Enum, "name"),
        "trait_item" => (SymbolKind::Trait, "name"),
        "impl_item" => (SymbolKind::Impl, "type"),
        "mod_item" => (SymbolKind::Module, "name"),
        _ => return None,
    };

    let name_node = node.child_by_field_name(name_field)?;
    let name = name_node.utf8_text(source.as_bytes()).ok()?.to_string();

    Some(Symbol {
        name,
        kind: sym_kind,
        file: path.to_path_buf(),
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
        start_col: node.start_position().column as u32,
    })
}

fn extract_python_symbol(
    kind: &str,
    node: &tree_sitter::Node,
    source: &str,
    path: &Path,
) -> Option<Symbol> {
    let (sym_kind, name_field) = match kind {
        "function_definition" => (SymbolKind::Function, "name"),
        "class_definition" => (SymbolKind::Class, "name"),
        _ => return None,
    };

    let name_node = node.child_by_field_name(name_field)?;
    let name = name_node.utf8_text(source.as_bytes()).ok()?.to_string();

    Some(Symbol {
        name,
        kind: sym_kind,
        file: path.to_path_buf(),
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
        start_col: node.start_position().column as u32,
    })
}

fn extract_cpp_symbol(
    kind: &str,
    node: &tree_sitter::Node,
    source: &str,
    path: &Path,
) -> Option<Symbol> {
    let (sym_kind, name_field) = match kind {
        "function_definition" => (SymbolKind::Function, "declarator"),
        "class_specifier" => (SymbolKind::Class, "name"),
        "struct_specifier" => (SymbolKind::Struct, "name"),
        "enum_specifier" => (SymbolKind::Enum, "name"),
        _ => return None,
    };

    let name_node = node.child_by_field_name(name_field)?;
    // For functions, the declarator may be nested — get the identifier
    let name = if kind == "function_definition" {
        find_identifier(name_node, source)?
    } else {
        name_node.utf8_text(source.as_bytes()).ok()?.to_string()
    };

    Some(Symbol {
        name,
        kind: sym_kind,
        file: path.to_path_buf(),
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
        start_col: node.start_position().column as u32,
    })
}

/// Find the first identifier in a subtree (for C++ declarators)
fn find_identifier(node: tree_sitter::Node, source: &str) -> Option<String> {
    if node.kind() == "identifier" || node.kind() == "field_identifier" {
        return node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if let Some(name) = find_identifier(cursor.node(), source) {
                return Some(name);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}
