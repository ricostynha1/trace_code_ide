/// Unit tests for search module

#[cfg(test)]
mod tests {
    use super::super::search::*;

    #[test]
    fn search_empty_query_returns_nothing() {
        let files = vec![("file.rs".into(), "fn main() {}".into())];
        let results = search("", &files);
        assert!(results.is_empty());
    }

    #[test]
    fn search_finds_match() {
        let files = vec![("main.rs".into(), "fn hello_world() {\n    println!(\"hello\");\n}".into())];
        let results = search("hello", &files);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_case_insensitive() {
        let files = vec![("lib.rs".into(), "struct MyStruct {}".into())];
        let results = search("mystruct", &files);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn search_ranks_by_relevance() {
        let files = vec![(
            "data.rs".into(),
            "foo foo foo\nbar\nfoo".into(),
        )];
        let results = search("foo", &files);
        // First result should be the line with most occurrences
        assert!(results[0].relevance >= results[1].relevance);
    }
}
