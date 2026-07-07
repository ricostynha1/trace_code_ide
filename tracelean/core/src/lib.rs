//! TraceLean Core — shared engine for GUI and TUI frontends.
//! No Tauri dependency. Pure domain logic + service orchestration.

pub mod ai;
pub mod commands;
pub mod debug_nodes;
pub mod parser;
pub mod persistence;
pub mod requirements;
pub mod service;
pub mod state;
pub mod surgical_edit;
pub mod trace_graph;
pub mod undo_tree;

#[cfg(test)]
mod kw_test;

// Re-exports for convenience
pub use commands::Command;
pub use parser::SymbolTable;
pub use state::AppState;
pub use trace_graph::TraceGraph;
pub use ai::{InteractionLog, tracking::SessionStats, diff_pipeline::PendingDiff};
pub use ai::provider::{AiProvider, AiError, AiRequest, AiResponse};
pub use ai::mcp_client::McpClientManager;
pub use ai::tool_executor::AgentPermissions;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ─── EventSink Trait ─────────────────────────────────────────────────────────

/// Abstraction for push notifications from core to any UI.
/// Tauri implements via AppHandle.emit(); TUI implements via channel.
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: &str);
}

/// No-op sink for testing or headless usage.
pub struct NullSink;
impl EventSink for NullSink {
    fn emit(&self, _event: &str, _payload: &str) {}
}

// ─── Shared IPC Types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceGraphStats {
    pub nodes: usize,
    pub edges: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoTreeView {
    pub nodes: Vec<UndoNodeView>,
    pub current_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UndoNodeView {
    pub id: String,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub command_summary: String,
    pub file: Option<String>,
    pub timestamp: String,
    pub is_commit_point: bool,
    pub commit_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandLogEntry {
    pub index: usize,
    pub summary: String,
}

// ─── AI Settings ─────────────────────────────────────────────────────────────

/// Event emitted during tool call execution, for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub tool_name: String,
    pub status: ToolCallStatus,
    pub duration_ms: Option<u64>,
    /// Nesting depth (0 = top-level tool call, 1+ = nested sub-calls)
    pub depth: u32,
    /// Agent-provided reason (UI metadata, not part of MCP protocol).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Running,
    Completed,
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    pub active_provider: ai::ProviderKind,
    pub openrouter_api_key: Option<String>,
    pub bedrock_access_key: Option<String>,
    pub bedrock_secret_key: Option<String>,
    pub bedrock_region: Option<String>,
    pub selected_model: Option<ai::ModelConfig>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            active_provider: ai::ProviderKind::Mock,
            openrouter_api_key: None,
            bedrock_access_key: None,
            bedrock_secret_key: None,
            bedrock_region: None,
            selected_model: None,
        }
    }
}

// ─── SharedApp ───────────────────────────────────────────────────────────────

/// Central application handle shared between all frontends.
/// Each frontend holds an Arc<SharedApp> and calls methods on it.
pub struct SharedApp {
    pub state: Arc<Mutex<AppState>>,
    pub symbols: Arc<Mutex<SymbolTable>>,
    pub graph: Arc<Mutex<TraceGraph>>,
    pub ai_settings: Arc<Mutex<AiSettings>>,
    pub ai_log: Arc<Mutex<InteractionLog>>,
    pub ai_stats: Arc<Mutex<SessionStats>>,
    pub pending_diffs: Arc<Mutex<Vec<PendingDiff>>>,
    pub mcp_client: Arc<tokio::sync::Mutex<McpClientManager>>,
    pub agent_permissions: Arc<Mutex<HashMap<String, AgentPermissions>>>,
    pub event_sink: Arc<dyn EventSink>,
}

impl SharedApp {
    /// Create a new SharedApp with default state and the given event sink.
    pub fn new(event_sink: Arc<dyn EventSink>) -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::new())),
            symbols: Arc::new(Mutex::new(SymbolTable::new())),
            graph: Arc::new(Mutex::new(TraceGraph::new())),
            ai_settings: Arc::new(Mutex::new(AiSettings::default())),
            ai_log: Arc::new(Mutex::new(InteractionLog::new())),
            ai_stats: Arc::new(Mutex::new(SessionStats::default())),
            pending_diffs: Arc::new(Mutex::new(Vec::new())),
            mcp_client: Arc::new(tokio::sync::Mutex::new(McpClientManager::new())),
            agent_permissions: Arc::new(Mutex::new(HashMap::new())),
            event_sink,
        }
    }

    /// Create with NullSink (for tests / headless).
    pub fn new_headless() -> Self {
        Self::new(Arc::new(NullSink))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn command_summary(cmd: &Command) -> String {
    match cmd {
        Command::Insert { file, text, offset, .. } => {
            let preview = if text.len() > 20 { format!("{}...", &text[..20]) } else { text.clone() };
            format!("Insert @{}:{} \"{}\"", file.display(), offset, preview)
        }
        Command::Delete { file, offset, len, .. } => format!("Delete @{}:{} len={}", file.display(), offset, len),
        Command::SetCursor { file, new_pos, .. } => format!("Cursor @{}:{}:{}", file.display(), new_pos.line, new_pos.col),
        Command::SetSelection { file, .. } => format!("Select @{}", file.display()),
        Command::CreateFile { path } => format!("Create {}", path.display()),
        Command::DeleteFile { path, .. } => format!("Delete file {}", path.display()),
        Command::RenameFile { from, to } => format!("Rename {} → {}", from.display(), to.display()),
        Command::Batch { commands } => {
            if commands.is_empty() {
                "Initial".to_string()
            } else {
                format!("Batch ({} cmds)", commands.len())
            }
        }
    }
}

pub fn command_file(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Insert { file, .. } | Command::Delete { file, .. }
        | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => Some(file.to_string_lossy().to_string()),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => Some(path.to_string_lossy().to_string()),
        Command::RenameFile { from, .. } => Some(from.to_string_lossy().to_string()),
        Command::Batch { .. } => None,
    }
}

pub fn command_affects_file(cmd: &Command, filter: &str) -> bool {
    match cmd {
        Command::Insert { file, .. } | Command::Delete { file, .. }
        | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => file.to_string_lossy() == filter,
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.to_string_lossy() == filter,
        Command::RenameFile { from, to } => from.to_string_lossy() == filter || to.to_string_lossy() == filter,
        Command::Batch { commands } => commands.iter().any(|c| command_affects_file(c, filter)),
    }
}
