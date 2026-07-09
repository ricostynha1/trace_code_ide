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
const MIN_SCORE_THRESHOLD: f64 = 0.01;

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
        "read_file" => ToolEnrichment {
            category: "filesystem",
            aliases: &["open file", "load file", "display file", "show source code", "cat file", "view file", "get contents", "look at file"],
            examples: &["Read Cargo.toml", "Open src/main.rs", "Display configuration file", "Show auth.rs contents", "View the readme", "Check what is in file"],
        },
        "write_file" => ToolEnrichment {
            category: "filesystem",
            aliases: &["create file", "save file", "overwrite file", "put content", "write content", "make file", "output to file", "generate file"],
            examples: &["Create a new config.json", "Write hello world to main.rs", "Save output to results.txt", "Add phrase to end of file", "Put text in file", "Create auth.rs with content"],
        },
        "str_replace" => ToolEnrichment {
            category: "editing",
            aliases: &["replace text", "find and replace", "substitute", "edit text", "change text", "modify content", "swap text", "update line", "patch file", "fix typo"],
            examples: &["Replace TODO with implementation", "Change function name from foo to bar", "Fix typo in auth.rs", "Update the import statement", "Add phrase at end of file by replacing last line"],
        },
        "insert_lines" => ToolEnrichment {
            category: "editing",
            aliases: &["add lines", "append text", "prepend text", "insert text", "add content at line", "put text at position", "add to file"],
            examples: &["Insert import at top of file", "Add line at end of file", "Prepend header comment", "Add phrase at the end", "Insert after line 10"],
        },
        "list_files" => ToolEnrichment {
            category: "filesystem",
            aliases: &["show directory", "ls", "list directory", "browse files", "show tree", "what files", "directory contents", "find files"],
            examples: &["List all files in src/", "Show project structure", "What files are in the root", "Browse the test directory"],
        },
        "emit_command" => ToolEnrichment {
            category: "commands",
            aliases: &["run command", "execute command", "do operation", "undo redo", "insert delete"],
            examples: &["Insert text at position", "Delete range", "Replace content"],
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
        "search_files" => ToolEnrichment {
            category: "search",
            aliases: &["find text", "grep", "search code", "look for", "find pattern", "where is", "search project"],
            examples: &["Find all TODO comments", "Search for 'error' in project", "Where is function login defined", "Grep for API_KEY"],
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
    /// Includes query relevance + previously called tools.
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

    #[test]
    fn selects_write_tools_for_add_phrase_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("Add the phrase 'Hello bob' at the end of the file auth.rs", None);
        let names: Vec<&str> = selected.iter().map(|t| t.function.name.as_str()).collect();
        // Should include file-editing tools
        let has_edit_tool = names.contains(&"write_file")
            || names.contains(&"str_replace")
            || names.contains(&"insert_lines");
        assert!(has_edit_tool, "Expected a file editing tool in {:?}", names);
        // Should include read_file (to check current content)
        assert!(names.contains(&"read_file"), "Expected read_file in {:?}", names);
    }

    #[test]
    fn selects_list_files_for_directory_query() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("show me the project structure", None);
        let names: Vec<&str> = selected.iter().map(|t| t.function.name.as_str()).collect();
        assert!(names.contains(&"list_files"), "Expected list_files in {:?}", names);
    }

    #[test]
    fn default_returns_at_most_5_tools() {
        let schemas = builtin_tool_schemas();
        let index = ToolIndex::new(&schemas);
        let selected = index.select("read and modify files then run tests", None);
        assert!(selected.len() <= 5, "Default should be max 5, got {}", selected.len());
    }
}
