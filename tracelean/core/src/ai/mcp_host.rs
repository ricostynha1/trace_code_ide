//! MCP Tool Host (4.15) — Exposes TraceLean internals as MCP tools.
//! External agents/plugins connect via stdio or TCP and invoke tools using JSON-RPC.
//!
//! Protocol: JSON-RPC 2.0 over stdio (one JSON object per line).
//! Methods:
//!   - tools/list → returns available tool definitions
//!   - tools/call → execute a tool call, returns result
//!   - initialize → handshake (client sends capabilities)

use super::tools::{ToolCall, builtin_tool_definitions};
use super::tool_executor::{self, AgentPermissions};
use crate::parser::SymbolTable;
use crate::state::AppState;
use crate::trace_graph::TraceGraph;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// MCP JSON-RPC request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// MCP JSON-RPC response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Server capabilities returned during initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub name: String,
    pub version: String,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySet {
    pub tools: ToolCapability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

/// Handle a single JSON-RPC request and produce a response.
/// Called from the MCP host loop (stdio reader).
pub fn handle_request(
    request: &JsonRpcRequest,
    project_root: &Path,
    state: &mut AppState,
    symbols: &SymbolTable,
    graph: &TraceGraph,
    permissions: &AgentPermissions,
) -> JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => handle_initialize(request),
        "tools/list" => handle_tools_list(request),
        "tools/call" => handle_tools_call(request, project_root, state, symbols, graph, permissions),
        _ => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: request.id.clone(),
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        },
    }
}

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    let caps = ServerCapabilities {
        name: "tracelean-mcp".into(),
        version: "0.1.0".into(),
        capabilities: CapabilitySet {
            tools: ToolCapability { list_changed: false },
        },
    };
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: request.id.clone(),
        result: Some(serde_json::to_value(caps).unwrap()),
        error: None,
    }
}

fn handle_tools_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    let tools = builtin_tool_definitions();
    // Convert to MCP format
    let mcp_tools: Vec<McpToolDef> = tools.into_iter().map(|t| McpToolDef {
        name: t.name,
        description: t.description,
        input_schema: params_to_json_schema(&t.parameters),
    }).collect();

    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: request.id.clone(),
        result: Some(serde_json::json!({ "tools": mcp_tools })),
        error: None,
    }
}

fn handle_tools_call(
    request: &JsonRpcRequest,
    project_root: &Path,
    state: &mut AppState,
    symbols: &SymbolTable,
    graph: &TraceGraph,
    permissions: &AgentPermissions,
) -> JsonRpcResponse {
    // Extract tool name and arguments from params
    let tool_name = request.params.get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let arguments = request.params.get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let call = ToolCall {
        name: tool_name.to_string(),
        arguments,
    };

    let result = tool_executor::execute_tool(&call, project_root, state, symbols, graph, permissions);

    let mcp_result = serde_json::json!({
        "content": [{
            "type": "text",
            "text": result.content,
        }],
        "isError": !result.success,
    });

    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: request.id.clone(),
        result: Some(mcp_result),
        error: None,
    }
}

/// MCP tool definition format for tools/list response.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpToolDef {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: serde_json::Value,
}

/// Convert our tool params to JSON Schema format.
fn params_to_json_schema(params: &[super::tools::ToolParam]) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();

    for p in params {
        let type_str = match &p.param_type {
            super::tools::ParamType::String => "string",
            super::tools::ParamType::Integer => "integer",
            super::tools::ParamType::Boolean => "boolean",
            super::tools::ParamType::Array { .. } => "array",
            super::tools::ParamType::Object => "object",
        };
        properties.insert(p.name.clone(), serde_json::json!({
            "type": type_str,
            "description": p.description,
        }));
        if p.required {
            required.push(serde_json::Value::String(p.name.clone()));
        }
    }

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}
