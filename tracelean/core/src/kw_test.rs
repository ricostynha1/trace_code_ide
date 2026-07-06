#[cfg(test)]
mod kw_check {
    use tree_sitter::{Language, Query};

    #[test]
    fn check_keywords() {
        let lang: Language = tree_sitter_rust::LANGUAGE.into();
        let keywords = vec![
            "let", "mut", "fn", "pub", "struct", "enum", "impl", "trait", "use",
            "mod", "crate", "self", "super", "where", "as", "in", "for", "while",
            "loop", "if", "else", "match", "return", "break", "continue", "async",
            "await", "move", "ref", "type", "const", "static", "unsafe", "extern", "dyn"
        ];
        
        for kw in &keywords {
            let query_src = format!("\"{}\" @keyword", kw);
            match Query::new(&lang, &query_src) {
                Ok(_) => println!("OK: {}", kw),
                Err(e) => println!("FAIL: {} - {:?}", kw, e),
            }
        }
    }
}
