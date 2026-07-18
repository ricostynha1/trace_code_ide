//! Tree-sitter based parsing: syntax highlighting and symbol extraction.
//! Highlighting: .scm query execution → capture-name → theme-map color resolution.
//! Symbol extraction: language-agnostic walk looking for named definition nodes.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

// --- Language Registry ---

/// Language definition: name, extensions, tree-sitter grammar.
#[derive(Debug, Clone)]
pub struct Lang {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub tree_sitter_language: Language,
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
        Self {
            name,
            extensions,
            tree_sitter_language: ts_lang,
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        all_languages().into_iter().find(|l| l.extensions.contains(&ext))
    }
}

// --- Syntax Highlighting ---

/// A highlight span: byte range + color hex string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub from: usize,
    pub to: usize,
    pub color: String,
}

/// Build a lookup: byte_offset → char_offset.
/// Returns a vec where index = byte offset, value = char offset.
fn build_byte_to_char_map(content: &str) -> Vec<usize> {
    let mut map = Vec::with_capacity(content.len() + 1);
    let mut char_idx = 0;
    for (byte_idx, _ch) in content.char_indices() {
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

// --- Query-based Highlighting ---

/// A raw highlight capture: capture name + byte range.
/// Color resolution happens in a separate step (theme map).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HighlightCapture {
    pub capture_name: String,
    pub from: usize, // byte offset
    pub to: usize,   // byte offset
}

/// Load the .scm query source for a language.
/// Searches same candidate paths as load_color_config.
fn load_query_source(lang_name: &str) -> Option<String> {
    let (new_path, old_path) = if lang_name == "markdown_inline" {
        (
            "ui_settings/languages/markdown/highlights_inline.scm".to_string(),
            "grammars/markdown/highlights_inline.scm".to_string(),
        )
    } else {
        (
            format!("ui_settings/languages/{}/highlights.scm", lang_name),
            format!("grammars/{}/highlights.scm", lang_name),
        )
    };

    // Search candidates: new location first, then old for backward compat
    for filename in &[&new_path, &old_path] {
        let candidates = [
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join(filename))),
            Some(PathBuf::from("/app").join(filename)),
            Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(filename),
            ),
            Some(PathBuf::from(*filename)),
        ];

        for candidate in candidates.iter().flatten() {
            if let Ok(content) = fs::read_to_string(candidate) {
                return Some(content);
            }
        }
    }
    None
}

/// Cached compiled queries per language. Key = language name.
static QUERY_CACHE: OnceLock<std::sync::Mutex<HashMap<String, Arc<Query>>>> = OnceLock::new();

fn get_query_cache() -> &'static std::sync::Mutex<HashMap<String, Arc<Query>>> {
    QUERY_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Get or compile the highlight query for a language.
fn get_or_compile_query(lang_name: &str, ts_language: &Language) -> Option<Arc<Query>> {
    let cache = get_query_cache();
    let mut map = cache.lock().ok()?;

    if let Some(q) = map.get(lang_name) {
        return Some(Arc::clone(q));
    }

    let source = load_query_source(lang_name)?;
    let query = Query::new(ts_language, &source).ok()?;
    let arc = Arc::new(query);
    map.insert(lang_name.to_string(), Arc::clone(&arc));
    Some(arc)
}

/// Run highlight query on a parsed tree, return raw captures.
fn run_query_on_tree(
    query: &Query,
    tree: &Tree,
    source: &[u8],
) -> Vec<HighlightCapture> {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source);
    let capture_names = query.capture_names();

    let mut captures = Vec::new();
    while let Some(m) = matches.next() {
        for cap in m.captures {
            let name = &capture_names[cap.index as usize];
            let node = cap.node;
            if node.start_byte() < node.end_byte() {
                captures.push(HighlightCapture {
                    capture_name: name.to_string(),
                    from: node.start_byte(),
                    to: node.end_byte(),
                });
            }
        }
    }
    captures
}

/// Get highlight captures via .scm query execution.
/// Returns raw (capture_name, byte_range) results.
/// For markdown: runs query on both block and inline trees, merges.
pub fn get_highlight_captures(path: &Path, content: &str) -> Vec<HighlightCapture> {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e,
        None => return Vec::new(),
    };
    let lang = match Lang::from_extension(ext) {
        Some(l) => l,
        None => return Vec::new(),
    };

    let src = content.as_bytes();

    if lang.name == "markdown" {
        get_highlight_captures_markdown(content)
    } else {
        let query = match get_or_compile_query(lang.name, &lang.tree_sitter_language) {
            Some(q) => q,
            None => return Vec::new(),
        };

        let mut parser = Parser::new();
        if parser.set_language(&lang.tree_sitter_language).is_err() {
            return Vec::new();
        }
        let tree = match parser.parse(content, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut captures = run_query_on_tree(&query, &tree, src);
        captures.sort_by_key(|c| (c.from, c.to));
        captures
    }
}

/// Markdown: run query on block tree + inline tree, merge captures.
fn get_highlight_captures_markdown(content: &str) -> Vec<HighlightCapture> {
    let src = content.as_bytes();
    let mut captures = Vec::new();

    let block_lang: Language = tree_sitter_md::LANGUAGE.into();
    let inline_lang: Language = tree_sitter_md::INLINE_LANGUAGE.into();

    // Block parse + query
    if let Some(query) = get_or_compile_query("markdown", &block_lang) {
        let mut parser = Parser::new();
        if parser.set_language(&block_lang).is_ok() {
            if let Some(tree) = parser.parse(content, None) {
                captures.extend(run_query_on_tree(&query, &tree, src));
            }
        }
    }

    // Inline parse + query (uses same .scm file — captures that don't match are just skipped)
    if let Some(query) = get_or_compile_query("markdown_inline", &inline_lang) {
        let mut parser = Parser::new();
        if parser.set_language(&inline_lang).is_ok() {
            if let Some(tree) = parser.parse(content, None) {
                captures.extend(run_query_on_tree(&query, &tree, src));
            }
        }
    }

    // Sort and dedup (inline overrides block at same position)
    captures.sort_by_key(|c| (c.from, c.to));
    captures.dedup_by(|b, a| a.from == b.from && a.to == b.to && a.capture_name == b.capture_name);
    captures
}

// --- Theme Map: capture-name → color with longest-prefix fallback ---

static THEME_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();

fn load_theme_map() -> HashMap<String, String> {
    let filename = "ui_settings/theme.json";
    let candidates = [
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(filename))),
        Some(PathBuf::from("/app").join(filename)),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new(".")).join(filename)),
        Some(PathBuf::from(filename)),
    ];
    for candidate in candidates.iter().flatten() {
        if let Ok(content) = fs::read_to_string(candidate) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
                return map;
            }
        }
    }
    HashMap::new()
}

fn get_theme_map() -> &'static HashMap<String, String> {
    THEME_MAP.get_or_init(load_theme_map)
}

/// Resolve a capture name to a color using longest-prefix match.
/// E.g. "function.call" matches "function.call" > "function" > (none).
pub fn resolve_capture_color(capture_name: &str) -> Option<String> {
    let theme = get_theme_map();
    if theme.is_empty() {
        return None;
    }
    let mut name = capture_name;
    loop {
        if let Some(color) = theme.get(name) {
            return Some(color.clone());
        }
        match name.rfind('.') {
            Some(pos) => name = &name[..pos],
            None => return None,
        }
    }
}

/// Get highlight spans using query-based system with theme map color resolution.
/// Returns HighlightSpan (from/to in CHARACTER offsets, color resolved from capture name).
pub fn get_highlights_query(path: &Path, content: &str) -> Vec<HighlightSpan> {
    let captures = get_highlight_captures(path, content);
    if captures.is_empty() {
        return Vec::new();
    }

    let byte_to_char = build_byte_to_char_map(content);
    let content_len_chars = content.chars().count();

    let mut spans: Vec<HighlightSpan> = captures
        .into_iter()
        .filter_map(|cap| {
            let color = resolve_capture_color(&cap.capture_name)?;
            let mut from = byte_to_char_offset(&byte_to_char, cap.from);
            let mut to = byte_to_char_offset(&byte_to_char, cap.to);
            if to > content_len_chars { to = content_len_chars; }
            if from > to { from = to; }
            if from >= to { return None; }
            Some(HighlightSpan { from, to, color })
        })
        .collect();

    spans.sort_by_key(|s| s.from);
    spans
}

// --- Myth: node-at-position, actions-at-position, semantic navigation ---

/// Info about the AST node (or capture) under a position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRefInfo {
    /// Capture names covering the position, outermost → innermost.
    pub captures: Vec<String>,
    /// Innermost named AST node kind.
    pub kind: String,
    /// Char range of the innermost named node.
    pub from: usize,
    pub to: usize,
    pub text: String,
}

fn char_to_byte_offset(content: &str, char_pos: usize) -> usize {
    content
        .char_indices()
        .nth(char_pos)
        .map(|(b, _)| b)
        .unwrap_or(content.len())
}

fn parse_tree_for(path: &Path, content: &str) -> Option<(Lang, Tree)> {
    let ext = path.extension()?.to_str()?;
    let lang = Lang::from_extension(ext)?;
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter_language).ok()?;
    let tree = parser.parse(content, None)?;
    Some((lang, tree))
}

/// The node under a char position: innermost named AST node + the highlight
/// captures covering the position (outermost → innermost, i.e. capture order).
pub fn node_at(path: &Path, content: &str, char_pos: usize) -> Option<NodeRefInfo> {
    let byte_pos = char_to_byte_offset(content, char_pos);
    let (_, tree) = parse_tree_for(path, content)?;
    let root = tree.root_node();
    let node = root
        .named_descendant_for_byte_range(byte_pos, byte_pos)
        .unwrap_or(root);

    let captures: Vec<String> = get_highlight_captures(path, content)
        .into_iter()
        .filter(|c| c.from <= byte_pos && byte_pos < c.to.max(c.from + 1))
        .map(|c| c.capture_name)
        .collect();

    let byte_to_char = build_byte_to_char_map(content);
    Some(NodeRefInfo {
        captures,
        kind: node.kind().to_string(),
        from: byte_to_char_offset(&byte_to_char, node.start_byte()),
        to: byte_to_char_offset(&byte_to_char, node.end_byte()),
        text: node.utf8_text(content.as_bytes()).unwrap_or("").to_string(),
    })
}

/// Actions available at a position: union of the actions bound to every
/// capture covering it (fable Q5), in capture order.
pub fn actions_at(path: &Path, content: &str, char_pos: usize) -> Vec<String> {
    let Some(info) = node_at(path, content, char_pos) else {
        return Vec::new();
    };
    crate::myth::bindings::union_actions(&info.captures)
}

/// Semantic movement direction over the AST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NavDirection {
    Parent,
    Child,
    PrevSibling,
    NextSibling,
}

/// Semantic navigation: from the innermost named node at `char_pos`, move in
/// `dir` and return the char range of the destination node.
pub fn semantic_nav(
    path: &Path,
    content: &str,
    char_pos: usize,
    dir: NavDirection,
) -> Option<(usize, usize)> {
    let byte_pos = char_to_byte_offset(content, char_pos);
    let (_, tree) = parse_tree_for(path, content)?;
    let root = tree.root_node();
    let node = root.named_descendant_for_byte_range(byte_pos, byte_pos)?;

    let dest = match dir {
        NavDirection::Parent => node.parent(),
        NavDirection::Child => node.named_child(0),
        NavDirection::PrevSibling => node.prev_named_sibling(),
        NavDirection::NextSibling => node.next_named_sibling(),
    }?;

    let byte_to_char = build_byte_to_char_map(content);
    Some((
        byte_to_char_offset(&byte_to_char, dest.start_byte()),
        byte_to_char_offset(&byte_to_char, dest.end_byte()),
    ))
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

/// Try to extract a symbol from a node using config-driven symbol map.
/// Works across languages by checking node kinds against symbols.json.
fn try_extract_symbol(
    node: &tree_sitter::Node,
    source: &str,
    path: &Path,
) -> Option<Symbol> {
    let kind = node.kind();
    let map = get_symbol_map();

    let sym_kind = match map.get(kind).map(|s| s.as_str()) {
        Some("Function") => SymbolKind::Function,
        Some("Method") => SymbolKind::Method,
        Some("Class") => SymbolKind::Class,
        Some("Struct") => SymbolKind::Struct,
        Some("Enum") => SymbolKind::Enum,
        Some("Module") => SymbolKind::Module,
        Some("Trait") => SymbolKind::Trait,
        Some("Impl") => SymbolKind::Impl,
        _ => return None,
    };

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

static SYMBOL_MAP: OnceLock<HashMap<String, String>> = OnceLock::new();

fn load_symbol_map() -> HashMap<String, String> {
    let filename = "ui_settings/symbols.json";
    let candidates = [
        std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(filename))),
        Some(PathBuf::from("/app").join(filename)),
        Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap_or(Path::new(".")).join(filename)),
        Some(PathBuf::from(filename)),
    ];
    for candidate in candidates.iter().flatten() {
        if let Ok(content) = fs::read_to_string(candidate) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
                return map;
            }
        }
    }
    HashMap::new()
}

fn get_symbol_map() -> &'static HashMap<String, String> {
    SYMBOL_MAP.get_or_init(load_symbol_map)
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
