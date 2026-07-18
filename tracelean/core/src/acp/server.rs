//! ACP Client implementation — our IDE acts as the ACP Client.
//! Handles incoming requests from agents (fs/read_text_file, fs/write_text_file,
//! session/request_permission) and sends outgoing requests (initialize, session/new, session/prompt).
//!
//! Uses the `agent-client-protocol` crate's Client builder pattern.

use crate::SharedApp;
use crate::commands::Command;
use super::types::{AcpAgentConfig, AcpAgentState, AcpConnectionStatus};
use super::transport::{AcpConnectionHandle, AcpLaunchConfig, McpServerConfig, spawn_agent_connection};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// ACP Client manager — manages connections to external ACP agents.
pub struct AcpClientManager {
    /// Reference to shared app state for tool execution.
    app: Arc<SharedApp>,
    /// Active agent connections.
    agents: Mutex<Vec<AcpAgentState>>,
    /// Live connection handles keyed by agent name.
    connections: Mutex<HashMap<String, AcpConnectionHandle>>,
}

impl AcpClientManager {
    pub fn new(app: Arc<SharedApp>) -> Self {
        Self {
            app,
            agents: Mutex::new(Vec::new()),
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// Launch and connect to an external ACP agent.
    pub async fn connect_agent(self: &Arc<Self>, config: AcpAgentConfig) -> Result<String, String> {
        let agent_name = config.name.clone();
        let mut agents = self.agents.lock().await;

        // Check not already connected
        if agents.iter().any(|a| a.config.name == agent_name) {
            return Err(format!("Agent '{}' already connected", agent_name));
        }

        let mut state = AcpAgentState::new(config.clone());
        state.status = AcpConnectionStatus::Connecting;
        agents.push(state);
        drop(agents);

        // Read MCP config from .tracelean/mcp.json
        let (mcp_servers, project_root) = {
            let app_state = self.app.state.lock().map_err(|e| e.to_string())?;
            let root = app_state.project_root().cloned().unwrap_or_default();
            let config_path = root.join(".tracelean").join("mcp.json");
            let servers = if let Ok(content) = std::fs::read_to_string(&config_path) {
                #[derive(serde::Deserialize)]
                struct McpFile { #[serde(default)] servers: Vec<McpServerConfig> }
                serde_json::from_str::<McpFile>(&content)
                    .map(|f| f.servers)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            (servers, root)
        };

        // Build launch config
        let launch_config = AcpLaunchConfig {
            agent: config,
            mcp_servers,
            project_root: Some(project_root.to_string_lossy().to_string()),
        };

        // Spawn the agent connection
        let handle = spawn_agent_connection(
            launch_config,
            Arc::clone(self),
            Arc::clone(&self.app.event_sink),
        );

        // Store handle
        self.connections.lock().await.insert(agent_name.clone(), handle);

        // Mark as initialized
        let mut agents = self.agents.lock().await;
        if let Some(a) = agents.iter_mut().find(|a| a.config.name == agent_name) {
            a.status = AcpConnectionStatus::Initialized;
        }

        Ok(agent_name)
    }

    /// Disconnect an agent by name.
    pub async fn disconnect_agent(&self, name: &str) -> Result<(), String> {
        // Shutdown the connection handle first
        if let Some(handle) = self.connections.lock().await.remove(name) {
            let _ = handle.shutdown().await;
        }

        let mut agents = self.agents.lock().await;
        let pos = agents.iter().position(|a| a.config.name == name)
            .ok_or_else(|| format!("Agent '{}' not found", name))?;
        agents.remove(pos);
        Ok(())
    }

    /// Send a prompt to a connected agent.
    pub async fn prompt_agent(&self, name: &str, text: String) -> Result<(), String> {
        let conns = self.connections.lock().await;
        let handle = conns.get(name)
            .ok_or_else(|| format!("Agent '{}' not connected", name))?;
        handle.prompt(text).await
    }

    /// Cancel the current operation for an agent.
    pub async fn cancel_agent(&self, name: &str) -> Result<(), String> {
        let conns = self.connections.lock().await;
        let handle = conns.get(name)
            .ok_or_else(|| format!("Agent '{}' not connected", name))?;
        handle.cancel().await
    }

    /// Respond to a permission request from an agent.
    pub async fn respond_permission(&self, name: &str, option_id: String) -> Result<(), String> {
        let conns = self.connections.lock().await;
        let handle = conns.get(name)
            .ok_or_else(|| format!("Agent '{}' not connected", name))?;
        handle.respond_permission(option_id).await
    }

    /// Get a reference to the connections map if agent exists.
    /// Caller can access the handle via the returned guard.
    pub async fn get_agent_handle(&self, name: &str) -> Option<tokio::sync::MutexGuard<'_, HashMap<String, AcpConnectionHandle>>> {
        let conns = self.connections.lock().await;
        if conns.contains_key(name) {
            Some(conns)
        } else {
            None
        }
    }

    /// Get status of all connected agents.
    pub async fn list_agents(&self) -> Vec<AcpAgentState> {
        self.agents.lock().await.clone()
    }

    /// Handle an agent's fs/read_text_file request.
    /// Reads from IDE buffer (unsaved content) or falls back to disk.
    pub fn handle_read_file(&self, path: &str, line: Option<u32>, limit: Option<u32>) -> Result<String, String> {
        let state = self.app.state.lock().map_err(|e| e.to_string())?;
        let project_root = state.project_root().cloned().unwrap_or_default();

        // Try buffer first (may have unsaved edits)
        let abs_path = PathBuf::from(path);
        let rel_path = abs_path.strip_prefix(&project_root)
            .unwrap_or(&abs_path)
            .to_path_buf();

        let content = if let Some(buf_content) = state.get_content(&rel_path) {
            buf_content.to_string()
        } else {
            std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read '{}': {}", path, e))?
        };

        // Apply line/limit filtering
        let lines: Vec<&str> = content.lines().collect();
        let start = line.map(|l| (l as usize).saturating_sub(1)).unwrap_or(0);
        let end = limit.map(|l| start + l as usize).unwrap_or(lines.len()).min(lines.len());

        if start >= lines.len() {
            return Ok(String::new());
        }

        Ok(lines[start..end].join("\n"))
    }

    /// Handle an agent's fs/write_text_file request.
    /// Writes through the command system (preserving undo chain).
    pub fn handle_write_file(&self, path: &str, content: &str) -> Result<(), String> {
        let mut state = self.app.state.lock().map_err(|e| e.to_string())?;
        let project_root = state.project_root().cloned().unwrap_or_default();

        let abs_path = PathBuf::from(path);
        let rel_path = abs_path.strip_prefix(&project_root)
            .unwrap_or(&abs_path)
            .to_path_buf();
        let full_path = project_root.join(&rel_path);

        // Ensure parent directory exists
        if let Some(parent) = full_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let file_exists = full_path.exists() || state.get_content(&rel_path).is_some();

        if !file_exists {
            let _ = state.apply(Command::CreateFile { path: rel_path.clone() });
        }

        // Get existing content from buffer or disk
        let existing = if let Some(c) = state.get_content(&rel_path) {
            c.to_string()
        } else {
            std::fs::read_to_string(&full_path).unwrap_or_default()
        };

        // Load into buffer if not already there
        if state.get_content(&rel_path).is_none() {
            state.load_file(rel_path.clone(), existing.clone());
        }

        // Apply as command
        state
            .apply(Command::replace(rel_path.clone(), 0, existing, content.to_string()))
            .map_err(|e| format!("Edit rejected: {}", e))?;

        // Write to disk
        std::fs::write(&full_path, content)
            .map_err(|e| format!("Failed to write '{}': {}", path, e))?;

        // Emit event so UI refreshes
        self.app.event_sink.emit("file-changed", &serde_json::json!({"path": rel_path.to_string_lossy()}).to_string());

        Ok(())
    }
}
