//! MCP Client (4.16) — Connect to user-configured external MCP servers.
//! Discovers tools from remote servers and makes them available to agents.
//!
//! Configuration lives in `.tracelean/mcp.json`:
//! ```json
//! {
//!   "servers": [
//!     { "name": "my-server", "command": "uvx", "args": ["my-mcp-server"], "env": {} }
//!   ]
//! }
//! ```

use super::tools::{ToolDefinition, ToolParam, ParamType, ToolResult};
use super::mcp_host::JsonRpcResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command as TokioCommand};

/// MCP server configuration (from .tracelean/mcp.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
}

/// Full MCP configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
}

impl McpConfig {
    /// Load from project's .tracelean/mcp.json.
    pub fn load(project_root: &Path) -> Self {
        let config_path = project_root.join(".tracelean").join("mcp.json");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            serde_json::from_str(&content).unwrap_or_else(|_| Self { servers: vec![] })
        } else {
            Self { servers: vec![] }
        }
    }

    /// Active (non-disabled) servers.
    pub fn active_servers(&self) -> Vec<&McpServerConfig> {
        self.servers.iter().filter(|s| !s.disabled).collect()
    }
}

/// A connected MCP server instance.
pub struct McpConnection {
    pub config: McpServerConfig,
    pub tools: Vec<ToolDefinition>,
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl McpConnection {
    /// Spawn and initialize an MCP server process.
    pub async fn connect(config: McpServerConfig) -> Result<Self, String> {
        let mut cmd = TokioCommand::new(&config.command);
        cmd.args(&config.args);
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn '{}': {}", config.command, e))?;

        let stdin = child.stdin.take().ok_or("No stdin")?;
        let stdout = child.stdout.take().ok_or("No stdout")?;
        let stdout = BufReader::new(stdout);

        let mut conn = Self {
            config,
            tools: vec![],
            child,
            stdin,
            stdout,
            next_id: 1,
        };

        // Initialize
        conn.send_request("initialize", serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "tracelean", "version": "0.1.0" }
        })).await?;

        // Discover tools
        let tools_resp = conn.send_request("tools/list", serde_json::json!({})).await?;
        if let Some(tools_arr) = tools_resp.get("tools").and_then(|t| t.as_array()) {
            for tool_val in tools_arr {
                if let Some(td) = parse_mcp_tool(tool_val) {
                    conn.tools.push(td);
                }
            }
        }

        Ok(conn)
    }

    /// Call a tool on this server.
    pub async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> Result<ToolResult, String> {
        let result = self.send_request("tools/call", serde_json::json!({
            "name": name,
            "arguments": arguments,
        })).await?;

        let content = result.get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);

        Ok(ToolResult {
            success: !is_error,
            content,
            data: Some(result),
        })
    }

    /// Send a JSON-RPC request and read the response.
    async fn send_request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.next_id;
        self.next_id += 1;

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        line.push('\n');

        self.stdin.write_all(line.as_bytes()).await
            .map_err(|e| format!("Write error: {}", e))?;
        self.stdin.flush().await
            .map_err(|e| format!("Flush error: {}", e))?;

        // Read response line
        let mut resp_line = String::new();
        self.stdout.read_line(&mut resp_line).await
            .map_err(|e| format!("Read error: {}", e))?;

        let resp: JsonRpcResponse = serde_json::from_str(&resp_line)
            .map_err(|e| format!("Parse error: {} (line: {})", e, resp_line.trim()))?;

        if let Some(err) = resp.error {
            return Err(format!("RPC error {}: {}", err.code, err.message));
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }

    /// Shut down the server process.
    pub async fn disconnect(&mut self) {
        let _ = self.child.kill().await;
    }
}

/// Manager for all MCP client connections.
pub struct McpClientManager {
    connections: Vec<McpConnection>,
}

impl McpClientManager {
    pub fn new() -> Self {
        Self { connections: vec![] }
    }

    /// Connect to all configured servers.
    pub async fn connect_all(&mut self, config: &McpConfig) -> Vec<String> {
        let mut errors = Vec::new();
        for server in config.active_servers() {
            match McpConnection::connect(server.clone()).await {
                Ok(conn) => self.connections.push(conn),
                Err(e) => errors.push(format!("{}: {}", server.name, e)),
            }
        }
        errors
    }

    /// Get all available tools from connected servers (prefixed with server name).
    pub fn all_tools(&self) -> Vec<(String, ToolDefinition)> {
        let mut tools = Vec::new();
        for conn in &self.connections {
            for tool in &conn.tools {
                tools.push((conn.config.name.clone(), tool.clone()));
            }
        }
        tools
    }

    /// Get all tool schemas directly (JSON Schema passthrough, no lossy conversion).
    pub fn all_tool_schemas(&self) -> Vec<super::provider::ToolSchema> {
        let mut schemas = Vec::new();
        for conn in &self.connections {
            for tool in &conn.tools {
                schemas.push(tool.to_tool_schema());
            }
        }
        schemas
    }

    /// Call a tool on the appropriate server.
    pub async fn call_tool(&mut self, server_name: &str, tool_name: &str, arguments: serde_json::Value) -> Result<ToolResult, String> {
        for conn in &mut self.connections {
            if conn.config.name == server_name {
                return conn.call_tool(tool_name, arguments).await;
            }
        }
        Err(format!("Server '{}' not connected.", server_name))
    }

    /// Disconnect all servers.
    pub async fn disconnect_all(&mut self) {
        for conn in &mut self.connections {
            conn.disconnect().await;
        }
        self.connections.clear();
    }

    /// Number of active connections.
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// List connected server names.
    pub fn connected_servers(&self) -> Vec<String> {
        self.connections.iter().map(|c| c.config.name.clone()).collect()
    }
}

/// Parse an MCP tool JSON object into our ToolDefinition.
fn parse_mcp_tool(val: &serde_json::Value) -> Option<ToolDefinition> {
    let name = val.get("name")?.as_str()?.to_string();
    let description = val.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();

    let mut parameters = Vec::new();
    if let Some(schema) = val.get("inputSchema") {
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            let required: Vec<String> = schema.get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();

            for (pname, pval) in props {
                let param_type = match pval.get("type").and_then(|t| t.as_str()).unwrap_or("string") {
                    "integer" | "number" => ParamType::Integer,
                    "boolean" => ParamType::Boolean,
                    "array" => ParamType::Array { item_type: Box::new(ParamType::String) },
                    "object" => ParamType::Object,
                    _ => ParamType::String,
                };
                let desc = pval.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                parameters.push(ToolParam {
                    name: pname.clone(),
                    param_type,
                    description: desc,
                    required: required.contains(pname),
                });
            }
        }
    }

    Some(ToolDefinition { name, description, parameters })
}

/// Parse an MCP tool JSON object directly into a ToolSchema (JSON Schema passthrough).
/// No lossy conversion — the inputSchema from MCP goes straight to the LLM's native tool calling.
#[allow(dead_code)]
fn parse_mcp_tool_schema(val: &serde_json::Value) -> Option<super::provider::ToolSchema> {
    let name = val.get("name")?.as_str()?.to_string();
    let description = val.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();

    // Pass through inputSchema as-is (it's already JSON Schema)
    let parameters = val.get("inputSchema").cloned().unwrap_or_else(|| {
        serde_json::json!({"type": "object", "properties": {}})
    });

    Some(super::provider::ToolSchema {
        tool_type: "function".into(),
        function: super::provider::ToolFunction {
            name,
            description,
            parameters,
        },
    })
}
