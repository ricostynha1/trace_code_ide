//! ACP (Agent Client Protocol) module.
//!
//! Makes our IDE an ACP Client — external agents (Claude Code, Codex, Gemini CLI, etc.)
//! can connect as subprocesses and interact with the IDE via JSON-RPC 2.0 over stdio.
//!
//! Architecture:
//!   IDE (Client) ←→ Agent (subprocess)
//!   - Client sends: initialize, session/new, session/prompt, session/cancel
//!   - Agent sends: session/update notifications, fs/read_text_file, fs/write_text_file
//!
//! Built on the official `agent-client-protocol` Rust crate (v1.2.0).

pub mod types;
pub mod server;
pub mod transport;
pub mod builtin_agent;
pub mod orchestrator;

pub use types::{AcpAgentConfig, AcpAgentState, AcpConnectionStatus, AcpMcpServerConfig};
pub use server::AcpClientManager;
pub use transport::{
    AcpCommand, AcpEvent, AcpConnectionHandle, AcpLaunchConfig,
    McpServerConfig, UiPermissionOption, spawn_agent_connection,
};
pub use orchestrator::{AgentOrchestrator, AgentSession, AgentSessionStatus, ConversationTurn, TurnRole, ToolCallRecord};
