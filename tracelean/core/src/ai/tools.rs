//! MCP Tool definitions — the set of tools agents can use to interact with the environment.
//! Used by the MCP host/client and agent permission model.

use serde::{Deserialize, Serialize};
use super::provider::{ToolSchema, ToolFunction};
use super::tool_registry::{ToolJsonEntry, ToolRegistry};

/// A tool callable by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ToolParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub param_type: ParamType,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    String,
    Integer,
    Boolean,
    Array { item_type: Box<ParamType> },
    Object,
}

impl ToolDefinition {
    /// Convert to OpenAI-compatible function calling schema.
    pub fn to_tool_schema(&self) -> ToolSchema {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            let type_str = match &param.param_type {
                ParamType::String => "string",
                ParamType::Integer => "integer",
                ParamType::Boolean => "boolean",
                ParamType::Array { .. } => "array",
                ParamType::Object => "object",
            };

            let mut prop = serde_json::Map::new();
            prop.insert("type".into(), serde_json::Value::String(type_str.into()));
            prop.insert("description".into(), serde_json::Value::String(param.description.clone()));

            if let ParamType::Array { item_type } = &param.param_type {
                let item_type_str = match item_type.as_ref() {
                    ParamType::String => "string",
                    ParamType::Integer => "integer",
                    ParamType::Boolean => "boolean",
                    _ => "string",
                };
                let mut items = serde_json::Map::new();
                items.insert("type".into(), serde_json::Value::String(item_type_str.into()));
                prop.insert("items".into(), serde_json::Value::Object(items));
            }

            properties.insert(param.name.clone(), serde_json::Value::Object(prop));

            if param.required {
                required.push(serde_json::Value::String(param.name.clone()));
            }
        }

        let parameters = serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
        });

        ToolSchema {
            tool_type: "function".into(),
            function: ToolFunction {
                name: self.name.clone(),
                description: self.description.clone(),
                parameters,
            },
        }
    }
}

/// Convert all builtin tool definitions to OpenAI tool schemas (alphabetically sorted for cache stability).
pub fn builtin_tool_schemas() -> Vec<ToolSchema> {
    let mut schemas: Vec<ToolSchema> = builtin_tool_definitions().iter().map(|t| t.to_tool_schema()).collect();
    schemas.sort_by(|a, b| a.function.name.cmp(&b.function.name));
    schemas
}

/// Result of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    pub data: Option<serde_json::Value>,
}

/// A tool call request from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// UI/tracing metadata attached to a tool call by the agent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
}

/// Agent-side tool call: MCP-compliant call + UI metadata envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ToolCallMeta>,
}

impl AgentToolCall {
    pub fn to_mcp_call(&self) -> ToolCall {
        ToolCall {
            name: self.name.clone(),
            arguments: self.arguments.clone(),
        }
    }

    pub fn reason(&self) -> Option<&str> {
        self.ui.as_ref().and_then(|m| m.reason.as_deref())
    }
}

/// Convert a `data/tools.json` entry's JSON-Schema `input_schema` into our
/// `ToolParam` list (name/type/description/required).
fn schema_to_params(entry: &ToolJsonEntry) -> Vec<ToolParam> {
    let Some(props) = entry.input_schema.get("properties").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    let required: Vec<&str> = entry.input_schema.get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    props.iter().map(|(name, prop)| {
        let param_type = match prop.get("type").and_then(|t| t.as_str()).unwrap_or("string") {
            "integer" | "number" => ParamType::Integer,
            "boolean" => ParamType::Boolean,
            "array" => ParamType::Array { item_type: Box::new(ParamType::String) },
            "object" => ParamType::Object,
            _ => ParamType::String,
        };
        ToolParam {
            name: name.clone(),
            param_type,
            description: prop.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
            required: required.contains(&name.as_str()),
        }
    }).collect()
}

/// All tools available to MCP clients, derived from the single source of
/// truth (`data/tools.json` via `ToolRegistry`) instead of a second,
/// independently hand-maintained list — a stale second copy here previously
/// advertised tool names (`read_range`, `write_range`, `count_lines`, ...)
/// that didn't exist in `tool_executor.rs`'s dispatch table at all.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    let Some(registry) = ToolRegistry::embedded() else { return Vec::new() };
    registry.all_tool_entries().iter().map(|entry| ToolDefinition {
        name: entry.name.clone(),
        description: entry.description.clone(),
        parameters: schema_to_params(entry),
    }).collect()
}
