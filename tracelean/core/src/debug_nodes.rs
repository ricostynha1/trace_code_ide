/// Debug/integration tests for tree-sitter highlighting.
/// Run with: cargo test --lib debug_nodes -- --nocapture

#[cfg(test)]
mod tests {
    use crate::parser::{all_languages, get_highlights_query};
    use std::path::Path;
    use tree_sitter::Parser;

    fn dump_tree(source: &str, lang: tree_sitter::Language, label: &str) {
        let mut parser = Parser::new();
        parser.set_language(&lang).unwrap();
        let tree = parser.parse(source, None).unwrap();
        println!("\n=== {} ===", label);
        dump_node(&tree.root_node(), source, 0);
    }

    fn dump_node(node: &tree_sitter::Node, source: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        let text = node.utf8_text(source.as_bytes()).unwrap_or("");
        let short = if text.len() > 40 { &text[..40] } else { text };
        println!(
            "{}kind={:?} named={} children={} [{}-{}] text={:?}",
            indent, node.kind(), node.is_named(), node.child_count(),
            node.start_byte(), node.end_byte(), short
        );
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                dump_node(&cursor.node(), source, depth + 1);
                if !cursor.goto_next_sibling() { break; }
            }
        }
    }

    #[test]
    fn debug_dump_lean() {
        let src = "def hello : Nat := 42\n\ntheorem foo : True := by\n  trivial\n";
        dump_tree(src, tree_sitter_lean4::language().into(), "LEAN4");
    }

    #[test]
    fn debug_dump_markdown() {
        let src = "# Hello World\n\nThis is **bold** and *italic*.\n\n- item 1\n- item 2\n";
        dump_tree(src, tree_sitter_md::LANGUAGE.into(), "MARKDOWN BLOCK");
        dump_tree(src, tree_sitter_md::INLINE_LANGUAGE.into(), "MARKDOWN INLINE");
    }

    #[test]
    fn debug_dump_rust() {
        let src = "fn main() {\n    let x = 42;\n    println!(\"hello\");\n}\n";
        dump_tree(src, tree_sitter_rust::LANGUAGE.into(), "RUST");
    }

    /// Verify all .scm query files compile against their grammar.
    #[test]
    fn all_queries_compile() {
        let langs = all_languages();
        let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().to_path_buf();

        let mut failures = Vec::new();

        for lang in &langs {
            let path = parent.join(format!("grammars/{}/highlights.scm", lang.name));
            if !path.exists() { continue; }

            let content = std::fs::read_to_string(&path).unwrap();
            if let Err(e) = tree_sitter::Query::new(&lang.tree_sitter_language, &content) {
                failures.push(format!("{}: {:?}", lang.name, e));
            }
        }

        // Also check markdown_inline
        let inline_path = parent.join("grammars/markdown/highlights_inline.scm");
        if inline_path.exists() {
            let content = std::fs::read_to_string(&inline_path).unwrap();
            let inline_lang: tree_sitter::Language = tree_sitter_md::INLINE_LANGUAGE.into();
            if let Err(e) = tree_sitter::Query::new(&inline_lang, &content) {
                failures.push(format!("markdown_inline: {:?}", e));
            }
        }

        assert!(failures.is_empty(), "Query compilation failures:\n{}", failures.join("\n"));
    }

    #[test]
    fn test_highlights_rust() {
        let src = "fn main() {\n    let x = 42;\n}\n";
        let spans = get_highlights_query(Path::new("test.rs"), src);
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "fn" && s.color == "#c678dd"),
            "Expected 'fn' keyword colored");
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "let" && s.color == "#c678dd"),
            "Expected 'let' keyword colored");
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "42" && s.color == "#d19a66"),
            "Expected '42' number colored");
    }

    #[test]
    fn test_highlights_lean() {
        let src = "def hello : Nat := 42\n";
        let spans = get_highlights_query(Path::new("test.lean"), src);
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "def" && s.color == "#c678dd"),
            "Expected 'def' keyword colored");
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "Nat" && s.color == "#e5c07b"),
            "Expected 'Nat' type colored");
        assert!(spans.iter().any(|s| &src[s.from..s.to] == ":=" && s.color == "#56b6c2"),
            "Expected ':=' operator colored");
    }

    #[test]
    fn test_highlights_markdown() {
        let src = "# Hello World\n\nSome **bold** text.\n";
        let spans = get_highlights_query(Path::new("test.md"), src);
        assert!(!spans.is_empty(), "Expected markdown to produce highlight spans");
    }
}
