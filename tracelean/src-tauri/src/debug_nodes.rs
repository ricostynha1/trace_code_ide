/// Temporary debug: dump tree-sitter node kinds for a given source.
/// Run with: cargo test --lib debug_dump -- --nocapture

#[cfg(test)]
mod tests {
    use tree_sitter::Parser;
    use crate::parser::{get_highlights, HighlightSpan};
    use std::path::Path;

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
            indent,
            node.kind(),
            node.is_named(),
            node.child_count(),
            node.start_byte(),
            node.end_byte(),
            short
        );
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                dump_node(&cursor.node(), source, depth + 1);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    #[test]
    fn debug_dump_lean() {
        let src = r#"def hello : Nat := 42

theorem foo : True := by
  trivial
"#;
        dump_tree(src, tree_sitter_lean4::language().into(), "LEAN4");
    }

    #[test]
    fn debug_dump_markdown() {
        let src = r#"# Hello World

This is **bold** and *italic*.

- item 1
- item 2
"#;
        dump_tree(src, tree_sitter_md::LANGUAGE.into(), "MARKDOWN BLOCK");
        dump_tree(src, tree_sitter_md::INLINE_LANGUAGE.into(), "MARKDOWN INLINE");
    }

    #[test]
    fn debug_dump_rust() {
        let src = r#"fn main() {
    let x = 42;
    println!("hello");
}
"#;
        dump_tree(src, tree_sitter_rust::LANGUAGE.into(), "RUST");
    }

    #[test]
    fn test_highlights_lean() {
        let src = "def hello : Nat := 42\n";
        let spans = get_highlights(Path::new("test.lean"), src);
        println!("\n=== LEAN HIGHLIGHTS ===");
        for s in &spans {
            let text = &src[s.from..s.to];
            println!("  [{}-{}] {:?} color={}", s.from, s.to, text, s.color);
        }
        // "def" should be colored as keyword
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "def" && s.color == "#c678dd"),
            "Expected 'def' to be colored as keyword");
        // "Nat" should be colored as type
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "Nat" && s.color == "#e5c07b"),
            "Expected 'Nat' to be colored as type");
        // ":=" should be colored as operator
        assert!(spans.iter().any(|s| &src[s.from..s.to] == ":=" && s.color == "#56b6c2"),
            "Expected ':=' to be colored as operator");
    }

    #[test]
    fn test_highlights_markdown() {
        let src = "# Hello World\n\nSome **bold** text.\n";
        let spans = get_highlights(Path::new("test.md"), src);
        println!("\n=== MARKDOWN HIGHLIGHTS ===");
        for s in &spans {
            let text = &src[s.from..s.to];
            println!("  [{}-{}] {:?} color={}", s.from, s.to, text, s.color);
        }
        // Should have at least some spans
        assert!(!spans.is_empty(), "Expected markdown to produce highlight spans");
        // Heading marker "#" should be colored
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "#" && s.color == "#e06c75"),
            "Expected '#' heading marker colored");
    }

    #[test]
    fn test_highlights_rust() {
        let src = "fn main() {\n    let x = 42;\n}\n";
        let spans = get_highlights(Path::new("test.rs"), src);
        println!("\n=== RUST HIGHLIGHTS ===");
        for s in &spans {
            let text = &src[s.from..s.to];
            println!("  [{}-{}] {:?} color={}", s.from, s.to, text, s.color);
        }
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "fn" && s.color == "#c678dd"),
            "Expected 'fn' keyword colored");
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "let" && s.color == "#c678dd"),
            "Expected 'let' keyword colored");
        assert!(spans.iter().any(|s| &src[s.from..s.to] == "42" && s.color == "#d19a66"),
            "Expected '42' number colored");
    }
}
