//! Tool selector — picks relevant tools for a given user query using keyword/TF-IDF scoring.
//! Avoids sending all tools to the model on every request.

use super::provider::ToolSchema;
use std::collections::HashMap;

/// Maximum number of tools to include in a single request.
const DEFAULT_MAX_TOOLS: usize = 12;

/// Minimum relevance score to include a tool (0.0 = include everything above zero).
const MIN_SCORE_THRESHOLD: f64 = 0.01;

/// Pre-indexed tool corpus for fast selection.
pub struct ToolIndex {
    tools: Vec<IndexedTool>,
    /// Inverse document frequency per term
    idf: HashMap<String, f64>,
}

struct IndexedTool {
    schema: ToolSchema,
    /// TF-IDF vector for this tool's text (name + description + param names)
    terms: HashMap<String, f64>,
    /// Category tags for boosting
    categories: Vec<String>,
}

impl ToolIndex {
    /// Build index from a set of tool schemas.
    pub fn new(tools: &[ToolSchema]) -> Self {
        let n_docs = tools.len() as f64;

        // Count document frequency per term
        let mut df: HashMap<String, u32> = HashMap::new();
        let tool_tokens: Vec<Vec<String>> = tools.iter().map(|t| {
            let text = tool_text(t);
            tokenize(&text)
        }).collect();

        for tokens in &tool_tokens {
            let unique: std::collections::HashSet<&String> = tokens.iter().collect();
            for term in unique {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }

        // Compute IDF
        let idf: HashMap<String, f64> = df.iter()
            .map(|(term, count)| (term.clone(), (n_docs / (*count as f64 + 1.0)).ln() + 1.0))
            .collect();

        // Build indexed tools with TF-IDF vectors
        let indexed: Vec<IndexedTool> = tools.iter().zip(tool_tokens.iter()).map(|(schema, tokens)| {
            let mut tf: HashMap<String, u32> = HashMap::new();
            for t in tokens {
                *tf.entry(t.clone()).or_insert(0) += 1;
            }
            let max_tf = *tf.values().max().unwrap_or(&1) as f64;
            let terms: HashMap<String, f64> = tf.iter().map(|(term, count)| {
                let tf_norm = *count as f64 / max_tf;
                let idf_val = idf.get(term).copied().unwrap_or(1.0);
                (term.clone(), tf_norm * idf_val)
            }).collect();

            let categories = categorize_tool(&schema.function.name);

            IndexedTool { schema: schema.clone(), terms, categories }
        }).collect();

        Self { tools: indexed, idf }
    }

    /// Select the most relevant tools for a query.
    /// Returns up to `max_tools` tools sorted by relevance.
    pub fn select(&self, query: &str, max_tools: Option<usize>) -> Vec<ToolSchema> {
        let max = max_tools.unwrap_or(DEFAULT_MAX_TOOLS);
        let query_tokens = tokenize(query);

        if query_tokens.is_empty() {
            // No query content — return top tools by category diversity
            return self.tools.iter().take(max).map(|t| t.schema.clone()).collect();
        }

        // Compute query TF-IDF vector
        let mut query_tf: HashMap<String, u32> = HashMap::new();
        for t in &query_tokens {
            *query_tf.entry(t.clone()).or_insert(0) += 1;
        }
        let max_qtf = *query_tf.values().max().unwrap_or(&1) as f64;
        let query_vec: HashMap<String, f64> = query_tf.iter().map(|(term, count)| {
            let tf_norm = *count as f64 / max_qtf;
            let idf_val = self.idf.get(term).copied().unwrap_or(1.0);
            (term.clone(), tf_norm * idf_val)
        }).collect();

        // Score each tool using cosine similarity
        let mut scored: Vec<(f64, usize)> = self.tools.iter().enumerate().map(|(i, tool)| {
            let mut dot = 0.0;
            let mut norm_tool = 0.0;
            let mut norm_query = 0.0;

            for (term, q_weight) in &query_vec {
                let t_weight = tool.terms.get(term).copied().unwrap_or(0.0);
                dot += q_weight * t_weight;
                norm_query += q_weight * q_weight;
            }
            for (_term, t_weight) in &tool.terms {
                norm_tool += t_weight * t_weight;
            }

            let denom = norm_tool.sqrt() * norm_query.sqrt();
            let cosine = if denom > 0.0 { dot / denom } else { 0.0 };

            // Boost: exact name match in query
            let name_boost = if query.to_lowercase().contains(&tool.schema.function.name.to_lowercase().replace('_', " "))
                || query.to_lowercase().contains(&tool.schema.function.name.to_lowercase()) {
                0.5
            } else {
                0.0
            };

            // Boost: category keyword match
            let cat_boost = category_boost(&tool.categories, &query_tokens);

            (cosine + name_boost + cat_boost, i)
        }).collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored.iter()
            .filter(|(score, _)| *score >= MIN_SCORE_THRESHOLD)
            .take(max)
            .map(|(_, i)| self.tools[*i].schema.clone())
            .collect()
    }

    /// Select tools given previous tool results context (for multi-turn).
    /// Includes query relevance + tools that are commonly used together.
    pub fn select_with_context(
        &self,
        query: &str,
        previously_called: &[String],
        max_tools: Option<usize>,
    ) -> Vec<ToolSchema> {
        let max = max_tools.unwrap_or(DEFAULT_MAX_TOOLS);
        let mut selected = self.select(query, Some(max.saturating_sub(2)));

        // Always re-include previously called tools (model may want to call them again)
        for name in previously_called {
            if !selected.iter().any(|t| &t.function.name == name) {
                if let Some(tool) = self.tools.iter().find(|t| &t.schema.function.name == name) {
                    selected.push(tool.schema.clone());
                }
            }
        }

        selected.truncate(max);
        selected
    }
}

/// Extract searchable text from a tool schema.
fn tool_text(schema: &ToolSchema) -> String {
    let mut text = format!("{} {}", schema.function.name.replace('_', " "), schema.function.description);

    // Include parameter names and descriptions
    if let Some(props) = schema.function.parameters.get("properties").and_then(|p| p.as_object()) {
        for (name, val) in props {
            text.push(' ');
            text.push_str(&name.replace('_', " "));
            if let Some(desc) = val.get("description").and_then(|d| d.as_str()) {
                text.push(' ');
                text.push_str(desc);
            }
        }
    }

    text
}

/// Tokenize text into lowercase terms, filtering stopwords.
fn tokenize(text: &str) -> Vec<String> {
    let stopwords: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "shall", "can", "to", "of", "in", "for",
        "on", "with", "at", "by", "from", "as", "into", "through", "during",
        "before", "after", "above", "below", "between", "out", "off", "over",
        "under", "again", "further", "then", "once", "here", "there", "when",
        "where", "why", "how", "all", "each", "every", "both", "few", "more",
        "most", "other", "some", "such", "no", "not", "only", "own", "same",
        "so", "than", "too", "very", "just", "because", "if", "or", "and",
        "but", "this", "that", "these", "those", "it", "its", "i", "me", "my",
        "we", "our", "you", "your", "he", "him", "she", "her", "they", "them",
    ];

    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !stopwords.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Assign category tags based on tool name.
fn categorize_tool(name: &str) -> Vec<String> {
    let mut cats = Vec::new();
    match name {
        "read_file" | "write_file" | "str_replace" | "insert_lines" | "list_files" =>
            cats.push("file".into()),
        "emit_command" =>
            cats.push("command".into()),
        "query_trace_graph" | "query_code_element" =>
            cats.push("trace".into()),
        "list_requirements" =>
            cats.push("requirements".into()),
        "get_symbols" =>
            cats.push("symbols".into()),
        "run_shell" =>
            cats.push("shell".into()),
        "search_files" =>
            cats.push("search".into()),
        _ => {
            // MCP tools — try to infer category from name
            if name.contains("file") || name.contains("read") || name.contains("write") {
                cats.push("file".into());
            }
            if name.contains("search") || name.contains("find") || name.contains("grep") {
                cats.push("search".into());
            }
            if name.contains("run") || name.contains("exec") || name.contains("shell") {
                cats.push("shell".into());
            }
        }
    }
    cats
}

/// Boost score if query mentions category-related keywords.
fn category_boost(categories: &[String], query_tokens: &[String]) -> f64 {
    let category_keywords: &[(&str, &[&str])] = &[
        ("file", &["file", "read", "write", "open", "save", "create", "edit", "modify", "content", "path"]),
        ("search", &["search", "find", "grep", "look", "pattern", "match"]),
        ("shell", &["run", "execute", "command", "shell", "terminal", "build", "test", "compile"]),
        ("trace", &["trace", "requirement", "spec", "link", "traceability"]),
        ("requirements", &["requirement", "req", "requirements", "status"]),
        ("symbols", &["symbol", "function", "class", "struct", "parse", "definition"]),
        ("command", &["undo", "redo", "command", "insert", "delete", "replace"]),
    ];

    let mut boost = 0.0;
    for cat in categories {
        if let Some((_cat_name, keywords)) = category_keywords.iter().find(|(name, _)| name == cat) {
            for kw in *keywords {
                if query_tokens.iter().any(|qt| qt == kw) {
                    boost += 0.2;
                    break; // One match per category is enough
                }
            }
        }
    }
    boost
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::tools::builtin_tool_schemas;

    #[test]
    fn selects_file_tools_for_file_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("read the main.rs file", None);
        let names: Vec<&str> = selected.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"read_file"), "Expected read_file in {:?}", names);
    }

    #[test]
    fn selects_search_tools_for_search_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("find all uses of 'TODO' in the codebase", None);
        let names: Vec<&str> = selected.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"search_files"), "Expected search_files in {:?}", names);
    }

    #[test]
    fn respects_max_tools_limit() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("do everything", Some(3));
        assert!(selected.len() <= 3);
    }

    #[test]
    fn context_includes_previously_called() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select_with_context(
            "now write the result",
            &["read_file".into()],
            Some(5),
        );
        let names: Vec<&str> = selected.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"read_file"), "Should include previously called tool");
    }
}
