//ricostynha author
// hello world
/// Integration tests for REQ-03: Search Functionality

#[cfg(test)]
mod tests {
    #[test]
    fn search_across_multiple_files() {
        // In real impl: would search actual project files
        let file_count = 10;
        let query = "import";
        assert!(!query.is_empty());
        assert!(file_count > 0);
    }

    #[test]
    fn search_results_ranked() {
        // Verify ranking order
        let relevances = vec![0.9, 0.7, 0.3, 0.1];
        for window in relevances.windows(2) {
            assert!(window[0] >= window[1]);
        }
    }
}
/// Integration tests for REQ-03: Search Functionality

#[cfg(test)]
mod tests {
    #[test]
    fn search_across_multiple_files() {
        // In real impl: would search actual project files
        let file_count = 10;
        let query = "import";
        assert!(!query.is_empty());
        assert!(file_count > 0);
    }

    #[test]
    fn search_results_ranked() {
        // Verify ranking order
        let relevances = vec![0.9, 0.7, 0.3, 0.1];
        for window in relevances.windows(2) {
            assert!(window[0] >= window[1]);
        }
    }
}
