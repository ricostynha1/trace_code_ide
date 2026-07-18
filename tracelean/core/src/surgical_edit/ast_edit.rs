//! AST Edit — syntax-aware editing via Tree-sitter node operations.
//!
//! Locates nodes by type + name (not line numbers), verifies node type,
//! then applies operations on a shadow buffer. diff_to_commands converts
//! the result to Insert/Delete commands only.

use crate::parser::Lang;
use crate::surgical_edit::error::SurgicalEditError;
use crate::surgical_edit::shadow_diff::diff_to_commands;
use crate::surgical_edit::EditResult;
use std::path::PathBuf;
use tree_sitter::{Node, Parser, Tree};

/// AST edit operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstEditOp {
    /// Insert new source text as a sibling after the matched node.
    InsertAfter { new_text: String },
    /// Insert new source text as a sibling before the matched node.
    InsertBefore { new_text: String },
    /// Delete the matched node entirely.
    DeleteNode,
    /// Replace the matched node's text with new content.
    ReplaceNode { new_text: String },
    /// Move matched node to after target (identified by a second NodeSelector).
    MoveAfter { target: NodeSelector },
    /// Wrap the matched node with prefix/suffix text.
    WrapNode { prefix: String, suffix: String },
    /// Unwrap: replace matched node with its children's text (remove wrapper).
    UnwrapNode,
}

/// How to locate a node in the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSelector {
    /// Tree-sitter node type (e.g., "function_item", "impl_item", "struct_item")
    pub node_type: String,
    /// Name identifier to match (e.g., function name, struct name).
    /// If None, matches by node_type alone (must be unique).
    pub name: Option<String>,
    /// Optional: restrict to Nth occurrence (0-indexed) if multiple exist.
    pub index: Option<usize>,
}

/// A surgical AST edit request.
#[derive(Debug, Clone)]
pub struct AstEdit {
    pub selector: NodeSelector,
    pub operation: AstEditOp,
}

impl AstEdit {
    pub fn new(selector: NodeSelector, operation: AstEditOp) -> Self {
        Self { selector, operation }
    }

    /// Apply this AST edit to the given source content.
    /// Produces shadow buffer, then diffs to Insert/Delete commands.
    pub fn apply(
        &self,
        file: &PathBuf,
        source: &str,
        lang: &Lang,
    ) -> Result<EditResult, SurgicalEditError> {
        let tree = parse_source(source, lang)?;
        let root = tree.root_node();

        let matches = find_nodes(root, source, &self.selector);
        let node = select_unique_node(&matches, &self.selector)?;

        let start_byte = node.start_byte();
        let end_byte = node.end_byte();
        let node_text = &source[start_byte..end_byte];

        // Build shadow buffer with the edit applied
        let shadow = match &self.operation {
            AstEditOp::InsertAfter { new_text } => {
                let mut s = String::with_capacity(source.len() + new_text.len() + 1);
                s.push_str(&source[..end_byte]);
                if !new_text.starts_with('\n') && !source[..end_byte].ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(new_text);
                s.push_str(&source[end_byte..]);
                s
            }
            AstEditOp::InsertBefore { new_text } => {
                let mut s = String::with_capacity(source.len() + new_text.len() + 1);
                s.push_str(&source[..start_byte]);
                s.push_str(new_text);
                if !new_text.ends_with('\n') && !source[start_byte..].starts_with('\n') {
                    s.push('\n');
                }
                s.push_str(&source[start_byte..]);
                s
            }
            AstEditOp::DeleteNode => {
                let mut s = String::with_capacity(source.len());
                s.push_str(&source[..start_byte]);
                let skip_end = if end_byte < source.len() && source.as_bytes()[end_byte] == b'\n' {
                    end_byte + 1
                } else {
                    end_byte
                };
                s.push_str(&source[skip_end..]);
                s
            }
            AstEditOp::ReplaceNode { new_text } => {
                let mut s = String::with_capacity(source.len() - (end_byte - start_byte) + new_text.len());
                s.push_str(&source[..start_byte]);
                s.push_str(new_text);
                s.push_str(&source[end_byte..]);
                s
            }
            AstEditOp::MoveAfter { target } => {
                let target_matches = find_nodes(root, source, target);
                let target_node = select_unique_node(&target_matches, target)?;
                let target_end = target_node.end_byte();

                let extracted = node_text.to_string();
                let mut temp = String::with_capacity(source.len());
                temp.push_str(&source[..start_byte]);
                let skip_end = if end_byte < source.len() && source.as_bytes()[end_byte] == b'\n' {
                    end_byte + 1
                } else {
                    end_byte
                };
                temp.push_str(&source[skip_end..]);

                let adjusted_target_end = if target_end > end_byte {
                    target_end - (skip_end - start_byte)
                } else {
                    target_end
                };

                let mut s = String::with_capacity(temp.len() + extracted.len() + 1);
                s.push_str(&temp[..adjusted_target_end]);
                if !temp[..adjusted_target_end].ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&extracted);
                s.push_str(&temp[adjusted_target_end..]);
                s
            }
            AstEditOp::WrapNode { prefix, suffix } => {
                let mut s = String::with_capacity(source.len() + prefix.len() + suffix.len());
                s.push_str(&source[..start_byte]);
                s.push_str(prefix);
                s.push_str(node_text);
                s.push_str(suffix);
                s.push_str(&source[end_byte..]);
                s
            }
            AstEditOp::UnwrapNode => {
                let inner = extract_inner_content(node, source);
                let mut s = String::with_capacity(source.len());
                s.push_str(&source[..start_byte]);
                s.push_str(&inner);
                s.push_str(&source[end_byte..]);
                s
            }
        };

        // Diff original → shadow → Insert/Delete commands
        let commands = diff_to_commands(file, source, &shadow);

        Ok(EditResult {
            file: file.clone(),
            commands,
            new_content: shadow,
        })
    }
}

// --- Internal helpers ---

fn parse_source(source: &str, lang: &Lang) -> Result<Tree, SurgicalEditError> {
    let mut parser = Parser::new();
    parser
        .set_language(&lang.tree_sitter_language)
        .map_err(|e| SurgicalEditError::ParseError(e.to_string()))?;
    parser
        .parse(source, None)
        .ok_or_else(|| SurgicalEditError::ParseError("Tree-sitter parse returned None".into()))
}

fn find_nodes<'a>(root: Node<'a>, source: &'a str, selector: &NodeSelector) -> Vec<Node<'a>> {
    let mut results = Vec::new();
    collect_matching_nodes(root, source, selector, &mut results);
    results
}

fn collect_matching_nodes<'a>(
    node: Node<'a>,
    source: &'a str,
    selector: &NodeSelector,
    results: &mut Vec<Node<'a>>,
) {
    if node.kind() == selector.node_type {
        if let Some(ref name) = selector.name {
            if node_has_name(node, source, name) {
                results.push(node);
            }
        } else {
            results.push(node);
        }
    }
    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            collect_matching_nodes(child, source, selector, results);
        }
    }
}

fn node_has_name(node: Node, source: &str, name: &str) -> bool {
    if let Some(name_node) = node.child_by_field_name("name") {
        let text = &source[name_node.start_byte()..name_node.end_byte()];
        return text == name;
    }
    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            if child.kind() == "identifier" || child.kind() == "type_identifier" {
                let text = &source[child.start_byte()..child.end_byte()];
                if text == name {
                    return true;
                }
            }
        }
    }
    false
}

fn select_unique_node<'a>(
    matches: &[Node<'a>],
    selector: &NodeSelector,
) -> Result<Node<'a>, SurgicalEditError> {
    if let Some(idx) = selector.index {
        matches.get(idx).copied().ok_or_else(|| {
            SurgicalEditError::NodeNotFound(format!(
                "{}[{}] (found {} matches)",
                selector.node_type, idx, matches.len()
            ))
        })
    } else {
        match matches.len() {
            0 => Err(SurgicalEditError::NodeNotFound(format!(
                "{} {:?}",
                selector.node_type, selector.name
            ))),
            1 => Ok(matches[0]),
            n => Err(SurgicalEditError::AmbiguousNode {
                description: format!("{} {:?}", selector.node_type, selector.name),
                count: n,
            }),
        }
    }
}

fn extract_inner_content(node: Node, source: &str) -> String {
    let child_count = node.child_count();
    if child_count < 3 {
        return source[node.start_byte()..node.end_byte()].to_string();
    }
    let first_content = node.child(1).unwrap();
    let last_content = node.child(child_count - 2).unwrap();
    source[first_content.start_byte()..last_content.end_byte()].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command;

    fn rust_lang() -> Lang {
        Lang::from_extension("rs").unwrap()
    }

    fn assert_only_replace(commands: &[Command]) {
        for cmd in commands {
            match cmd {
                Command::Replace { .. } => {}
                other => panic!("Expected only Replace, got: {:?}", other),
            }
        }
    }

    #[test]
    fn test_insert_after_function() {
        let source = "fn hello() {\n    println!(\"hello\");\n}\n\nfn world() {\n    println!(\"world\");\n}\n";
        let edit = AstEdit::new(
            NodeSelector { node_type: "function_item".into(), name: Some("hello".into()), index: None },
            AstEditOp::InsertAfter { new_text: "\nfn inserted() {}\n".into() },
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source, &rust_lang()).unwrap();
        assert_only_replace(&result.commands);
        assert!(result.new_content.contains("fn inserted() {}"));
        let hello_pos = result.new_content.find("fn hello").unwrap();
        let inserted_pos = result.new_content.find("fn inserted").unwrap();
        let world_pos = result.new_content.find("fn world").unwrap();
        assert!(hello_pos < inserted_pos && inserted_pos < world_pos);
    }

    #[test]
    fn test_delete_function() {
        let source = "fn keep_me() {}\nfn delete_me() {\n    let x = 42;\n}\nfn also_keep() {}\n";
        let edit = AstEdit::new(
            NodeSelector { node_type: "function_item".into(), name: Some("delete_me".into()), index: None },
            AstEditOp::DeleteNode,
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source, &rust_lang()).unwrap();
        assert_only_replace(&result.commands);
        assert!(!result.new_content.contains("delete_me"));
        assert!(result.new_content.contains("keep_me"));
        assert!(result.new_content.contains("also_keep"));
    }

    #[test]
    fn test_replace_function_body() {
        let source = "fn target() {\n    old_code();\n}\n";
        let edit = AstEdit::new(
            NodeSelector { node_type: "function_item".into(), name: Some("target".into()), index: None },
            AstEditOp::ReplaceNode { new_text: "fn target() {\n    new_code();\n}".into() },
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source, &rust_lang()).unwrap();
        assert_only_replace(&result.commands);
        assert!(result.new_content.contains("new_code()"));
        assert!(!result.new_content.contains("old_code()"));
    }

    #[test]
    fn test_node_not_found() {
        let source = "fn existing() {}";
        let edit = AstEdit::new(
            NodeSelector { node_type: "function_item".into(), name: Some("nonexistent".into()), index: None },
            AstEditOp::DeleteNode,
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source, &rust_lang());
        assert!(matches!(result, Err(SurgicalEditError::NodeNotFound(_))));
    }

    #[test]
    fn test_ambiguous_node() {
        let source = "fn dup() {}\nfn dup() {}\n";
        let edit = AstEdit::new(
            NodeSelector { node_type: "function_item".into(), name: Some("dup".into()), index: None },
            AstEditOp::DeleteNode,
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source, &rust_lang());
        assert!(matches!(result, Err(SurgicalEditError::AmbiguousNode { .. })));
    }

    #[test]
    fn test_index_selector_disambiguates() {
        let source = "fn dup() { /* first */ }\nfn dup() { /* second */ }\n";
        let edit = AstEdit::new(
            NodeSelector { node_type: "function_item".into(), name: Some("dup".into()), index: Some(1) },
            AstEditOp::DeleteNode,
        );
        let result = edit.apply(&PathBuf::from("test.rs"), source, &rust_lang()).unwrap();
        assert_only_replace(&result.commands);
        assert!(result.new_content.contains("first"));
        assert!(!result.new_content.contains("second"));
    }
}
