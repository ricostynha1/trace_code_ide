//! ACP transport — launches agent subprocesses and drives ACP connections.
//! Uses `agent-client-protocol` crate's AcpAgent + Client builder.
//!
//! The agent process communicates via stdio (newline-delimited JSON-RPC 2.0).
//! We are the Client; the subprocess is the Agent.

use super::server::AcpClientManager;
use super::types::{
    AcpAgentConfig, Client, Agent, AcpAgent,
    InitializeRequest, NewSessionRequest, PromptRequest, CancelNotification,
    ReadTextFileRequest, ReadTextFileResponse, WriteTextFileRequest, WriteTextFileResponse,
    RequestPermissionRequest, RequestPermissionResponse, RequestPermissionOutcome,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate,
    ContentBlock, TextContent, McpServer, McpServerStdio, EnvVariable,
    Implementation, ProtocolVersion,
};
use agent_client_protocol::{on_receive_notification, on_receive_request, ConnectionTo, Responder};
use std::sync::Arc;
use tokio::sync::mpsc;

/// MCP server configuration for forwarding to the agent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// Message from UI to ACP connection task.
#[derive(Debug)]
pub enum AcpCommand {
    /// Send a prompt to the agent's active session.
    Prompt { text: String },
    /// Cancel current operation.
    Cancel,
    /// Disconnect.
    Shutdown,
    /// UI responds to a permission request from agent.
    PermissionResponse { option_id: String },
}

/// Event from ACP connection task to UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpEvent {
    /// Agent sent a text chunk.
    AgentMessage { text: String, message_id: String },
    /// Agent produced a thought/reasoning step.
    AgentThought { text: String },
    /// Agent started a tool call.
    ToolCallStarted {
        tool_call_id: String,
        title: String,
        kind: Option<String>,
    },
    /// Agent tool call status update.
    ToolCallUpdate {
        tool_call_id: String,
        status: String,
    },
    /// Agent finished a tool call.
    ToolCallCompleted { tool_call_id: String },
    /// Agent requests permission from user.
    PermissionRequest {
        request_id: String,
        description: String,
        options: Vec<UiPermissionOption>,
    },
    /// Session created.
    SessionCreated { session_id: String },
    /// Agent disconnected or errored.
    Disconnected { reason: String },
}

/// A permission option presented to the user.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UiPermissionOption {
    pub id: String,
    pub label: String,
}

/// Extended agent config that includes MCP server forwarding.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AcpLaunchConfig {
    pub agent: AcpAgentConfig,
    /// MCP servers to forward to the agent in NewSessionRequest.
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    /// Project root / cwd for the agent session.
    pub project_root: Option<String>,
}

/// Handle to a running ACP agent connection.
pub struct AcpConnectionHandle {
    /// Send commands to the connection task.
    pub commands: mpsc::Sender<AcpCommand>,
    /// Agent name.
    pub agent_name: String,
}

impl AcpConnectionHandle {
    pub async fn prompt(&self, text: String) -> Result<(), String> {
        self.commands
            .send(AcpCommand::Prompt { text })
            .await
            .map_err(|_| "Agent connection closed".to_string())
    }

    pub async fn cancel(&self) -> Result<(), String> {
        self.commands
            .send(AcpCommand::Cancel)
            .await
            .map_err(|_| "Agent connection closed".to_string())
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        self.commands
            .send(AcpCommand::Shutdown)
            .await
            .map_err(|_| "Agent connection closed".to_string())
    }

    pub async fn respond_permission(&self, option_id: String) -> Result<(), String> {
        self.commands
            .send(AcpCommand::PermissionResponse { option_id })
            .await
            .map_err(|_| "Agent connection closed".to_string())
    }
}

/// Spawn an ACP agent subprocess and return a connection handle.
pub fn spawn_agent_connection(
    config: AcpLaunchConfig,
    manager: Arc<AcpClientManager>,
    event_sink: Arc<dyn crate::EventSink>,
) -> AcpConnectionHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel::<AcpCommand>(32);
    let agent_name = config.agent.name.clone();

    tokio::spawn(run_agent_connection(config, manager, event_sink, cmd_rx));

    AcpConnectionHandle {
        commands: cmd_tx,
        agent_name,
    }
}

/// Internal: runs the ACP connection lifecycle in a spawned task.
async fn run_agent_connection(
    config: AcpLaunchConfig,
    manager: Arc<AcpClientManager>,
    event_sink: Arc<dyn crate::EventSink>,
    cmd_rx: mpsc::Receiver<AcpCommand>,
) {
    let agent_cfg = &config.agent;

    // Build args for AcpAgent::from_args — format is [ENV=VAL..., command, args...]
    let mut spawn_args: Vec<String> = Vec::new();
    for (k, v) in &agent_cfg.env {
        spawn_args.push(format!("{}={}", k, v));
    }
    spawn_args.push(agent_cfg.command.clone());
    spawn_args.extend(agent_cfg.args.iter().cloned());

    let agent = match AcpAgent::from_args(spawn_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("[ACP] Failed to create agent '{}': {}", agent_cfg.name, e);
            emit_event(&event_sink, &AcpEvent::Disconnected {
                reason: format!("Failed to create agent: {}", e),
            });
            return;
        }
    };

    // Channel for passing permission responses from cmd loop to request handlers.
    let (perm_tx, perm_rx) = mpsc::channel::<String>(4);
    let perm_rx = Arc::new(tokio::sync::Mutex::new(perm_rx));

    // Shared cmd_rx wrapped so the connect_with closure can access it
    let cmd_rx = Arc::new(tokio::sync::Mutex::new(cmd_rx));

    let sink_notif = event_sink.clone();
    let sink_perm = event_sink.clone();
    let mgr_read = manager.clone();
    let mgr_write = manager.clone();
    let perm_rx_for_handler = perm_rx.clone();

    let result = Client
        .builder()
        .on_receive_notification(
            {
                let sink_notif = sink_notif.clone();
                async move |notif: SessionNotification, _cx: ConnectionTo<Agent>| {
                    handle_session_notification(&sink_notif, &notif);
                    Ok(())
                }
            },
            on_receive_notification!(),
        )
        .on_receive_request(
            {
                let mgr_read = mgr_read.clone();
                async move |req: ReadTextFileRequest, responder: Responder<ReadTextFileResponse>, _cx: ConnectionTo<Agent>| {
                    let path_str = req.path.to_string_lossy().to_string();
                    let content = match mgr_read.handle_read_file(&path_str, req.line, req.limit) {
                        Ok(c) => c,
                        Err(e) => format!("ERROR: {}", e),
                    };
                    let _ = responder.respond(ReadTextFileResponse::new(content));
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let mgr_write = mgr_write.clone();
                async move |req: WriteTextFileRequest, responder: Responder<WriteTextFileResponse>, _cx: ConnectionTo<Agent>| {
                    let path_str = req.path.to_string_lossy().to_string();
                    match mgr_write.handle_write_file(&path_str, &req.content) {
                        Ok(()) => {
                            let _ = responder.respond(WriteTextFileResponse::new());
                        }
                        Err(e) => {
                            eprintln!("[ACP] write_file error: {}", e);
                            let _ = responder.respond_with_internal_error(e);
                        }
                    }
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .on_receive_request(
            {
                let sink_perm = sink_perm.clone();
                let perm_rx = perm_rx_for_handler.clone();
                async move |req: RequestPermissionRequest, responder: Responder<RequestPermissionResponse>, _cx: ConnectionTo<Agent>| {
                    // Build description from tool_call info
                    let description = req.tool_call.fields.title.clone()
                        .unwrap_or_else(|| "Permission requested".to_string());

                    let options: Vec<UiPermissionOption> = req.options.iter().map(|o| {
                        UiPermissionOption {
                            id: o.option_id.0.to_string(),
                            label: o.name.clone(),
                        }
                    }).collect();

                    let request_id = uuid::Uuid::new_v4().to_string();
                    emit_event(&sink_perm, &AcpEvent::PermissionRequest {
                        request_id,
                        description,
                        options,
                    });

                    // Wait for UI response
                    let mut rx = perm_rx.lock().await;
                    let outcome = match rx.recv().await {
                        Some(option_id) => {
                            RequestPermissionOutcome::Selected(
                                SelectedPermissionOutcome::new(option_id)
                            )
                        }
                        None => RequestPermissionOutcome::Cancelled,
                    };

                    let _ = responder.respond(RequestPermissionResponse::new(outcome));
                    Ok(())
                }
            },
            on_receive_request!(),
        )
        .connect_with(agent, {
            let event_sink = event_sink.clone();
            let config = config.clone();
            let cmd_rx = cmd_rx.clone();
            let perm_tx = perm_tx.clone();
            async move |connection: ConnectionTo<Agent>| -> agent_client_protocol::Result<()> {
                // Phase 1: Initialize
                let _init_resp = connection
                    .send_request(
                        InitializeRequest::new(ProtocolVersion::LATEST)
                            .client_info(Implementation::new("tracelean", env!("CARGO_PKG_VERSION")))
                    )
                    .block_task()
                    .await
                    .map_err(|e| {
                        eprintln!("[ACP] Initialize failed: {}", e);
                        emit_event(&event_sink, &AcpEvent::Disconnected {
                            reason: format!("Initialize failed: {}", e),
                        });
                        e
                    })?;

                // Phase 2: NewSession
                let cwd = config.project_root.clone().unwrap_or_else(|| ".".to_string());

                let mcp_servers: Vec<McpServer> = config.mcp_servers.iter().map(|s| {
                    let env: Vec<EnvVariable> = s.env.iter()
                        .map(|(k, v)| EnvVariable::new(k, v))
                        .collect();
                    McpServer::Stdio(
                        McpServerStdio::new(&s.name, &s.command)
                            .args(s.args.clone())
                            .env(env)
                    )
                }).collect();

                let session_resp = connection
                    .send_request(
                        NewSessionRequest::new(&cwd).mcp_servers(mcp_servers)
                    )
                    .block_task()
                    .await
                    .map_err(|e| {
                        eprintln!("[ACP] NewSession failed: {}", e);
                        emit_event(&event_sink, &AcpEvent::Disconnected {
                            reason: format!("NewSession failed: {}", e),
                        });
                        e
                    })?;

                let session_id = session_resp.session_id.clone();
                emit_event(&event_sink, &AcpEvent::SessionCreated {
                    session_id: session_id.0.to_string(),
                });

                // Phase 3: Command loop
                run_command_loop(
                    &connection,
                    &cmd_rx,
                    &perm_tx,
                    &event_sink,
                    &session_id,
                ).await;

                emit_event(&event_sink, &AcpEvent::Disconnected {
                    reason: "shutdown".to_string(),
                });

                Ok(())
            }
        })
        .await;

    if let Err(e) = result {
        eprintln!("[ACP] Connection error for '{}': {}", agent_cfg.name, e);
        emit_event(&event_sink, &AcpEvent::Disconnected {
            reason: format!("Connection error: {}", e),
        });
    }
}

/// Command loop — reads AcpCommands from channel, dispatches to connection.
async fn run_command_loop(
    connection: &ConnectionTo<Agent>,
    cmd_rx: &Arc<tokio::sync::Mutex<mpsc::Receiver<AcpCommand>>>,
    perm_tx: &mpsc::Sender<String>,
    event_sink: &Arc<dyn crate::EventSink>,
    session_id: &agent_client_protocol::schema::v1::SessionId,
) {
    let mut rx = cmd_rx.lock().await;
    while let Some(cmd) = rx.recv().await {
        match cmd {
            AcpCommand::Prompt { text } => {
                let prompt = vec![ContentBlock::Text(TextContent::new(&text))];
                let req = PromptRequest::new(session_id.clone(), prompt);

                // Send prompt and handle response asynchronously via on_receiving_result
                // so we don't block the command loop while the agent thinks.
                let sent = connection.send_request(req);
                let sink = event_sink.clone();
                if let Err(e) = sent.on_receiving_result(move |result| async move {
                    match result {
                        Ok(_resp) => { /* turn complete; updates came via notifications */ }
                        Err(e) => {
                            eprintln!("[ACP] Prompt failed: {}", e);
                            emit_event(&sink, &AcpEvent::AgentMessage {
                                text: format!("[Error] Prompt failed: {}", e),
                                message_id: uuid::Uuid::new_v4().to_string(),
                            });
                        }
                    }
                    Ok(())
                }) {
                    eprintln!("[ACP] Failed to schedule prompt: {}", e);
                }
            }
            AcpCommand::Cancel => {
                if let Err(e) = connection.send_notification(
                    CancelNotification::new(session_id.clone())
                ) {
                    eprintln!("[ACP] Cancel failed: {}", e);
                }
            }
            AcpCommand::PermissionResponse { option_id } => {
                if let Err(e) = perm_tx.send(option_id).await {
                    eprintln!("[ACP] Permission response send failed: {}", e);
                }
            }
            AcpCommand::Shutdown => {
                break;
            }
        }
    }
}

/// Parse a SessionNotification and emit appropriate AcpEvents.
fn handle_session_notification(
    event_sink: &Arc<dyn crate::EventSink>,
    notif: &SessionNotification,
) {
    match &notif.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            let text = match &chunk.content {
                ContentBlock::Text(t) => t.text.clone(),
                _ => return,
            };
            let message_id = chunk.message_id
                .as_ref()
                .map(|id| id.0.to_string())
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            emit_event(event_sink, &AcpEvent::AgentMessage { text, message_id });
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            let text = match &chunk.content {
                ContentBlock::Text(t) => t.text.clone(),
                _ => return,
            };
            emit_event(event_sink, &AcpEvent::AgentThought { text });
        }
        SessionUpdate::ToolCall(tc) => {
            emit_event(event_sink, &AcpEvent::ToolCallStarted {
                tool_call_id: tc.tool_call_id.0.to_string(),
                title: tc.title.clone(),
                kind: Some(format!("{:?}", tc.kind)),
            });
        }
        SessionUpdate::ToolCallUpdate(tcu) => {
            let status = tcu.fields.status
                .as_ref()
                .map(|s| format!("{:?}", s))
                .unwrap_or_else(|| "running".to_string());
            emit_event(event_sink, &AcpEvent::ToolCallUpdate {
                tool_call_id: tcu.tool_call_id.0.to_string(),
                status,
            });
        }
        _ => {
            // Other update types (Plan, AvailableCommands, etc.) — ignore for now
        }
    }
}

/// Serialize and emit an AcpEvent.
fn emit_event(event_sink: &Arc<dyn crate::EventSink>, event: &AcpEvent) {
    if let Ok(payload) = serde_json::to_string(event) {
        event_sink.emit("acp-event", &payload);
    }
}
