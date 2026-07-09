//! MCP Tool definitions — the set of tools agents can use to interact with the environment.
//! These are the "hands" of the agent: read files, write files, emit commands, query trace graph, etc.
//!
//! Used by:
//! - Internal agents (elicitation, formalisation, implementation, repair)
//! - MCP tool host (4.15): exposed to external MCP clients
//! - Agent permission model (4.17): filtered per-agent

use serde::{Deserialize, Serialize};
use super::provider::{ToolSchema, ToolFunction};

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
    /// This is the format models were trained on.
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

/// Convert all builtin tool definitions to OpenAI tool schemas.
pub fn builtin_tool_schemas() -> Vec<ToolSchema> {
    builtin_tool_definitions().iter().map(|t| t.to_tool_schema()).collect()
}

/// Result of a tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    /// Structured data (JSON) if applicable.
    pub data: Option<serde_json::Value>,
}

/// A tool call request from an agent (MCP protocol 2024-11-05).
/// This is the wire format sent to MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// UI/tracing metadata attached to a tool call by the agent.
/// Not sent to MCP servers — purely for display and observability.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCallMeta {
    /// Why the agent is calling this tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// High-level goal this call is part of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Step number within the current plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
}

/// Agent-side tool call: MCP-compliant call + UI metadata envelope.
/// The agent emits this; the executor strips `ui` before sending to MCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolCall {
    /// MCP tool name.
    pub name: String,
    pub arguments: serde_json::Value,
    /// UI/tracing metadata (not sent to tool servers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<ToolCallMeta>,
}

impl AgentToolCall {
    /// Strip metadata, return MCP-pure ToolCall for execution.
    pub fn to_mcp_call(&self) -> ToolCall {
        ToolCall {
            name: self.name.clone(),
            arguments: self.arguments.clone(),
        }
    }

    /// Extract reason for UI display.
    pub fn reason(&self) -> Option<&str> {
        self.ui.as_ref().and_then(|m| m.reason.as_deref())
    }
}

/// All built-in tools available to agents.
pub fn builtin_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "read_file".into(),
            description: "Read the contents of a file relative to project root.".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative file path.".into(),
                required: true,
            }],
        },
        ToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file (creates or overwrites). Goes through the command system — fully reversible.".into(),
            parameters: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Relative file path.".into(),
                    required: true,
                },
                ToolParam {
                    name: "content".into(),
                    param_type: ParamType::String,
                    description: "New file content.".into(),
                    required: true,
                },
            ],
        },
        ToolDefinition {
            name: "str_replace".into(),
            description: "Replace a specific string in a file. Use this for surgical edits instead of rewriting the whole file. The old_str must match exactly (including whitespace).".into(),
            parameters: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Relative file path.".into(),
                    required: true,
                },
                ToolParam {
                    name: "old_str".into(),
                    param_type: ParamType::String,
                    description: "Exact string to find and replace (must match uniquely).".into(),
                    required: true,
                },
                ToolParam {
                    name: "new_str".into(),
                    param_type: ParamType::String,
                    description: "Replacement string.".into(),
                    required: true,
                },
            ],
        },
        ToolDefinition {
            name: "insert_lines".into(),
            description: "Insert text at a specific line number in a file. Line 0 = beginning of file.".into(),
            parameters: vec![
                ToolParam {
                    name: "path".into(),
                    param_type: ParamType::String,
                    description: "Relative file path.".into(),
                    required: true,
                },
                ToolParam {
                    name: "line".into(),
                    param_type: ParamType::Integer,
                    description: "Line number to insert before (0-indexed).".into(),
                    required: true,
                },
                ToolParam {
                    name: "text".into(),
                    param_type: ParamType::String,
                    description: "Text to insert.".into(),
                    required: true,
                },
            ],
        },
        ToolDefinition {
            name: "list_files".into(),
            description: "List files and directories under a given path.".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative directory path (empty string = project root).".into(),
                required: false,
            }],
        },
        ToolDefinition {
            name: "delete_file".into(),
            description: "Delete a file from the project. Reversible via undo.".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative file path to delete.".into(),
                required: true,
            }],
        },
        ToolDefinition {
            name: "query_trace_graph".into(),
            description: "Query the traceability graph by requirement ID. Returns linked specs, code elements, and tests.".into(),
            parameters: vec![ToolParam {
                name: "req_id".into(),
                param_type: ParamType::String,
                description: "Requirement ID (e.g. REQ-01).".into(),
                required: true,
            }],
        },
        ToolDefinition {
            name: "query_code_element".into(),
            description: "Query trace graph for a code element. Returns linked requirements and specs.".into(),
            parameters: vec![
                ToolParam {
                    name: "file".into(),
                    param_type: ParamType::String,
                    description: "File path containing the element.".into(),
                    required: true,
                },
                ToolParam {
                    name: "name".into(),
                    param_type: ParamType::String,
                    description: "Symbol name.".into(),
                    required: true,
                },
            ],
        },
        ToolDefinition {
            name: "list_requirements".into(),
            description: "List all requirements in the project with their IDs, titles, and statuses.".into(),
            parameters: vec![],
        },
        ToolDefinition {
            name: "get_symbols".into(),
            description: "Get parsed symbols (functions, classes, structs) from a file.".into(),
            parameters: vec![ToolParam {
                name: "path".into(),
                param_type: ParamType::String,
                description: "Relative file path.".into(),
                required: true,
            }],
        },
        ToolDefinition {
            name: "run_shell".into(),
            description: "Execute a shell command in the project directory. Returns stdout/stderr.".into(),
            parameters: vec![
                ToolParam {
                    name: "command".into(),
                    param_type: ParamType::String,
                    description: "Shell command to execute.".into(),
                    required: true,
                },
                ToolParam {
                    name: "timeout_secs".into(),
                    param_type: ParamType::Integer,
                    description: "Timeout in seconds (default 30).".into(),
                    required: false,
                },
            ],
        },
        ToolDefinition {
            name: "search_files".into(),
            description: "Search for a text pattern across project files. Returns matching file:line pairs.".into(),
            parameters: vec![
                ToolParam {
                    name: "pattern".into(),
                    param_type: ParamType::String,
                    description: "Search pattern (substring match).".into(),
                    required: true,
                },
                ToolParam {
                    name: "file_pattern".into(),
                    param_type: ParamType::String,
                    description: "File glob filter (e.g. '*.rs'). Empty = all files.".into(),
                    required: false,
                },
            ],
        },
    ]
}

/// Format tool definitions into a system prompt fragment for the agent.
pub fn tools_as_system_prompt(tools: &[ToolDefinition]) -> String {
    let mut out = String::from("You have access to the following tools to interact with the project:\n\n");
    for tool in tools {
        out.push_str(&format!("## {}\n{}\n", tool.name, tool.description));
        if !tool.parameters.is_empty() {
            out.push_str("Parameters:\n");
            for p in &tool.parameters {
                let req = if p.required { "required" } else { "optional" };
                out.push_str(&format!("  - {} ({:?}, {}): {}\n", p.name, p.param_type, req, p.description));
            }
        }
        out.push('\n');
    }
    out.push_str(
        "To call a tool, respond with a JSON block like:\n\
         ```tool_call\n\
         {\"name\": \"read_file\", \"arguments\": {\"path\": \"src/main.rs\"}, \"ui\": {\"reason\": \"Inspect entry point before editing\"}}\n\
         ```\n\n\
         The `ui` field is optional metadata for the user. It is never sent to the tool server.\n\
         You may call multiple tools in sequence. After each tool call, you will receive the result \
         before continuing. When done, provide your final answer without a tool_call block.\n"
    );
    out
}
