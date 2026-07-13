// Search module implementing full-text search across code files with relevance ranking
/// Full-text search module

pub struct SearchResult {
    pub file: String,
    pub line: u32,
    pub snippet: String,
    pub relevance: f64,
}

pub fn search(query: &str, files: &[(String, String)]) -> Vec<SearchResult> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<SearchResult> = Vec::new();
    let query_lower = query.to_lowercase();

    for (filename, content) in files {
        for (i, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                let relevance = compute_relevance(&query_lower, line);
                results.push(SearchResult {
                    file: filename.clone(),
                    line: (i + 1) as u32,
                    snippet: line.to_string(),
                    relevance,
                });
            }
        }
    }

    results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn compute_relevance(query: &str, line: &str) -> f64 {
    let count = line.to_lowercase().matches(query).count();
    count as f64 / line.len().max(1) as f64
}
/// Full-text search module

pub struct SearchResult {
    pub file: String,
    pub line: u32,
    pub snippet: String,
    pub relevance: f64,
}

pub fn search(query: &str, files: &[(String, String)]) -> Vec<SearchResult> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<SearchResult> = Vec::new();
    let query_lower = query.to_lowercase();

    for (filename, content) in files {
        for (i, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&query_lower) {
                let relevance = compute_relevance(&query_lower, line);
                results.push(SearchResult {
                    file: filename.clone(),
                    line: (i + 1) as u32,
                    snippet: line.to_string(),
                    relevance,
                });
            }
        }
    }

    results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn compute_relevance(query: &str, line: &str) -> f64 {
    let count = line.to_lowercase().matches(query).count();
    count as f64 / line.len().max(1) as f64
}I love coding
