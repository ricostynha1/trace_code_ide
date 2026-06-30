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
    Lean,
}

impl Lang {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Lang::Rust),
            "py" => Some(Lang::Python),
            "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" => Some(Lang::Cpp),
            "lean" => Some(Lang::Lean),
            _ => None,
        }
    }

    pub fn tree_sitter_language(&self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Lang::Lean => tree_sitter_lean4::language().into(),
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
                Lang::Lean => extract_lean_symbol(kind, &node, source, path),
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

fn extract_lean_symbol(
    kind: &str,
    node: &tree_sitter::Node,
    source: &str,
    path: &Path,
) -> Option<Symbol> {
    // Lean 4 tree-sitter node kinds for definitions
    let sym_kind = match kind {
        "definition" | "def" => SymbolKind::Function,
        "theorem" => SymbolKind::Function,
        "structure" => SymbolKind::Struct,
        "inductive" => SymbolKind::Enum,
        "class" => SymbolKind::Class,
        "instance" => SymbolKind::Impl,
        "namespace" => SymbolKind::Module,
        _ => return None,
    };

    // Try to find the name — look for first identifier-like child
    let name = find_lean_name(node, source)?;

    Some(Symbol {
        name,
        kind: sym_kind,
        file: path.to_path_buf(),
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
        start_col: node.start_position().column as u32,
    })
}

/// Find the name of a Lean definition (first identifier after the keyword)
fn find_lean_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let ck = child.kind();
            // Look for identifier or name nodes
            if ck == "identifier" || ck == "name" || ck == "ident" {
                return child.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
            }
            // Check field "name" if grammar uses it
            if let Some(name_node) = node.child_by_field_name("name") {
                return name_node.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

// --- Syntax Highlighting via Tree-Sitter ---

/// A highlight span: byte range + category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub from: usize,
    pub to: usize,
    /// Category: "keyword", "string", "comment", "number", "type", "function",
    /// "operator", "variable", "property", "punctuation"
    pub category: &'static str,
}

/// Parse a file and return highlight spans for the frontend.
pub fn get_highlights(path: &Path, content: &str) -> Vec<HighlightSpan> {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e,
        None => return Vec::new(),
    };
    let lang = match Lang::from_extension(ext) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let mut parser = Parser::new();
    if parser.set_language(&lang.tree_sitter_language()).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut spans = Vec::new();
    collect_highlight_spans(&tree.root_node(), content, lang, &mut spans);
    // Sort by start position for frontend consumption
    spans.sort_by_key(|s| s.from);
    spans
}

fn collect_highlight_spans(
    node: &tree_sitter::Node,
    source: &str,
    lang: Lang,
    spans: &mut Vec<HighlightSpan>,
) {
    let kind = node.kind();
    let from = node.start_byte();
    let to = node.end_byte();

    // If this node maps to a highlight category and is a leaf (or token-like), emit it
    if let Some(cat) = classify_node(kind, node, source, lang) {
        // Only emit for leaf-ish nodes (no children or token nodes)
        if node.child_count() == 0 || is_token_node(kind, lang) {
            spans.push(HighlightSpan { from, to, category: cat });
            return; // Don't recurse into children of token nodes
        }
    }

    // Recurse into children
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            collect_highlight_spans(&cursor.node(), source, lang, spans);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

/// Returns true for node kinds that are complete tokens (shouldn't recurse into)
fn is_token_node(kind: &str, _lang: Lang) -> bool {
    matches!(kind,
        "string_literal" | "string" | "raw_string_literal" |
        "line_comment" | "block_comment" | "comment" |
        "integer_literal" | "float_literal" | "number" |
        "char_literal" | "string_content"
    )
}

/// Map a tree-sitter node kind to a highlight category.
fn classify_node(kind: &str, node: &tree_sitter::Node, source: &str, lang: Lang) -> Option<&'static str> {
    // Universal patterns first
    match kind {
        // Comments
        "line_comment" | "block_comment" | "comment" => return Some("comment"),
        // Strings
        "string_literal" | "string" | "raw_string_literal" | "char_literal" |
        "string_content" | "escape_sequence" => return Some("string"),
        // Numbers
        "integer_literal" | "float_literal" | "number" | "number_literal" => return Some("number"),
        // Operators
        "!" | "!=" | "%" | "&" | "&&" | "*" | "+" | "-" | "/" |
        "<" | "<=" | "=" | "==" | ">" | ">=" | "|" | "||" | "^" |
        "+=" | "-=" | "*=" | "/=" | "<<" | ">>" | ".." | "..=" |
        "=>" | "->" | "<-" | ":=" => return Some("operator"),
        // Punctuation
        "(" | ")" | "[" | "]" | "{" | "}" | ";" | "," | "." | "::" | ":" => return Some("punctuation"),
        _ => {}
    }

    // Language-specific classification
    match lang {
        Lang::Rust => classify_rust_node(kind, node, source),
        Lang::Python => classify_python_node(kind, node, source),
        Lang::Cpp => classify_cpp_node(kind, node, source),
        Lang::Lean => classify_lean_node(kind, node, source),
    }
}

fn classify_rust_node(kind: &str, node: &tree_sitter::Node, _source: &str) -> Option<&'static str> {
    match kind {
        // Keywords
        "let" | "mut" | "fn" | "pub" | "struct" | "enum" | "impl" | "trait" |
        "use" | "mod" | "crate" | "self" | "super" | "where" | "as" | "in" |
        "for" | "while" | "loop" | "if" | "else" | "match" | "return" |
        "break" | "continue" | "async" | "await" | "move" | "ref" | "type" |
        "const" | "static" | "unsafe" | "extern" | "dyn" | "macro_rules!" => Some("keyword"),
        "true" | "false" => Some("number"), // bool literals
        // Type identifiers
        "type_identifier" | "primitive_type" => Some("type"),
        // Function calls
        "identifier" => {
            let parent = node.parent()?;
            match parent.kind() {
                "function_item" => Some("function"),
                "call_expression" => Some("function"),
                _ => None,
            }
        }
        "field_identifier" => Some("property"),
        "attribute_item" | "attribute" => Some("keyword"),
        "mutable_specifier" => Some("keyword"),
        _ => None,
    }
}

fn classify_python_node(kind: &str, node: &tree_sitter::Node, source: &str) -> Option<&'static str> {
    match kind {
        "def" | "class" | "return" | "if" | "elif" | "else" | "for" | "while" |
        "import" | "from" | "as" | "with" | "try" | "except" | "finally" |
        "raise" | "pass" | "break" | "continue" | "and" | "or" | "not" |
        "in" | "is" | "lambda" | "yield" | "global" | "nonlocal" | "assert" |
        "del" | "async" | "await" => Some("keyword"),
        "true" | "false" | "True" | "False" | "None" => Some("number"),
        "identifier" => {
            let parent = node.parent()?;
            match parent.kind() {
                "function_definition" => Some("function"),
                "class_definition" => Some("type"),
                "call" if node.start_byte() == parent.start_byte() => Some("function"),
                "decorator" => Some("keyword"),
                _ => {
                    // Check if it looks like a type (PascalCase)
                    let text = node.utf8_text(source.as_bytes()).ok()?;
                    if text.len() > 1 && text.chars().next()?.is_uppercase() {
                        Some("type")
                    } else {
                        None
                    }
                }
            }
        }
        "decorator" => Some("keyword"),
        _ => None,
    }
}

fn classify_cpp_node(kind: &str, node: &tree_sitter::Node, _source: &str) -> Option<&'static str> {
    match kind {
        "if" | "else" | "for" | "while" | "do" | "switch" | "case" | "break" |
        "continue" | "return" | "goto" | "typedef" | "struct" | "union" | "enum" |
        "class" | "public" | "private" | "protected" | "virtual" | "override" |
        "const" | "static" | "extern" | "inline" | "volatile" | "register" |
        "auto" | "template" | "typename" | "namespace" | "using" | "new" | "delete" |
        "throw" | "try" | "catch" | "sizeof" | "nullptr" | "#include" | "#define" |
        "#ifdef" | "#ifndef" | "#endif" | "#if" | "#else" => Some("keyword"),
        "true" | "false" | "NULL" => Some("number"),
        "type_identifier" | "primitive_type" | "sized_type_specifier" => Some("type"),
        "identifier" => {
            let parent = node.parent()?;
            match parent.kind() {
                "function_declarator" | "call_expression" => Some("function"),
                _ => None,
            }
        }
        "field_identifier" => Some("property"),
        "preproc_include" | "preproc_def" | "preproc_ifdef" => Some("keyword"),
        _ => None,
    }
}

fn classify_lean_node(kind: &str, node: &tree_sitter::Node, source: &str) -> Option<&'static str> {
    match kind {
        "def" | "theorem" | "lemma" | "example" | "structure" | "class" |
        "instance" | "inductive" | "namespace" | "section" | "open" | "variable" |
        "axiom" | "noncomputable" | "private" | "protected" | "partial" | "unsafe" |
        "where" | "with" | "match" | "do" | "let" | "have" | "show" | "if" |
        "then" | "else" | "for" | "in" | "return" | "import" | "prelude" |
        "universe" | "set_option" | "attribute" | "deriving" | "extends" |
        "abbrev" | "opaque" | "mutual" | "end" | "macro" | "syntax" | "elab" |
        "notation" | "by" | "fun" | "sorry" | "admit" => Some("keyword"),
        "Type" | "Prop" | "Sort" => Some("type"),
        "ident" | "identifier" | "name" => {
            let text = node.utf8_text(source.as_bytes()).ok()?;
            // Keywords that appear as identifiers in some grammars
            match text {
                "def" | "theorem" | "lemma" | "structure" | "class" | "instance" |
                "where" | "with" | "do" | "let" | "have" | "if" | "then" | "else" |
                "match" | "fun" | "by" | "sorry" | "import" | "open" | "namespace" |
                "end" | "return" | "for" | "in" => Some("keyword"),
                "Type" | "Prop" | "Sort" | "Nat" | "Int" | "Bool" | "String" |
                "Unit" | "Option" | "List" | "Array" | "IO" | "True" | "False" => Some("type"),
                _ => {
                    if text.len() > 1 && text.chars().next()?.is_uppercase() {
                        Some("type")
                    } else {
                        let parent = node.parent()?;
                        match parent.kind() {
                            "definition" | "def" | "theorem" | "lemma" => Some("function"),
                            _ => None,
                        }
                    }
                }
            }
        }
        _ => None,
    }
}
