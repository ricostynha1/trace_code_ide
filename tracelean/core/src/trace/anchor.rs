//! Resolving an annotation to a stable anchor in the code.
//!
//! Anchors are symbol *paths*, never line numbers: line numbers rot on the
//! first edit above them, while `src/auth.rs::AuthService::login` survives
//! reordering, reindentation and moving a function within its file.
//!
//! Resolution walks the tree-sitter tree directly rather than going through
//! `parser::SymbolTable`, because that table only extracts top-level items —
//! a method inside an `impl` or a class would otherwise find no symbol at all
//! and silently degrade to a whole-file anchor.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use super::annotation::{RawAnnotation, ScanResult};
use super::hash;
use crate::parser::Lang;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AnchorKind {
    /// A named declaration, addressed by its path within the file.
    Decl { symbol_path: String },
    /// An explicit `begin` … `@end` byte range.
    Region { start: usize, end: usize },
    /// The whole file — used when nothing named follows the annotation, and
    /// always for files whose language has no grammar.
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    pub file: PathBuf,
    pub kind: AnchorKind,
    /// Hash of the normalized body, with comment text removed.
    pub body_hash: String,
    pub start_line: u32,
    pub end_line: u32,
    /// False when the file had no grammar, so the anchor is the whole file and
    /// links through it are capped at L1.
    pub precise: bool,
}

impl Anchor {
    /// Stable identity string, used in the link hash and in the lockfile.
    pub fn ident(&self) -> String {
        match &self.kind {
            AnchorKind::Decl { symbol_path } => format!("{}::{}", self.file.display(), symbol_path),
            AnchorKind::Region { start, end } => {
                format!("{}@region[{start},{end}]", self.file.display())
            }
            AnchorKind::File => format!("{}@file", self.file.display()),
        }
    }

    pub fn symbol_path(&self) -> Option<&str> {
        match &self.kind {
            AnchorKind::Decl { symbol_path } => Some(symbol_path),
            _ => None,
        }
    }

    /// Does this anchor's line span contain `line` (zero-based)?
    /// Used to answer "what is traced at the cursor".
    pub fn covers_line(&self, line: usize) -> bool {
        let line = line as u32;
        line >= self.start_line && line <= self.end_line
    }

    /// Number of lines covered. Smaller = more specific, which is the order a
    /// cursor menu wants: the innermost annotation is the one being asked about.
    pub fn span_len(&self) -> u32 {
        self.end_line.saturating_sub(self.start_line)
    }
}

/// Node kinds that introduce a named scope worth putting in a symbol path.
const DECL_KINDS: &[&str] = &[
    // rust
    "function_item", "struct_item", "enum_item", "trait_item", "impl_item", "mod_item",
    "type_item", "const_item", "static_item", "macro_definition", "union_item",
    // python
    "function_definition", "class_definition",
    // c / c++
    "function_definition", "class_specifier", "struct_specifier", "namespace_definition",
    "enum_specifier",
    // lean 4 (tree-sitter-lean4): `def`/`theorem`/`abbrev`/`instance` all parse
    // to a named `definition` node whose first child is the anonymous keyword
    // token. The keyword kinds are deliberately absent here -- listing them
    // matched that inner token instead, so every Lean anchor resolved to the
    // symbol path "def" with a one-line span.
    "definition", "structure", "inductive", "example",
    "namespace", "section", "opaque", "axiom",
    // javascript
    "function_declaration", "class_declaration", "method_definition",
    "lexical_declaration", "variable_declaration",
];

/// Is this node a declaration?
///
/// The named check is load-bearing for Lean, whose grammar gives the keyword
/// token inside `structure Cart where` the kind `"structure"` -- the same
/// string as the declaration node containing it. Without `is_named` the
/// keyword wins (it starts first), and the anchor becomes the word rather than
/// the thing it introduces.
fn is_declaration(node: Node) -> bool {
    node.is_named() && DECL_KINDS.contains(&node.kind())
}

/// Wrapper nodes that must be unwrapped to reach the real declaration.
///
/// Without this, the attribute/decorator case — `@[simp] def f` in Lean, a
/// decorated Python function — yields no anchor at all, which is precisely the
/// case annotations are most often written above.
const WRAPPER_KINDS: &[&str] = &["declaration", "decorated_definition", "attribute_item", "export_statement"];

/// Resolve every annotation in a scan result to an anchor.
pub fn resolve_all(path: &Path, root: &Path, content: &str, scan: &ScanResult) -> Vec<(RawAnnotation, Anchor)> {
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    let tree = if scan.precise { tree_for(path, content) } else { None };

    scan.annotations
        .iter()
        .map(|ann| {
            let anchor = resolve_one(&rel, content, ann, tree.as_ref(), scan);
            (ann.clone(), anchor)
        })
        .collect()
}

fn tree_for(path: &Path, content: &str) -> Option<tree_sitter::Tree> {
    let ext = path.extension()?.to_str()?;
    let lang = Lang::from_extension(ext)?;
    let mut parser = Parser::new();
    parser.set_language(&lang.tree_sitter_language).ok()?;
    parser.parse(content, None)
}

fn resolve_one(
    rel: &Path,
    content: &str,
    ann: &RawAnnotation,
    tree: Option<&tree_sitter::Tree>,
    scan: &ScanResult,
) -> Anchor {
    // An explicit region always wins — the author said exactly what they meant.
    if let Some((start, end)) = ann.region {
        let body = normalized_slice(content, start, end, scan);
        return Anchor {
            file: rel.to_path_buf(),
            kind: AnchorKind::Region { start, end },
            body_hash: hash::hash_body(&body),
            start_line: line_of(content, start),
            end_line: line_of(content, end),
            precise: scan.precise,
        };
    }

    if let Some(tree) = tree {
        if let Some((node, path)) = next_declaration(tree.root_node(), content, ann.comment_end) {
            let body = normalized_slice(content, node.start_byte(), node.end_byte(), scan);
            return Anchor {
                file: rel.to_path_buf(),
                kind: AnchorKind::Decl { symbol_path: path },
                body_hash: hash::hash_body(&body),
                start_line: node.start_position().row as u32,
                end_line: node.end_position().row as u32,
                precise: true,
            };
        }
    }

    // Nothing named follows: the annotation describes the file.
    let body = normalized_slice(content, 0, content.len(), scan);
    Anchor {
        file: rel.to_path_buf(),
        kind: AnchorKind::File,
        body_hash: hash::hash_body(&body),
        start_line: 0,
        end_line: line_of(content, content.len()),
        precise: scan.precise,
    }
}

/// Find the first declaration beginning at or after `byte`, returning it with
/// its `::`-joined path of enclosing named scopes.
///
/// There is deliberately no line window: blank lines, attributes, decorators
/// and further comment lines between the annotation and the declaration are
/// skipped by construction, and a link is only rejected when nothing named
/// follows at all.
fn next_declaration<'t>(root: Node<'t>, content: &str, byte: usize) -> Option<(Node<'t>, String)> {
    let mut best: Option<(Node<'t>, String)> = None;
    walk_decls(root, content, byte, &mut Vec::new(), &mut best);
    best
}

fn walk_decls<'t>(
    node: Node<'t>,
    content: &str,
    byte: usize,
    scope: &mut Vec<String>,
    best: &mut Option<(Node<'t>, String)>,
) {
    // Prune: nothing inside a subtree that ends before `byte` can qualify.
    if node.end_byte() < byte {
        return;
    }
    // Prune: once we have a candidate, anything starting later is worse.
    if let Some((found, _)) = best {
        if node.start_byte() > found.start_byte() {
            return;
        }
    }

    let unwrapped = unwrap_declaration(node);
    let is_decl = is_declaration(unwrapped);

    if is_decl && unwrapped.start_byte() >= byte {
        let name = declaration_name(unwrapped, content).unwrap_or_else(|| unwrapped.kind().to_string());
        let mut path = scope.clone();
        path.push(name);
        let candidate = path.join("::");
        let better = match best {
            Some((found, _)) => unwrapped.start_byte() < found.start_byte(),
            None => true,
        };
        if better {
            *best = Some((unwrapped, candidate));
        }
        // Keep descending: a nested declaration may start even earlier only if
        // this one encloses `byte`, which the child loop below handles.
    }

    let pushed = if is_decl {
        match declaration_name(unwrapped, content) {
            Some(name) => {
                scope.push(name);
                true
            }
            None => false,
        }
    } else {
        false
    };

    let mut cursor = unwrapped.walk();
    for child in unwrapped.children(&mut cursor) {
        walk_decls(child, content, byte, scope, best);
    }

    if pushed {
        scope.pop();
    }
}

/// Unwrap `@[simp] def f` (lean `declaration`) and decorated Python
/// definitions to the declaration they contain.
fn unwrap_declaration(node: Node) -> Node {
    if !WRAPPER_KINDS.contains(&node.kind()) {
        return node;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_declaration(child) {
            return child;
        }
    }
    node
}

/// Best-effort name of a declaration: its `name` field where the grammar has
/// one, otherwise the first identifier-ish child.
fn declaration_name(node: Node, content: &str) -> Option<String> {
    if let Some(name) = node.child_by_field_name("name") {
        return slice(content, name);
    }
    // Rust `impl Foo for Bar` has `type`/`trait` fields rather than `name`.
    if let Some(ty) = node.child_by_field_name("type") {
        let base = slice(content, ty)?;
        return Some(match node.child_by_field_name("trait").and_then(|t| slice(content, t)) {
            Some(tr) => format!("impl {tr} for {base}"),
            None => format!("impl {base}"),
        });
    }
    if let Some(decl) = node.child_by_field_name("declarator") {
        // C/C++ function definitions nest the name under the declarator.
        if let Some(inner) = decl.child_by_field_name("declarator") {
            return slice(content, inner);
        }
        return slice(content, decl);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let k = child.kind();
        if k == "identifier" || k.ends_with("_identifier") || k == "type_identifier" {
            return slice(content, child);
        }
    }
    None
}

fn slice(content: &str, node: Node) -> Option<String> {
    content
        .get(node.start_byte()..node.end_byte())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Normalize a byte range for hashing: drop comment ranges that fall inside it,
/// protect string and character literals, collapse the rest of the whitespace.
fn normalized_slice(content: &str, start: usize, end: usize, scan: &ScanResult) -> String {
    let start = start.min(content.len());
    let end = end.min(content.len()).max(start);
    let src = &content[start..end];

    let shift = |ranges: &[(usize, usize)]| -> Vec<(usize, usize)> {
        ranges
            .iter()
            .filter(|(s, e)| *e > start && *s < end)
            .map(|(s, e)| (s.saturating_sub(start).min(src.len()), (*e).min(end) - start))
            .collect()
    };

    hash::normalize(src, &shift(&scan.literal_ranges), &shift(&scan.comment_ranges))
}

fn line_of(content: &str, byte: usize) -> u32 {
    content[..byte.min(content.len())].matches('\n').count() as u32
}

// --- the symbol table, for the project graph -----------------------------

/// One top-level declaration in a file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Declaration {
    pub name: String,
    /// Grammar node kind, e.g. `function_item`, `definition`, `class_definition`.
    pub kind: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// The top-level declarations in a source file.
///
/// Top-level only, and deliberately: the project graph needs one node per thing
/// a person would point at, and a file's every nested closure is not that. It
/// is also the difference between a graph that opens and one that hangs — a
/// real repository has tens of thousands of nested nodes and a few thousand
/// top-level ones.
///
/// Declarations are what makes the graph's grey honest. A file with one
/// annotated function and nine unannotated ones should not look traced, and it
/// cannot look untraced either; it is one node with nine grey children.
pub fn declarations_in(path: &Path, content: &str) -> Vec<Declaration> {
    let Some(tree) = tree_for(path, content) else { return Vec::new() };
    let mut out = Vec::new();
    collect_declarations(tree.root_node(), content, &mut out);
    out
}

fn collect_declarations(node: Node, content: &str, out: &mut Vec<Declaration>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let unwrapped = unwrap_declaration(child);
        if is_declaration(unwrapped) {
            // A Lean `namespace Foo` node spans only its own line, so it is not
            // a container to descend into -- the declarations it scopes are its
            // siblings. Recording it would add a node for a line of syntax.
            if unwrapped.kind() == "namespace" || unwrapped.kind() == "section" {
                continue;
            }
            out.push(Declaration {
                name: declaration_name(unwrapped, content)
                    .unwrap_or_else(|| unwrapped.kind().to_string()),
                kind: unwrapped.kind().to_string(),
                start_line: unwrapped.start_position().row as u32,
                end_line: unwrapped.end_position().row as u32,
            });
            continue;
        }
        collect_declarations(child, content, out);
    }
}
