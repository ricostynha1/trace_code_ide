//! Tree-sitter based parsing: syntax highlighting and symbol extraction.
//! Highlighting: parse tree → walk nodes → lookup node kind in JSON → return (span, color).
//! Symbol extraction: language-agnostic walk looking for named definition nodes.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Parser, Tree};

// --- Language Registry ---

/// Language definition: name, extensions, tree-sitter grammar, color config.
#[derive(Debug, Clone)]
pub struct Lang {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub tree_sitter_language: Language,
    /// node_kind → hex color, loaded from JSON
    pub colors: HashMap<String, String>,
}

/// All supported languages
pub fn all_languages() -> Vec<Lang> {
    vec![
        Lang::new("rust", &["rs"], tree_sitter_rust::LANGUAGE.into()),
        Lang::new("python", &["py"], tree_sitter_python::LANGUAGE.into()),
        Lang::new("cpp", &["c", "cpp", "cc", "cxx", "h", "hpp"], tree_sitter_cpp::LANGUAGE.into()),
        Lang::new("lean4", &["lean"], tree_sitter_lean4::language().into()),
        Lang::new("markdown", &["md"], tree_sitter_md::LANGUAGE.into()),
        Lang::new("javascript", &["js", "mjs", "cjs"], tree_sitter_javascript::LANGUAGE.into()),
        Lang::new("html", &["html", "htm"], tree_sitter_html::LANGUAGE.into()),
        Lang::new("css", &["css"], tree_sitter_css::LANGUAGE.into()),
        Lang::new("json", &["json"], tree_sitter_json::LANGUAGE.into()),
    ]
}

impl Lang {
    fn new(name: &'static str, extensions: &'static [&'static str], ts_lang: Language) -> Self {
        let colors = load_color_config(name);
        Self {
            name,
            extensions,
            tree_sitter_language: ts_lang,
            colors,
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        all_languages().into_iter().find(|l| l.extensions.contains(&ext))
    }
}

/// Load node_kind → color mapping from ui_settings/{name}.json
/// Searches multiple locations to work in both dev and deployed contexts.
fn load_color_config(name: &str) -> HashMap<String, String> {
    let filename = format!("{}.json", name);

    // Locations to search (in priority order):
    let candidates = [
        // 1. Next to binary (deployed)
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("ui_settings").join(&filename))),
        // 2. /app/ui_settings (Docker build context)
        Some(PathBuf::from("/app/ui_settings").join(&filename)),
        // 3. Relative to CARGO_MANIFEST_DIR (dev builds) → ../ui_settings/
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(Path::new("."))
            .join("ui_settings")
            .join(&filename)),
        // 4. CWD fallback
        Some(PathBuf::from("ui_settings").join(&filename)),
    ];

    for candidate in candidates.iter().flatten() {
        if let Ok(content) = fs::read_to_string(candidate) {
            if let Ok(map) = serde_json::from_str(&content) {
                return map;
            }
        }
    }
    HashMap::new()
}

// --- Syntax Highlighting ---

/// A highlight span: byte range + color hex string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub from: usize,
    pub to: usize,
    pub color: String,
}

/// Parse file, walk tree, return colored spans.
/// For markdown, uses both block and inline grammars and merges results.
/// Returns spans with CHARACTER offsets (not byte offsets) for frontend compatibility.
pub fn get_highlights(path: &Path, content: &str) -> Vec<HighlightSpan> {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e,
        None => return Vec::new(),
    };
    let lang = match Lang::from_extension(ext) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let src = content.as_bytes();

    // Markdown needs dual-parse (block + inline)
    let mut spans = if lang.name == "markdown" {
        get_highlights_markdown(content, &lang.colors)
    } else {
        let mut parser = Parser::new();
        if parser.set_language(&lang.tree_sitter_language).is_err() {
            return Vec::new();
        }
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Vec::new(),
        };
        let mut s = Vec::new();
        collect_spans(&tree.root_node(), &lang.colors, src, &mut s);
        s
    };

    // Convert byte offsets → char offsets for CodeMirror
    let byte_to_char = build_byte_to_char_map(content);
    let content_len_chars = content.chars().count();
    for span in &mut spans {
        span.from = byte_to_char_offset(&byte_to_char, span.from);
        span.to = byte_to_char_offset(&byte_to_char, span.to);
        // Clamp to valid range
        if span.to > content_len_chars {
            span.to = content_len_chars;
        }
        if span.from > span.to {
            span.from = span.to;
        }
    }

    // Remove zero-length spans and sort
    spans.retain(|s| s.from < s.to);
    spans.sort_by_key(|s| s.from);
    spans
}

/// Build a lookup: byte_offset → char_offset.
/// Returns a vec where index = byte offset, value = char offset.
fn build_byte_to_char_map(content: &str) -> Vec<usize> {
    let mut map = Vec::with_capacity(content.len() + 1);
    let mut char_idx = 0;
    for (byte_idx, ch) in content.char_indices() {
        // Fill all bytes of this character with the same char_idx
        while map.len() < byte_idx {
            map.push(char_idx);
        }
        map.push(char_idx);
        char_idx += 1;
    }
    // Fill remaining (for the position past the last char)
    while map.len() <= content.len() {
        map.push(char_idx);
    }
    map
}

/// Convert a byte offset to char offset using the precomputed map.
fn byte_to_char_offset(map: &[usize], byte_off: usize) -> usize {
    if byte_off >= map.len() {
        *map.last().unwrap_or(&0)
    } else {
        map[byte_off]
    }
}

/// Markdown: parse with block grammar, then inline grammar, merge spans.
fn get_highlights_markdown(content: &str, colors: &HashMap<String, String>) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    let src = content.as_bytes();

    // Block parse
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_md::LANGUAGE.into()).is_ok() {
        if let Some(tree) = parser.parse(content, None) {
            collect_spans(&tree.root_node(), colors, src, &mut spans);
        }
    }

    // Inline parse
    let mut parser2 = Parser::new();
    if parser2.set_language(&tree_sitter_md::INLINE_LANGUAGE.into()).is_ok() {
        if let Some(tree) = parser2.parse(content, None) {
            collect_spans(&tree.root_node(), colors, src, &mut spans);
        }
    }

    // Deduplicate: keep last (inline overrides block at same position)
    spans.sort_by_key(|s| (s.from, s.to));
    spans.dedup_by(|b, a| a.from == b.from && a.to == b.to);
    spans
}

/// Recursively walk tree, emit span when node kind has a color mapping.
/// Leaf nodes: match by kind first, then by text content (for type names etc).
/// Non-leaf nodes with a color: gap-fill uncovered ranges when children are simple.
fn collect_spans(
    node: &tree_sitter::Node,
    colors: &HashMap<String, String>,
    source: &[u8],
    spans: &mut Vec<HighlightSpan>,
) {
    let kind = node.kind();

    // Leaf node
    if node.child_count() == 0 {
        // Match by node kind
        if let Some(color) = colors.get(kind) {
            if node.start_byte() < node.end_byte() {
                spans.push(HighlightSpan {
                    from: node.start_byte(),
                    to: node.end_byte(),
                    color: color.clone(),
                });
            }
        } else if node.is_named() {
            // Fallback: match by text content (handles Nat, Bool, True, etc.)
            if let Ok(text) = node.utf8_text(source) {
                if let Some(color) = colors.get(text) {
                    spans.push(HighlightSpan {
                        from: node.start_byte(),
                        to: node.end_byte(),
                        color: color.clone(),
                    });
                }
            }
        }
        return;
    }

    // Non-leaf node
    let parent_color = colors.get(kind);

    // Recurse children
    let mut cursor = node.walk();
    let mut child_ranges: Vec<(usize, usize)> = Vec::new();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            child_ranges.push((child.start_byte(), child.end_byte()));
            collect_spans(&child, colors, source, spans);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    // Gap-fill for non-leaf nodes that have a color and only simple children
    if let Some(color) = parent_color {
        let all_children_simple = {
            let mut c = node.walk();
            let mut ok = true;
            if c.goto_first_child() {
                loop {
                    if c.node().is_named() && c.node().child_count() > 0 {
                        ok = false;
                        break;
                    }
                    if !c.goto_next_sibling() {
                        break;
                    }
                }
            }
            ok
        };

        if all_children_simple {
            let node_start = node.start_byte();
            let node_end = node.end_byte();
            let mut pos = node_start;
            for (cs, ce) in &child_ranges {
                if pos < *cs {
                    spans.push(HighlightSpan {
                        from: pos,
                        to: *cs,
                        color: color.clone(),
                    });
                }
                pos = pos.max(*ce);
            }
            if pos < node_end {
                spans.push(HighlightSpan {
                    from: pos,
                    to: node_end,
                    color: color.clone(),
                });
            }
        }
    }
}

// --- Symbol Extraction (language-agnostic) ---

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

    pub fn parse_file(&mut self, path: &Path, content: &str) -> Option<Vec<Symbol>> {
        let ext = path.extension()?.to_str()?;
        let lang = Lang::from_extension(ext)?;

        let mut parser = Parser::new();
        parser.set_language(&lang.tree_sitter_language).ok()?;
        let tree = parser.parse(content, None)?;

        let symbols = extract_symbols(&tree, content, path);
        self.files.insert(path.to_path_buf(), symbols.clone());
        Some(symbols)
    }

    pub fn remove_file(&mut self, path: &Path) {
        self.files.remove(path);
    }

    pub fn get_symbols(&self, path: &Path) -> Option<&Vec<Symbol>> {
        self.files.get(path)
    }

    pub fn all_symbols(&self) -> Vec<&Symbol> {
        self.files.values().flat_map(|s| s.iter()).collect()
    }
}

/// Parse multiple files in parallel
pub fn parse_files_parallel(files: &[(PathBuf, String)]) -> Vec<FileParseResult> {
    files.par_iter().filter_map(|(path, content)| {
        let ext = path.extension()?.to_str()?;
        let lang = Lang::from_extension(ext)?;

        let mut parser = Parser::new();
        parser.set_language(&lang.tree_sitter_language).ok()?;
        let tree = parser.parse(content, None)?;

        let symbols = extract_symbols(&tree, content, path);

        Some(FileParseResult {
            path: path.clone(),
            symbols,
            tree: Some(tree),
        })
    }).collect()
}

/// Language-agnostic symbol extraction.
/// Walks top-level nodes looking for common definition patterns.
fn extract_symbols(tree: &Tree, source: &str, path: &Path) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();

    if cursor.goto_first_child() {
        loop {
            let node = cursor.node();
            if let Some(sym) = try_extract_symbol(&node, source, path) {
                symbols.push(sym);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    symbols
}

/// Try to extract a symbol from a node using common tree-sitter patterns.
/// Works across languages by checking known definition node kinds.
fn try_extract_symbol(
    node: &tree_sitter::Node,
    source: &str,
    path: &Path,
) -> Option<Symbol> {
    let kind = node.kind();

    let sym_kind = match kind {
        // Rust
        "function_item" => SymbolKind::Function,
        "struct_item" => SymbolKind::Struct,
        "enum_item" => SymbolKind::Enum,
        "trait_item" => SymbolKind::Trait,
        "impl_item" => SymbolKind::Impl,
        "mod_item" => SymbolKind::Module,
        // Python
        "function_definition" => SymbolKind::Function,
        "class_definition" => SymbolKind::Class,
        // C++
        "class_specifier" => SymbolKind::Class,
        "struct_specifier" => SymbolKind::Struct,
        "enum_specifier" => SymbolKind::Enum,
        // Lean
        "definition" | "def" => SymbolKind::Function,
        "theorem" => SymbolKind::Function,
        "structure" => SymbolKind::Struct,
        "inductive" => SymbolKind::Enum,
        "instance" => SymbolKind::Impl,
        // JavaScript
        "function_declaration" => SymbolKind::Function,
        "method_definition" => SymbolKind::Method,
        _ => return None,
    };

    // Try to find name via "name" field, then "declarator", then first identifier child
    let name = find_name(node, source)?;

    Some(Symbol {
        name,
        kind: sym_kind,
        file: path.to_path_buf(),
        start_line: node.start_position().row as u32,
        end_line: node.end_position().row as u32,
        start_col: node.start_position().column as u32,
    })
}

/// Find the name of a definition node. Tries field "name", then "declarator", then first identifier.
fn find_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    // Try "name" field
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(text) = name_node.utf8_text(source.as_bytes()) {
            return Some(text.to_string());
        }
    }

    // Try "declarator" field (C++)
    if let Some(decl_node) = node.child_by_field_name("declarator") {
        if let Some(id) = find_first_identifier(&decl_node, source) {
            return Some(id);
        }
    }

    // Try "type" field (Rust impl)
    if let Some(type_node) = node.child_by_field_name("type") {
        if let Ok(text) = type_node.utf8_text(source.as_bytes()) {
            return Some(text.to_string());
        }
    }

    // Fallback: first identifier-like child
    find_first_identifier(node, source)
}

/// Find the first identifier node in a subtree
fn find_first_identifier(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let ck = child.kind();
            if ck == "identifier" || ck == "name" || ck == "ident" || ck == "field_identifier" {
                return child.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
            }
            // Recurse one level
            if let Some(id) = find_first_identifier(&child, source) {
                return Some(id);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}
