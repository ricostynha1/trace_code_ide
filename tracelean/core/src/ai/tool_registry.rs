//! Tool Registry — static/dynamic tool split with prompt caching support.
//!
//! Static tools (8) form the cacheable prefix sent every request.
//! Dynamic tools loaded via discover_tools, retained for a minimum number of turns
//! before becoming eligible for cost-based removal.

use serde::{Deserialize, Serialize};
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
}

/// The full tools.json structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub tools: Vec<ToolJsonEntry>,
    pub dynamic_tools: Vec<ToolJsonEntry>,
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
    /// Current turn counter.
    pub current_turn: usize,
}

impl ToolRegistry {
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
            current_turn: 0,
        })
    }

    /// Load with default path (relative to project data dir).
    pub fn load_default(data_dir: &Path) -> Result<Self, String> {
        Self::load_from_file(&data_dir.join("tools.json"))
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
            return Some(format!("# {}\n\n{}\n\n## Schema\n```json\n{}\n```",
                entry.name, entry.embedded_txt,
                serde_json::to_string_pretty(&entry.input_schema).unwrap_or_default()
            ));
        }
        // Check loaded dynamic tools not in all_entries
        if let Some(entry) = self.dynamic_tools.iter().find(|d| d.name == tool_name) {
            return Some(entry.embedded_txt.clone());
        }
        None
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
