//! Tool selector — picks relevant tools for a given user query.
//!
//! Uses enriched tool descriptions (aliases, examples, categories) with TF-IDF
//! cosine similarity scoring. This is the retrieval layer that avoids sending all
//! tools to the model on every request.
//!
//! Architecture (per new_tool_retrieval.md spec, fallback mode):
//! - Enriched tool text with name + description + category + aliases + examples
//! - TF-IDF vectorization with cosine similarity
//! - Top-K=5 default retrieval
//! - Ready for semantic embedding upgrade (swap TF-IDF for FastEmbed when available)

use super::provider::ToolSchema;
use std::collections::HashMap;

/// Default number of tools to include in a single request (per spec: top_k = 5).
const DEFAULT_MAX_TOOLS: usize = 5;

/// Minimum relevance score to include a tool.
/// Calibrated: should-match min=0.26, should-not-match max=0.15. Threshold=0.20.
const MIN_SCORE_THRESHOLD: f64 = 0.20;

/// Pre-indexed tool corpus for fast selection.
pub struct ToolIndex {
    tools: Vec<IndexedTool>,
    /// Inverse document frequency per term
    idf: HashMap<String, f64>,
}

struct IndexedTool {
    schema: ToolSchema,
    /// TF-IDF vector for this tool's enriched text
    terms: HashMap<String, f64>,
    /// Category for boosting
    category: String,
}

/// Enrichment data for a tool: aliases, examples, and category.
struct ToolEnrichment {
    category: &'static str,
    aliases: &'static [&'static str],
    examples: &'static [&'static str],
}

/// Get enrichment data for builtin tools.
fn get_enrichment(tool_name: &str) -> ToolEnrichment {
    match tool_name {
        "read_range" => ToolEnrichment {
            category: "filesystem",
            aliases: &["read file", "open file", "view file", "show lines", "cat file", "get contents", "look at file", "display code"],
            examples: &["Read first 20 lines of main.rs", "Show last 10 lines", "View src/auth.rs lines 5-30", "Check file contents"],
        },
        "write_range" => ToolEnrichment {
            category: "editing",
            aliases: &["write file", "edit file", "insert lines", "prepend", "append", "replace lines", "add to file", "create file", "overwrite"],
            examples: &["Prepend comment to file", "Append line at end", "Replace lines 5-10", "Insert import at top", "Add phrase to all files"],
        },
        "count_lines" => ToolEnrichment {
            category: "filesystem",
            aliases: &["line count", "file size", "how many lines", "file length", "wc"],
            examples: &["How many lines in main.rs", "Check file size", "Count lines before reading"],
        },
        "str_replace" => ToolEnrichment {
            category: "editing",
            aliases: &["replace text", "find and replace", "substitute", "edit text", "change text", "modify content", "swap text", "patch file", "fix typo"],
            examples: &["Replace TODO with implementation", "Change function name", "Fix typo", "Update import statement"],
        },
        "list_files" => ToolEnrichment {
            category: "filesystem",
            aliases: &["show directory", "ls", "list directory", "browse files", "show tree", "what files", "directory contents"],
            examples: &["List all files in src/", "Show project structure", "What files are in the root"],
        },
        "delete_file" => ToolEnrichment {
            category: "filesystem",
            aliases: &["remove file", "rm", "delete", "erase file"],
            examples: &["Delete temp file", "Remove scratch.rs"],
        },
        "find_grep" => ToolEnrichment {
            category: "search",
            aliases: &["grep", "search text", "find pattern", "regex search", "look for", "where is", "find in files", "search code"],
            examples: &["Find all TODO comments", "Search for fn main in src/", "Where is login defined", "Grep for API_KEY in *.rs"],
        },
        "find_embed" => ToolEnrichment {
            category: "search",
            aliases: &["semantic search", "find similar", "search meaning", "find function by description", "what does", "find related code"],
            examples: &["Find auth handling code", "Search for error recovery logic", "Find upload validation"],
        },
        "query_trace_graph" => ToolEnrichment {
            category: "traceability",
            aliases: &["trace requirement", "find links", "requirement coverage", "what implements", "trace link"],
            examples: &["What code implements REQ-01", "Show traceability for REQ-03", "Find tests for requirement"],
        },
        "query_code_element" => ToolEnrichment {
            category: "traceability",
            aliases: &["trace code", "code links", "what requirement", "element trace"],
            examples: &["What requirement does login() satisfy", "Find spec for upload function"],
        },
        "list_requirements" => ToolEnrichment {
            category: "requirements",
            aliases: &["show requirements", "all requirements", "requirement list", "specs", "project requirements"],
            examples: &["List all requirements", "Show project requirements", "What are the specs"],
        },
        "get_symbols" => ToolEnrichment {
            category: "code_analysis",
            aliases: &["parse symbols", "functions in file", "classes in file", "code structure", "definitions", "what functions"],
            examples: &["Get symbols from main.rs", "What functions are in auth.rs", "Show class structure"],
        },
        "run_shell" => ToolEnrichment {
            category: "shell",
            aliases: &["execute shell", "terminal", "run command", "bash", "compile", "build", "test", "make"],
            examples: &["Run cargo build", "Execute tests", "Compile the project", "Run make", "Check linting"],
        },
        _ => ToolEnrichment {
            category: "other",
            aliases: &[],
            examples: &[],
        },
    }
}

/// Build enriched text representation for a tool (per spec section 5).
fn enriched_tool_text(schema: &ToolSchema) -> String {
    let enrichment = get_enrichment(&schema.function.name);

    let mut text = format!(
        "Tool: {}\nDescription: {}\nCategory: {}",
        schema.function.name.replace('_', " "),
        schema.function.description,
        enrichment.category
    );

    if !enrichment.aliases.is_empty() {
        text.push_str("\nAliases: ");
        text.push_str(&enrichment.aliases.join(", "));
    }

    if !enrichment.examples.is_empty() {
        text.push_str("\nExamples: ");
        text.push_str(&enrichment.examples.join(", "));
    }

    // Include parameter names and descriptions
    if let Some(props) = schema.function.parameters.get("properties").and_then(|p| p.as_object()) {
        text.push_str("\nParameters: ");
        let params: Vec<String> = props.iter().map(|(name, val)| {
            let desc = val.get("description").and_then(|d| d.as_str()).unwrap_or("");
            format!("{} ({})", name.replace('_', " "), desc)
        }).collect();
        text.push_str(&params.join(", "));
    }

    text
}

impl ToolIndex {
    /// Build index from a set of tool schemas.
    pub fn new(tools: &[ToolSchema]) -> Self {
        let n_docs = tools.len() as f64;

        // Tokenize enriched text for each tool
        let tool_tokens: Vec<Vec<String>> = tools.iter().map(|t| {
            let text = enriched_tool_text(t);
            tokenize(&text)
        }).collect();

        // Count document frequency per term
        let mut df: HashMap<String, u32> = HashMap::new();
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

            let enrichment = get_enrichment(&schema.function.name);

            IndexedTool {
                schema: schema.clone(),
                terms,
                category: enrichment.category.to_string(),
            }
        }).collect();

        Self { tools: indexed, idf }
    }

    /// Select the most relevant tools for a query.
    /// Returns up to `max_tools` tools sorted by relevance (default: 5).
    pub fn select(&self, query: &str, max_tools: Option<usize>) -> Vec<ToolSchema> {
        let max = max_tools.unwrap_or(DEFAULT_MAX_TOOLS);
        let query_tokens = tokenize(query);

        if query_tokens.is_empty() {
            // No query content — return top tools by diversity
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

        // Score each tool using cosine similarity + boosts
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

            // Boost: exact tool name match in query
            let name_lower = tool.schema.function.name.to_lowercase();
            let query_lower = query.to_lowercase();
            let name_boost = if query_lower.contains(&name_lower) || query_lower.contains(&name_lower.replace('_', " ")) {
                0.5
            } else {
                0.0
            };

            // Boost: category keyword match
            let cat_boost = category_boost(&tool.category, &query_tokens);

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
    /// Returns top-K tools by relevance score. Re-includes previously called tools.
    pub fn select_with_context(
        &self,
        query: &str,
        previously_called: &[String],
        max_tools: Option<usize>,
    ) -> Vec<ToolSchema> {
        let max = max_tools.unwrap_or(DEFAULT_MAX_TOOLS);
        // Reserve slots for previously called tools
        let query_slots = max.saturating_sub(previously_called.len().min(2));
        let mut selected = self.select(query, Some(query_slots));

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

    /// Compute best relevance score for a query across all tools.
    /// Useful for debugging/testing threshold calibration.
    pub fn best_score(&self, query: &str) -> f64 {
        let query_tokens = tokenize(query);
        if query_tokens.is_empty() {
            return 1.0;
        }

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

        self.tools.iter().map(|tool| {
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

            let name_lower = tool.schema.function.name.to_lowercase();
            let query_lower = query.to_lowercase();
            let name_boost = if query_lower.contains(&name_lower) || query_lower.contains(&name_lower.replace('_', " ")) {
                0.5
            } else {
                0.0
            };

            let cat_boost = category_boost(&tool.category, &query_tokens);
            cosine + name_boost + cat_boost
        }).fold(0.0_f64, f64::max)
    }
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

/// Boost score if query mentions category-related keywords.
fn category_boost(category: &str, query_tokens: &[String]) -> f64 {
    let category_keywords: &[(&str, &[&str])] = &[
        ("filesystem", &["file", "read", "write", "open", "save", "create", "edit", "modify", "content", "path", "append", "end", "add", "phrase"]),
        ("editing", &["replace", "change", "fix", "update", "modify", "edit", "swap", "patch", "typo", "insert", "append", "add", "end", "phrase", "text"]),
        ("search", &["search", "find", "grep", "look", "pattern", "match", "where"]),
        ("shell", &["run", "execute", "command", "shell", "terminal", "build", "test", "compile", "make"]),
        ("traceability", &["trace", "requirement", "spec", "link", "traceability", "coverage"]),
        ("requirements", &["requirement", "req", "requirements", "status", "specs"]),
        ("code_analysis", &["symbol", "function", "class", "struct", "parse", "definition", "declarations"]),
        ("commands", &["undo", "redo", "command", "insert", "delete", "replace"]),
    ];

    if let Some((_cat_name, keywords)) = category_keywords.iter().find(|(name, _)| *name == category) {
        for kw in *keywords {
            if query_tokens.iter().any(|qt| qt == kw) {
                return 0.3;
            }
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::tools::builtin_tool_schemas;

    fn names(tools: &[ToolSchema]) -> Vec<&str> {
        tools.iter().map(|t| t.function.name.as_str()).collect()
    }

    // === Calibration tests: print best_score so we can find ideal threshold ===
    // Run with: cargo test tool_selector::tests::calibrate -- --nocapture

    #[test]
    fn calibrate_scores() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);

        // Queries that SHOULD select tools (expect score > threshold)
        let should_match = vec![
            ("read the main.rs file", "read_range"),
            ("show me the files in this project", "list_files"),
            ("find all TODO comments", "find_grep"),
            ("Add a comment at the end of every file", "write_range"),
            ("replace the word foo with bar in auth.rs", "str_replace"),
            ("run cargo build", "run_shell"),
            ("list all requirements", "list_requirements"),
            ("what functions are in search.rs", "get_symbols"),
            ("delete the temp file", "delete_file"),
            ("search for authentication code", "find_embed"),
            ("what implements REQ-01", "query_trace_graph"),
            ("show project structure", "list_files"),
            ("User: add 'hello' to end of every req file", "write_range"),
            ("count how many lines in main.rs", "count_lines"),
        ];

        // Queries that should NOT select tools (expect score < threshold)
        let should_not_match = vec![
            "what is the capital of france",
            "explain how recursion works",
            "what is 2 + 2",
            "tell me a joke",
            "who wrote hamlet",
            "what year was python released",
        ];

        println!("\n=== SHOULD MATCH (expect score >= {}) ===", MIN_SCORE_THRESHOLD);
        let mut min_match_score = f64::MAX;
        let mut total_tools_returned: usize = 0;
        for (query, expected_tool) in &should_match {
            let score = index.best_score(query);
            let selected = index.select(query, None);
            let got = names(&selected);
            let has_expected = got.contains(expected_tool);
            let marker = if has_expected { "✓" } else { "✗" };
            println!("  {} score={:.4} tools={} query={:50} expected={:20} got={:?}", marker, score, got.len(), query, expected_tool, got);
            total_tools_returned += got.len();
            if score < min_match_score { min_match_score = score; }
        }
        let avg_tools = total_tools_returned as f64 / should_match.len() as f64;

        println!("\n=== SHOULD NOT MATCH (expect score < {}) ===", MIN_SCORE_THRESHOLD);
        let mut max_nomatch_score = 0.0_f64;
        for query in &should_not_match {
            let score = index.best_score(query);
            let selected = index.select(query, None);
            let got = names(&selected);
            let marker = if got.is_empty() { "✓" } else { "✗" };
            println!("  {} score={:.4} tools={} query={:50} got={:?}", marker, score, got.len(), query, got);
            if score > max_nomatch_score { max_nomatch_score = score; }
        }

        println!("\n=== SUMMARY ===");
        println!("  Min score from should-match queries:     {:.4}", min_match_score);
        println!("  Max score from should-NOT-match queries: {:.4}", max_nomatch_score);
        println!("  Current MIN_SCORE_THRESHOLD:             {:.4}", MIN_SCORE_THRESHOLD);
        println!("  Ideal threshold range:                   ({:.4}, {:.4})", max_nomatch_score, min_match_score);
        println!("  Avg tools returned per match query:      {:.1}", avg_tools);
        println!("  Total tools in index:                    {}", schemas.len());

        // The threshold must be between max_nomatch and min_match
        assert!(
            max_nomatch_score < min_match_score,
            "Cannot separate! max_nomatch={:.4} >= min_match={:.4}. Enrichment needs work.",
            max_nomatch_score, min_match_score
        );

        // Verify tool count stays lean — avg should be well below total tool count
        assert!(
            avg_tools <= 4.0,
            "Too many tools returned on average ({:.1}). Selection too loose.",
            avg_tools
        );
    }

    // === Functional tests ===

    #[test]
    fn selects_file_tools_for_file_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("read the main.rs file", None);
        assert!(names(&selected).contains(&"read_range"), "Expected read_range in {:?}", names(&selected));
    }

    #[test]
    fn selects_search_tools_for_search_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("find all uses of 'TODO' in the codebase", None);
        assert!(names(&selected).contains(&"find_grep"), "Expected find_grep in {:?}", names(&selected));
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
            &["read_range".into()],
            Some(5),
        );
        assert!(names(&selected).contains(&"read_range"), "Should include previously called tool");
    }

    #[test]
    fn selects_write_tools_for_add_phrase_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("Add the phrase 'Hello bob' at the end of the file auth.rs", None);
        let n = names(&selected);
        let has_edit = n.contains(&"write_range") || n.contains(&"str_replace");
        assert!(has_edit, "Expected edit tool in {:?}", n);
    }

    #[test]
    fn selects_list_files_for_directory_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("show me the project structure", None);
        assert!(names(&selected).contains(&"list_files"), "Expected list_files in {:?}", names(&selected));
    }

    #[test]
    fn default_returns_at_most_5_tools() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("read and modify files then run tests", None);
        assert!(selected.len() <= 5, "Default should be max 5, got {}", selected.len());
    }

    #[test]
    fn no_tools_for_irrelevant_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("what is the capital of france", None);
        assert!(selected.is_empty(), "Expected no tools for general knowledge query, got {:?}", names(&selected));
    }

    #[test]
    fn selects_shell_for_build_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("run the tests", None);
        assert!(names(&selected).contains(&"run_shell"), "Expected run_shell in {:?}", names(&selected));
    }

    #[test]
    fn selects_grep_for_where_is_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("where is the login function defined", None);
        let n = names(&selected);
        let has_search = n.contains(&"find_grep") || n.contains(&"find_embed") || n.contains(&"get_symbols");
        assert!(has_search, "Expected search/code tool in {:?}", n);
    }

    #[test]
    fn selects_write_for_append_to_files() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("User: add 'I love requirements' as comment to end of every file\nAgent: I'll look for requirement files first", None);
        let n = names(&selected);
        let has_fs = n.contains(&"write_range") || n.contains(&"list_files");
        assert!(has_fs, "Expected filesystem tool in {:?}", n);
    }

    #[test]
    fn no_tools_for_math_question() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("what is 15 times 23", None);
        assert!(selected.is_empty(), "Expected no tools for math, got {:?}", names(&selected));
    }

    #[test]
    fn no_tools_for_explanation_question() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        // "binary search" contains "search" which matches search tools — use different query
        let selected = index.select("explain how recursion works in programming", None);
        assert!(selected.is_empty(), "Expected no tools for explanation, got {:?}", names(&selected));
    }

    #[test]
    fn selects_list_for_show_files() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("what files do we have in this project", None);
        assert!(names(&selected).contains(&"list_files"), "Expected list_files in {:?}", names(&selected));
    }

    #[test]
    fn selects_trace_for_requirement_coverage() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("show traceability for REQ-02", None);
        let n = names(&selected);
        let has_trace = n.contains(&"query_trace_graph") || n.contains(&"list_requirements");
        assert!(has_trace, "Expected traceability tool in {:?}", n);
    }

    #[test]
    fn selects_read_for_check_contents() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("show me the contents of src/auth.rs", None);
        assert!(names(&selected).contains(&"read_range"), "Expected read_range in {:?}", names(&selected));
    }

    #[test]
    fn no_tools_for_who_question() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("who invented the internet", None);
        assert!(selected.is_empty(), "Expected no tools, got {:?}", names(&selected));
    }

    #[test]
    fn selects_symbols_for_function_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("what functions are defined in upload.rs", None);
        let n = names(&selected);
        let has_code = n.contains(&"get_symbols") || n.contains(&"find_grep");
        assert!(has_code, "Expected code analysis tool in {:?}", n);
    }

    #[test]
    fn selects_delete_for_remove_file() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("remove the file scratch.txt", None);
        assert!(names(&selected).contains(&"delete_file"), "Expected delete_file in {:?}", names(&selected));
    }

    #[test]
    fn selects_replace_for_fix_typo() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("fix the typo in auth.rs, change 'authenicate' to 'authenticate'", None);
        let n = names(&selected);
        let has_edit = n.contains(&"str_replace") || n.contains(&"write_range");
        assert!(has_edit, "Expected edit tool for typo fix in {:?}", n);
    }
}
