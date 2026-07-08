//! ACP types — re-exports from the `agent-client-protocol` crate.
//! We are the CLIENT (IDE). External agents are subprocesses communicating via ACP.
//!
//! The official crate provides all wire types, traits, and transport.
//! This module adds our IDE-specific context on top.

pub use agent_client_protocol::schema::ProtocolVersion;
pub use agent_client_protocol::{Client, Agent, ConnectTo, AcpAgent, Stdio, ActiveSession};
pub use agent_client_protocol::schema::v1::{
    InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
    PromptRequest, PromptResponse, CancelNotification, SessionNotification, SessionUpdate,
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
    RequestPermissionRequest, RequestPermissionResponse, RequestPermissionOutcome,
    SelectedPermissionOutcome, ContentBlock, TextContent, McpServer, McpServerStdio,
    EnvVariable, Implementation, ToolCall, ToolCallUpdate as SchemaToolCallUpdate,
};

use serde::{Deserialize, Serialize};

/// Status of the ACP client connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AcpConnectionStatus {
    Disconnected,
    Connecting,
    Initialized,
    SessionActive,
    Error { message: String },
}

/// MCP server configuration to be passed to an ACP agent.
/// Compatible with `crate::ai::mcp_client::McpServerConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpMcpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub disabled: bool,
}

/// Configuration for launching an external ACP agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpAgentConfig {
    /// Display name for this agent.
    pub name: String,
    /// Command to launch the agent subprocess.
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables to set.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// MCP servers to expose to this agent.
    #[serde(default)]
    pub mcp_servers: Vec<AcpMcpServerConfig>,
}

/// State tracked for an active ACP agent connection.
#[derive(Debug, Clone)]
pub struct AcpAgentState {
    pub config: AcpAgentConfig,
    pub status: AcpConnectionStatus,
    pub session_id: Option<String>,
}

impl AcpAgentState {
    pub fn new(config: AcpAgentConfig) -> Self {
        Self {
            config,
            status: AcpConnectionStatus::Disconnected,
            session_id: None,
        }
    }
}
