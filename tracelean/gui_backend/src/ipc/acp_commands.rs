//! ACP IPC commands — connect/disconnect/prompt external ACP agents.

use crate::AcpManagerWrapper;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpAgentInfo {
    pub name: String,
    pub status: String,
    pub session_id: Option<String>,
}

/// Connect to an external ACP agent.
#[tauri::command]
pub async fn acp_connect_agent(
    acp: State<'_, AcpManagerWrapper>,
    name: String,
    command: String,
    args: Vec<String>,
) -> Result<String, String> {
    let config = tracelean_core::acp::AcpAgentConfig {
        name,
        command,
        args,
        env: std::collections::HashMap::new(),
        mcp_servers: Vec::new(),
    };
    acp.0.connect_agent(config).await
}

/// Disconnect an ACP agent by name.
#[tauri::command]
pub async fn acp_disconnect_agent(
    acp: State<'_, AcpManagerWrapper>,
    name: String,
) -> Result<(), String> {
    acp.0.disconnect_agent(&name).await
}

/// List all connected ACP agents.
#[tauri::command]
pub async fn acp_list_agents(
    acp: State<'_, AcpManagerWrapper>,
) -> Result<Vec<AcpAgentInfo>, String> {
    let agents = acp.0.list_agents().await;
    Ok(agents.iter().map(|a| AcpAgentInfo {
        name: a.config.name.clone(),
        status: format!("{:?}", a.status),
        session_id: a.session_id.clone(),
    }).collect())
}

/// Get ACP connection status summary.
#[tauri::command]
pub async fn acp_status(
    acp: State<'_, AcpManagerWrapper>,
) -> Result<String, String> {
    let agents = acp.0.list_agents().await;
    if agents.is_empty() {
        Ok("No ACP agents connected".to_string())
    } else {
        Ok(format!("{} agent(s) connected: {}",
            agents.len(),
            agents.iter().map(|a| a.config.name.as_str()).collect::<Vec<_>>().join(", ")
        ))
    }
}

/// Send a prompt to a named ACP agent.
#[tauri::command]
pub async fn acp_prompt(
    acp: State<'_, AcpManagerWrapper>,
    name: String,
    prompt: String,
) -> Result<(), String> {
    acp.0.prompt_agent(&name, prompt).await
}

/// Cancel the current operation on a named ACP agent.
#[tauri::command]
pub async fn acp_cancel(
    acp: State<'_, AcpManagerWrapper>,
    name: String,
) -> Result<(), String> {
    acp.0.cancel_agent(&name).await
}

/// Respond to a permission request from an ACP agent.
#[tauri::command]
pub async fn acp_permission_respond(
    acp: State<'_, AcpManagerWrapper>,
    name: String,
    request_id: String,
    approved: bool,
) -> Result<(), String> {
    // Map boolean approval to option_id; request_id used for routing in full impl
    let option_id = if approved {
        "approve".to_string()
    } else {
        "deny".to_string()
    };
    let _ = &request_id; // Will be used when we track pending permission requests
    acp.0.respond_permission(&name, option_id).await
}
