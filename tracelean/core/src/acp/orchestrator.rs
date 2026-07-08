//! Multi-agent orchestrator — manages simultaneous agent connections,
//! routes prompts by name, supports sub-agent delegation, tracks conversation
//! state per agent, and provides a unified event stream.

use super::server::AcpClientManager;
use super::transport::{AcpConnectionHandle, AcpLaunchConfig, spawn_agent_connection};
use super::types::AcpAgentConfig;
use crate::EventSink;
use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Utc};

/// Orchestrates multiple ACP agent connections.
pub struct AgentOrchestrator {
    manager: Arc<AcpClientManager>,
    active_agents: HashMap<String, AgentSession>,
    handles: HashMap<String, AcpConnectionHandle>,
    event_sink: Arc<dyn EventSink>,
}

/// Tracks a single agent's session state and conversation history.
#[derive(Debug, Clone)]
pub struct AgentSession {
    pub agent_name: String,
    pub conversation: Vec<ConversationTurn>,
    pub status: AgentSessionStatus,
}

/// Current processing status of an agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSessionStatus {
    Idle,
    Processing,
    WaitingPermission,
    Aborted,
}

/// A single turn in the agent conversation.
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub role: TurnRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub tool_calls: Vec<ToolCallRecord>,
}

/// Who produced this turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Agent,
}

/// Record of a tool invocation within a turn.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub status: String,
    pub duration_ms: Option<u64>,
}

impl AgentOrchestrator {
    /// Create a new orchestrator backed by the given manager and event sink.
    pub fn new(manager: Arc<AcpClientManager>, event_sink: Arc<dyn EventSink>) -> Self {
        Self {
            manager,
            active_agents: HashMap::new(),
            handles: HashMap::new(),
            event_sink,
        }
    }

    /// Spawn a new agent connection from config. Returns the agent name.
    pub async fn spawn_agent(&mut self, config: AcpAgentConfig) -> Result<String, String> {
        let name = config.name.clone();

        if self.active_agents.contains_key(&name) {
            return Err(format!("Agent '{}' already active", name));
        }

        // Register with the underlying manager
        self.manager.connect_agent(config.clone()).await?;

        // Spawn the transport-level connection
        let launch_config = AcpLaunchConfig {
            agent: config,
            mcp_servers: Vec::new(),
            project_root: None,
        };
        let handle = spawn_agent_connection(
            launch_config,
            Arc::clone(&self.manager),
            Arc::clone(&self.event_sink),
        );

        let session = AgentSession {
            agent_name: name.clone(),
            conversation: Vec::new(),
            status: AgentSessionStatus::Idle,
        };

        self.active_agents.insert(name.clone(), session);
        self.handles.insert(name.clone(), handle);

        self.event_sink.emit("orchestrator", &serde_json::json!({
            "event": "agent_spawned",
            "agent": &name,
        }).to_string());

        Ok(name)
    }

    /// Send a prompt to a specific agent by name.
    pub async fn prompt(&mut self, agent_name: &str, text: &str) -> Result<(), String> {
        let session = self.active_agents.get_mut(agent_name)
            .ok_or_else(|| format!("Agent '{}' not found", agent_name))?;

        session.status = AgentSessionStatus::Processing;
        session.conversation.push(ConversationTurn {
            role: TurnRole::User,
            content: text.to_string(),
            timestamp: Utc::now(),
            tool_calls: Vec::new(),
        });

        let handle = self.handles.get(agent_name)
            .ok_or_else(|| format!("No handle for agent '{}'", agent_name))?;

        handle.prompt(text.to_string()).await?;

        Ok(())
    }

    /// Cancel the current operation for a specific agent.
    pub async fn cancel(&mut self, agent_name: &str) -> Result<(), String> {
        let session = self.active_agents.get_mut(agent_name)
            .ok_or_else(|| format!("Agent '{}' not found", agent_name))?;

        session.status = AgentSessionStatus::Aborted;

        let handle = self.handles.get(agent_name)
            .ok_or_else(|| format!("No handle for agent '{}'", agent_name))?;

        handle.cancel().await
    }

    /// Cancel all active agents.
    pub async fn cancel_all(&mut self) {
        let names: Vec<String> = self.active_agents.keys().cloned().collect();
        for name in names {
            let _ = self.cancel(&name).await;
        }
    }

    /// Shut down and disconnect a specific agent.
    pub async fn shutdown_agent(&mut self, name: &str) -> Result<(), String> {
        if let Some(handle) = self.handles.remove(name) {
            let _ = handle.shutdown().await;
        }

        self.active_agents.remove(name);
        self.manager.disconnect_agent(name).await?;

        self.event_sink.emit("orchestrator", &serde_json::json!({
            "event": "agent_shutdown",
            "agent": name,
        }).to_string());

        Ok(())
    }

    /// List all active agent sessions.
    pub fn list_agents(&self) -> Vec<&AgentSession> {
        self.active_agents.values().collect()
    }

    /// Get conversation history for a specific agent.
    pub fn get_conversation(&self, name: &str) -> Option<&[ConversationTurn]> {
        self.active_agents.get(name).map(|s| s.conversation.as_slice())
    }

    /// Record an agent response turn (called when agent event arrives).
    pub fn record_agent_turn(&mut self, agent_name: &str, content: String, tool_calls: Vec<ToolCallRecord>) {
        if let Some(session) = self.active_agents.get_mut(agent_name) {
            session.conversation.push(ConversationTurn {
                role: TurnRole::Agent,
                content,
                timestamp: Utc::now(),
                tool_calls,
            });
            session.status = AgentSessionStatus::Idle;
        }
    }

    /// Delegate: agent A requests spawning agent B as sub-agent.
    /// Returns the sub-agent name on success.
    pub async fn delegate(&mut self, _parent_agent: &str, sub_config: AcpAgentConfig) -> Result<String, String> {
        self.event_sink.emit("orchestrator", &serde_json::json!({
            "event": "delegation",
            "parent": _parent_agent,
            "child": &sub_config.name,
        }).to_string());

        self.spawn_agent(sub_config).await
    }
}
