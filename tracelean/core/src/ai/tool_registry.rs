//! Tool Registry — static/dynamic tool split with prompt caching support.
//!
//! Static tools (8) form the cacheable prefix sent every request.
//! Dynamic tools loaded via discover_tools, retained for a minimum number of turns
//! before becoming eligible for cost-based removal.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use super::provider::{ToolSchema, ToolFunction};

/// Configuration for dynamic tool retention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolConfig {
    /// Minimum turns before tool becomes eligible for cost-based removal.
    /// NOT automatic removal — cost engine decides after this period.
    pub retention_turns: usize,
    /// Max combined schema chars for all dynamic tools.
    pub max_schema_chars: usize,
    /// User-triggered force purge flag.
    pub force_purge: bool,
}

impl Default for DynamicToolConfig {
    fn default() -> Self {
        Self {
            retention_turns: 5,
            max_schema_chars: 2048,
            force_purge: false,
        }
    }
}

/// A tool definition as stored in data/tools.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolJsonEntry {
    pub name: String,
    pub tier: String,
    pub category: String,
    pub description: String,
    pub embedded_txt: String,
    pub input_schema: serde_json::Value,
    /// A complete, valid arguments object — shown to the model verbatim when
    /// it produces a malformed call for this tool.
    #[serde(default)]
    pub example: Option<serde_json::Value>,
    /// One-line signature help, e.g. "read_file(path, offset?) — Read file lines."
    /// The single source for help/error text (bugs.md Bug 2: no hardcoded copies).
    #[serde(default)]
    pub short_help: Option<String>,
    /// Alternate phrasings a user query might use instead of the tool name,
    /// boosting `tool_selector.rs`'s discovery ranking.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Short natural-language example queries this tool answers, also fed
    /// into `tool_selector.rs`'s enrichment text.
    #[serde(default)]
    pub examples: Vec<String>,
}

/// Shared response-message conventions declared in tools.json, so every tool
/// phrases cross-cutting messages (e.g. how to page through capped results)
/// the same way instead of hand-rolling per tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolConventions {
    /// Template for "results were capped" messages. Placeholders:
    /// {shown}, {total}, {next_offset}.
    #[serde(default)]
    pub pagination_hint: Option<String>,
    /// Template for read_file's "single line too long, truncated" message.
    /// Placeholders: {path}, {line}, {actual}, {lower}, {upper}, {next}.
    #[serde(default)]
    pub giant_line_hint: Option<String>,
}

/// The full tools.json structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub tools: Vec<ToolJsonEntry>,
    pub dynamic_tools: Vec<ToolJsonEntry>,
    #[serde(default)]
    pub conventions: ToolConventions,
    /// category -> keywords that boost a tool's discovery score when a
    /// query token matches (`tool_selector.rs::category_boost`).
    #[serde(default)]
    pub category_keywords: HashMap<String, Vec<String>>,
}

/// A dynamically-loaded tool tracked by the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicEntry {
    pub schema: ToolSchema,
    pub name: String,
    pub embedded_txt: String,
    pub loaded_at_turn: usize,
    pub last_used_turn: usize,
    pub schema_chars: usize,
    /// Whether this entry is eligible for cost-based removal
    pub eligible_for_removal: bool,
}

/// The central tool registry managing static and dynamic tool sets.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    /// Static tools (8) — immutable after init, forms cacheable prefix.
    pub static_tools: Vec<ToolSchema>,
    /// All tool entries (static + dynamic available) for embedded_txt lookup.
    all_entries: Vec<ToolJsonEntry>,
    /// Currently loaded dynamic tools.
    pub dynamic_tools: Vec<DynamicEntry>,
    /// Configuration.
    pub config: DynamicToolConfig,
    /// Shared response-message conventions from tools.json.
    pub conventions: ToolConventions,
    /// category -> discovery-boost keywords from tools.json.
    pub category_keywords: HashMap<String, Vec<String>>,
    /// Current turn counter.
    pub current_turn: usize,
}

impl ToolRegistry {
    /// Registry built from the embedded data/tools.json, lazily initialized.
    /// Returns None only if the embedded JSON fails to parse (guarded by tests).
    pub fn embedded() -> Option<&'static ToolRegistry> {
        static EMBEDDED: std::sync::OnceLock<Option<ToolRegistry>> = std::sync::OnceLock::new();
        EMBEDDED
            .get_or_init(|| {
                ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).ok()
            })
            .as_ref()
    }

    /// Load registry from a JSON string (e.g. from `include_str!`).
    pub fn load_from_str(json: &str) -> Result<Self, String> {
        let config: ToolsConfig = serde_json::from_str(json)
            .map_err(|e| format!("Failed to parse tools.json: {}", e))?;

        let static_tools: Vec<ToolSchema> = config.tools.iter()
            .map(|entry| entry_to_schema(entry))
            .collect();

        let mut all_entries = config.tools.clone();
        all_entries.extend(config.dynamic_tools.clone());

        Ok(Self {
            static_tools,
            all_entries,
            dynamic_tools: Vec::new(),
            config: DynamicToolConfig::default(),
            conventions: config.conventions,
            category_keywords: config.category_keywords,
            current_turn: 0,
        })
    }

    /// Load registry from data/tools.json file.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read tools.json: {}", e))?;
        let config: ToolsConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse tools.json: {}", e))?;

        let static_tools: Vec<ToolSchema> = config.tools.iter()
            .map(|entry| entry_to_schema(entry))
            .collect();

        let mut all_entries = config.tools.clone();
        all_entries.extend(config.dynamic_tools.clone());

        Ok(Self {
            static_tools,
            all_entries,
            dynamic_tools: Vec::new(),
            config: DynamicToolConfig::default(),
            conventions: config.conventions,
            category_keywords: config.category_keywords,
            current_turn: 0,
        })
    }

    /// Load with default path (relative to project data dir).
    pub fn load_default(data_dir: &Path) -> Result<Self, String> {
        Self::load_from_file(&data_dir.join("tools.json"))
    }

    /// Format the shared "results were capped" message from the tools.json
    /// convention template (with a hardcoded fallback). `total` is a string so
    /// callers can pass e.g. "2000+" when the scan itself was capped.
    pub fn pagination_hint(shown: usize, total: &str, next_offset: usize) -> String {
        const DEFAULT: &str =
            "{shown} of {total} matches shown, call again with offset={next_offset} to see the rest";
        let tpl = Self::embedded()
            .and_then(|r| r.conventions.pagination_hint.clone())
            .unwrap_or_else(|| DEFAULT.to_string());
        tpl.replace("{shown}", &shown.to_string())
            .replace("{total}", total)
            .replace("{next_offset}", &next_offset.to_string())
    }

    /// Format read_file's "single line too long, truncated" hint from the
    /// tools.json convention template (with a hardcoded fallback), so the
    /// message isn't duplicated/hardcoded in the executor (P1 in tool_errors.rs).
    /// `upper` is the char cap the line was clipped to; `lower` is always 1
    /// since the clip always starts at the beginning of the line.
    pub fn giant_line_hint(path: &str, line: usize, actual: usize, upper: usize) -> String {
        const DEFAULT: &str =
            "Line {line} of '{path}' is {actual} chars but only bytes {lower}-{upper} are \
             shown here (truncated) — {line} is the line number to target, and in the cut \
             range {lower} is the lower byte offset and {upper} is the upper byte offset of \
             the shown slice. To read more of this single line, use run_shell with: \
             sed -n '{line}p' '{path}' | cut -c{lower}-{upper}, then shift both offsets \
             forward (e.g. cut -c{upper}-{next}) to page through the rest of the line.";
        let tpl = Self::embedded()
            .and_then(|r| r.conventions.giant_line_hint.clone())
            .unwrap_or_else(|| DEFAULT.to_string());
        let next = upper * 2;
        tpl.replace("{path}", path)
            .replace("{line}", &line.to_string())
            .replace("{actual}", &actual.to_string())
            .replace("{lower}", "1")
            .replace("{upper}", &upper.to_string())
            .replace("{next}", &next.to_string())
    }

    /// Get all tool schemas for the current request (static + active dynamic).
    pub fn request_schemas(&self) -> Vec<ToolSchema> {
        let mut schemas = self.static_tools.clone();
        for entry in &self.dynamic_tools {
            schemas.push(entry.schema.clone());
        }
        schemas
    }

    /// Get only static tool schemas (cacheable prefix).
    pub fn static_schemas(&self) -> &[ToolSchema] {
        &self.static_tools
    }

    /// Get dynamic tool schemas only.
    pub fn dynamic_schemas(&self) -> Vec<&ToolSchema> {
        self.dynamic_tools.iter().map(|e| &e.schema).collect()
    }

    /// Load dynamic tools from discover_tools results.
    /// Returns list of newly added tool names.
    pub fn load_dynamic_tools(&mut self, tool_names: &[String]) -> Vec<String> {
        let mut added = Vec::new();
        for name in tool_names {
            // Skip if already loaded — update last_used_turn
            if let Some(entry) = self.dynamic_tools.iter_mut().find(|d| &d.name == name) {
                entry.last_used_turn = self.current_turn;
                entry.eligible_for_removal = false;
                continue;
            }
            // Find in all_entries
            if let Some(json_entry) = self.all_entries.iter().find(|e| &e.name == name) {
                let schema = entry_to_schema(json_entry);
                let schema_chars = serde_json::to_string(&schema).unwrap_or_default().len();
                self.dynamic_tools.push(DynamicEntry {
                    schema,
                    name: name.clone(),
                    embedded_txt: json_entry.embedded_txt.clone(),
                    loaded_at_turn: self.current_turn,
                    last_used_turn: self.current_turn,
                    schema_chars,
                    eligible_for_removal: false,
                });
                added.push(name.clone());
            }
        }
        self.enforce_schema_limit();
        added
    }

    /// Mark a dynamic tool as used this turn.
    pub fn mark_tool_used(&mut self, name: &str) {
        if let Some(entry) = self.dynamic_tools.iter_mut().find(|d| d.name == name) {
            entry.last_used_turn = self.current_turn;
            entry.eligible_for_removal = false;
        }
    }

    /// Advance turn counter and update eligibility.
    pub fn advance_turn(&mut self) {
        self.current_turn += 1;
        let retention = self.config.retention_turns;
        let turn = self.current_turn;
        for entry in &mut self.dynamic_tools {
            if turn - entry.last_used_turn > retention {
                entry.eligible_for_removal = true;
            }
        }
    }

    /// Get tools eligible for cost-based removal.
    pub fn eligible_for_removal(&self) -> Vec<&DynamicEntry> {
        self.dynamic_tools.iter().filter(|e| e.eligible_for_removal).collect()
    }

    /// Remove specific dynamic tools by name (called by cost engine).
    pub fn remove_dynamic_tools(&mut self, names: &[String]) {
        self.dynamic_tools.retain(|e| !names.contains(&e.name));
    }

    /// User-triggered purge: remove ALL dynamic tools immediately.
    pub fn purge_all_dynamic(&mut self) {
        self.dynamic_tools.clear();
    }

    /// Handle help_tool request: return embedded_txt for a tool.
    pub fn help_tool(&self, tool_name: &str) -> Option<String> {
        // Check all_entries (static + dynamic available)
        if let Some(entry) = self.all_entries.iter().find(|e| e.name == tool_name) {
            let sig = entry.short_help.as_deref().unwrap_or(&entry.description);
            return Some(format!("# {}\n\n{}\n\n{}\n\n## Schema\n```json\n{}\n```",
                entry.name, sig, entry.embedded_txt,
                serde_json::to_string_pretty(&entry.input_schema).unwrap_or_default()
            ));
        }
        // Check loaded dynamic tools not in all_entries
        if let Some(entry) = self.dynamic_tools.iter().find(|d| d.name == tool_name) {
            return Some(entry.embedded_txt.clone());
        }
        None
    }

    /// One-line signature help from tools.json (falls back to the description).
    pub fn short_help_for(&self, tool_name: &str) -> Option<&str> {
        self.all_entries
            .iter()
            .find(|e| e.name == tool_name)
            .map(|e| e.short_help.as_deref().unwrap_or(&e.description))
    }

    /// Names of the static (core) tools, in tools.json order.
    pub fn static_tool_names(&self) -> Vec<&str> {
        self.static_tools.iter().map(|s| s.function.name.as_str()).collect()
    }

    /// Names of dynamic tools available for discovery (not necessarily loaded).
    pub fn available_dynamic_names(&self) -> Vec<&str> {
        self.all_entries
            .iter()
            .filter(|e| e.tier == "dynamic")
            .map(|e| e.name.as_str())
            .collect()
    }

    /// Retrieval for discover_tools: rank available (non-static) tools against a
    /// free-text capability query. Scores keyword overlap over name, description
    /// and embedded_txt, with a strong boost for name (sub)matches.
    /// Returns matching tool names, best first, score-thresholded.
    pub fn discover_tools(&self, query: &str, max_results: usize) -> Vec<String> {
        let q_tokens = search_tokenize(query);
        if q_tokens.is_empty() {
            return Vec::new();
        }
        let q_lower = query.to_lowercase();

        let mut scored: Vec<(f64, &str)> = self
            .all_entries
            .iter()
            .filter(|e| e.tier == "dynamic")
            .map(|e| {
                let mut score = 0.0f64;
                let name_lower = e.name.to_lowercase();
                let name_spaced = name_lower.replace('_', " ");
                // Exact/substring name match dominates
                if q_lower.contains(&name_lower) || q_lower.contains(&name_spaced) {
                    score += 3.0;
                }
                // Per-token hits, weighted by where they land
                let desc = e.description.to_lowercase();
                let long = e.embedded_txt.to_lowercase();
                for t in &q_tokens {
                    if name_spaced.split(' ').any(|p| p == t) {
                        score += 1.5;
                    }
                    if desc.contains(t.as_str()) {
                        score += 0.8;
                    }
                    if long.contains(t.as_str()) {
                        score += 0.3;
                    }
                }
                // Normalize slightly by query length so long queries don't
                // trivially clear the threshold on embedded_txt noise.
                score /= (q_tokens.len() as f64).sqrt();
                (score, e.name.as_str())
            })
            .filter(|(score, _)| *score >= 0.5)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(max_results.max(1))
            .map(|(_, name)| name.to_string())
            .collect()
    }

    /// Get combined schema chars of all dynamic tools.
    pub fn dynamic_schema_chars(&self) -> usize {
        self.dynamic_tools.iter().map(|e| e.schema_chars).sum()
    }

    /// Check if a tool name is in the static set.
    pub fn is_static_tool(&self, name: &str) -> bool {
        self.static_tools.iter().any(|s| s.function.name == name)
    }

    /// Get all available tool entries for embedding index building.
    pub fn all_tool_entries(&self) -> &[ToolJsonEntry] {
        &self.all_entries
    }

    /// Canonical example arguments for a tool (from tools.json).
    pub fn example_for(&self, name: &str) -> Option<&serde_json::Value> {
        self.all_entries
            .iter()
            .find(|e| e.name == name)
            .and_then(|e| e.example.as_ref())
    }

    /// JSON schema for a tool's arguments, if known.
    pub fn schema_for(&self, name: &str) -> Option<&serde_json::Value> {
        self.all_entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| &e.input_schema)
    }

    /// Enforce max_schema_chars limit by purging oldest-unused dynamic tools.
    fn enforce_schema_limit(&mut self) {
        while self.dynamic_schema_chars() > self.config.max_schema_chars && !self.dynamic_tools.is_empty() {
            // Find oldest unused
            let oldest_idx = self.dynamic_tools.iter()
                .enumerate()
                .min_by_key(|(_, e)| e.last_used_turn)
                .map(|(i, _)| i)
                .unwrap();
            self.dynamic_tools.remove(oldest_idx);
        }
    }
}

/// Lowercased alphanumeric tokens ≥3 chars, minus generic filler words.
fn search_tokenize(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "for", "with", "from", "that", "this", "what", "which",
        "need", "want", "how", "can", "use", "tool", "tools", "please", "get",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Convert a ToolJsonEntry to a ToolSchema (OpenAI-compatible function calling format).
fn entry_to_schema(entry: &ToolJsonEntry) -> ToolSchema {
    ToolSchema {
        tool_type: "function".to_string(),
        function: ToolFunction {
            name: entry.name.clone(),
            description: entry.description.clone(),
            parameters: entry.input_schema.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_tools_json() -> String {
        r#"{
  "tools": [
    {
      "name": "read_file",
      "tier": "static",
      "category": "filesystem",
      "description": "Read file lines.",
      "embedded_txt": "Detailed read_file help text.",
      "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
    },
    {
      "name": "help_tool",
      "tier": "static",
      "category": "meta",
      "description": "Get tool help.",
      "embedded_txt": "Help tool detailed info.",
      "input_schema": {"type": "object", "properties": {"tool_name": {"type": "string"}}, "required": ["tool_name"]}
    }
  ],
  "dynamic_tools": [
    {
      "name": "delete_file",
      "tier": "dynamic",
      "category": "filesystem",
      "description": "Delete a file.",
      "embedded_txt": "Detailed delete_file help.",
      "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
    }
  ]
}"#.to_string()
    }

    #[test]
    fn test_load_from_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_tools_json()).unwrap();
        let registry = ToolRegistry::load_from_file(tmp.path()).unwrap();
        assert_eq!(registry.static_tools.len(), 2);
        assert_eq!(registry.dynamic_tools.len(), 0);
        assert_eq!(registry.all_entries.len(), 3);
    }

    #[test]
    fn test_load_dynamic_tools() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_tools_json()).unwrap();
        let mut registry = ToolRegistry::load_from_file(tmp.path()).unwrap();

        let added = registry.load_dynamic_tools(&["delete_file".to_string()]);
        assert_eq!(added, vec!["delete_file"]);
        assert_eq!(registry.dynamic_tools.len(), 1);
        assert_eq!(registry.request_schemas().len(), 3);
    }

    #[test]
    fn test_eligibility_after_retention() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_tools_json()).unwrap();
        let mut registry = ToolRegistry::load_from_file(tmp.path()).unwrap();
        registry.config.retention_turns = 2;

        registry.load_dynamic_tools(&["delete_file".to_string()]);
        assert!(!registry.dynamic_tools[0].eligible_for_removal);

        registry.advance_turn();
        registry.advance_turn();
        assert!(!registry.dynamic_tools[0].eligible_for_removal);

        // 3rd turn: 3 > 2, now eligible
        registry.advance_turn();
        assert!(registry.dynamic_tools[0].eligible_for_removal);
    }

    #[test]
    fn test_help_tool() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_tools_json()).unwrap();
        let registry = ToolRegistry::load_from_file(tmp.path()).unwrap();

        let help = registry.help_tool("read_file").unwrap();
        assert!(help.contains("Detailed read_file help text"));

        let help = registry.help_tool("delete_file").unwrap();
        assert!(help.contains("Detailed delete_file help"));

        assert!(registry.help_tool("nonexistent").is_none());
    }

    #[test]
    fn test_purge_all_dynamic() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_tools_json()).unwrap();
        let mut registry = ToolRegistry::load_from_file(tmp.path()).unwrap();

        registry.load_dynamic_tools(&["delete_file".to_string()]);
        assert_eq!(registry.dynamic_tools.len(), 1);

        registry.purge_all_dynamic();
        assert_eq!(registry.dynamic_tools.len(), 0);
        assert_eq!(registry.request_schemas().len(), 2);
    }

    // ── discover_tools retrieval over the real embedded tools.json ──────────
    // (bugs.md Bug 0.5: the retrieval "did not catch well the tool lists")

    fn real_registry() -> &'static ToolRegistry {
        ToolRegistry::embedded().expect("embedded tools.json parses")
    }

    #[test]
    fn embedded_registry_has_web_tools_in_core() {
        // bugs.md Bug 0: web_search/web_fetch must be static (core) tools.
        let names = real_registry().static_tool_names();
        assert!(names.contains(&"web_search"), "core tools: {:?}", names);
        assert!(names.contains(&"web_fetch"), "core tools: {:?}", names);
    }

    #[test]
    fn embedded_registry_all_entries_have_short_help() {
        for entry in real_registry().all_tool_entries() {
            assert!(
                entry.short_help.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
                "tool `{}` missing short_help in tools.json",
                entry.name
            );
        }
    }

    #[test]
    fn search_finds_delete_file() {
        for q in ["delete a file", "remove obsolete file", "delete_file"] {
            let hits = real_registry().discover_tools(q, 5);
            assert!(hits.contains(&"delete_file".to_string()), "query {:?} → {:?}", q, hits);
        }
    }

    #[test]
    fn search_finds_trace_tools() {
        let hits = real_registry().discover_tools("what code implements requirement REQ-01", 5);
        assert!(hits.contains(&"query_trace_graph".to_string()), "{:?}", hits);

        let hits = real_registry().discover_tools("which requirements does this function satisfy", 5);
        assert!(hits.contains(&"query_code_element".to_string()), "{:?}", hits);
    }

    #[test]
    fn search_finds_symbols_and_requirements() {
        let hits = real_registry().discover_tools("list functions and structs in a file", 5);
        assert!(hits.contains(&"get_symbols".to_string()), "{:?}", hits);

        let hits = real_registry().discover_tools("show all project requirements and status", 5);
        assert!(hits.contains(&"list_requirements".to_string()), "{:?}", hits);
    }

    #[test]
    fn search_returns_nothing_for_unrelated_query() {
        let hits = real_registry().discover_tools("capital of portugal", 5);
        assert!(hits.is_empty(), "unrelated query must not load tools: {:?}", hits);
    }

    #[test]
    fn search_respects_max_results() {
        let hits = real_registry().discover_tools("file requirements symbols trace", 2);
        assert!(hits.len() <= 2);
    }

    #[test]
    fn search_then_load_makes_tool_available() {
        // End-to-end discover flow: search → load → present in request set.
        let mut reg = ToolRegistry::load_from_str(include_str!("../../../data/tools.json")).unwrap();
        let hits = reg.discover_tools("delete a file", 3);
        let added = reg.load_dynamic_tools(&hits);
        assert!(added.contains(&"delete_file".to_string()));
        assert!(reg.dynamic_schemas().iter().any(|s| s.function.name == "delete_file"));
    }

    #[test]
    fn test_mark_used_resets_eligibility() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_tools_json()).unwrap();
        let mut registry = ToolRegistry::load_from_file(tmp.path()).unwrap();
        registry.config.retention_turns = 1;

        registry.load_dynamic_tools(&["delete_file".to_string()]);
        registry.advance_turn();
        registry.advance_turn();
        assert!(registry.dynamic_tools[0].eligible_for_removal);

        registry.mark_tool_used("delete_file");
        assert!(!registry.dynamic_tools[0].eligible_for_removal);
    }
}
