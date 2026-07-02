//! TraceLean IDE - Rust backend
//! Thin orchestrator: module declarations, shared state, and app entry point.
//! IPC commands live in `ipc/` submodules.

pub mod ai;
pub mod commands;
pub mod debug_nodes;
pub mod ipc;
pub mod parser;
pub mod persistence;
pub mod requirements;
pub mod service;
pub mod state;
pub mod trace_graph;
pub mod undo_tree;

use commands::Command;
use parser::SymbolTable;
use serde::{Deserialize, Serialize};
use state::AppState;
use trace_graph::TraceGraph;
use ai::{InteractionLog, tracking::SessionStats};
use std::collections::HashMap;
use std::sync::Mutex;

// ─── Shared State Wrappers ───────────────────────────────────────────────────

pub struct AppStateWrapper(pub Mutex<AppState>);
pub struct SymbolTableWrapper(pub Mutex<SymbolTable>);
pub struct TraceGraphWrapper(pub Mutex<TraceGraph>);

pub struct UndoTreeCache {
    pub version: u64,
    pub view: Option<UndoTreeView>,
    pub file_filter: Option<String>,
}

impl UndoTreeCache {
    pub fn new() -> Self {
        Self { version: 0, view: None, file_filter: None }
    }
}

pub struct UndoTreeCacheWrapper(pub Mutex<UndoTreeCache>);

// ─── AI State ────────────────────────────────────────────────────────────────

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

pub struct AiSettingsWrapper(pub Mutex<AiSettings>);
pub struct AiLogWrapper(pub Mutex<InteractionLog>);
pub struct AiSessionStatsWrapper(pub Mutex<SessionStats>);
pub struct MockPendingWrapper(pub Mutex<Vec<ai::mock::MockPendingRequest>>);
pub struct MockProviderWrapper(pub std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<ai::mock::MockProvider>>>>);
pub struct PendingDiffsWrapper(pub Mutex<Vec<ai::diff_pipeline::PendingDiff>>);

// ─── MCP & Permissions State ─────────────────────────────────────────────────

pub struct McpHostPermissionsWrapper(pub Mutex<ai::AgentPermissions>);
pub struct McpClientWrapper(pub tokio::sync::Mutex<ai::mcp_client::McpClientManager>);
pub struct AgentPermissionsStore(pub Mutex<HashMap<String, ai::AgentPermissions>>);

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

// ─── Helpers (used by IPC modules) ──────────────────────────────────────────

pub fn invalidate_undo_cache(cache: &tauri::State<'_, UndoTreeCacheWrapper>) {
    if let Ok(mut c) = cache.0.lock() {
        c.view = None;
    }
}

pub fn command_summary(cmd: &Command) -> String {
    match cmd {
        Command::Insert { file, text, offset, .. } => {
            let preview = if text.len() > 20 { format!("{}...", &text[..20]) } else { text.clone() };
            format!("Insert @{}:{} \"{}\"", file.display(), offset, preview)
        }
        Command::Delete { file, offset, len, .. } => format!("Delete @{}:{} len={}", file.display(), offset, len),
        Command::Replace { file, offset, .. } => format!("Replace @{}:{}", file.display(), offset),
        Command::SetCursor { file, new_pos, .. } => format!("Cursor @{}:{}:{}", file.display(), new_pos.line, new_pos.col),
        Command::SetSelection { file, .. } => format!("Select @{}", file.display()),
        Command::CreateFile { path } => format!("Create {}", path.display()),
        Command::DeleteFile { path, .. } => format!("Delete file {}", path.display()),
        Command::RenameFile { from, to } => format!("Rename {} → {}", from.display(), to.display()),
        Command::Batch { commands } => format!("Batch ({} cmds)", commands.len()),
    }
}

pub fn command_file(cmd: &Command) -> Option<String> {
    match cmd {
        Command::Insert { file, .. } | Command::Delete { file, .. }
        | Command::Replace { file, .. } | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => Some(file.to_string_lossy().to_string()),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => Some(path.to_string_lossy().to_string()),
        Command::RenameFile { from, .. } => Some(from.to_string_lossy().to_string()),
        Command::Batch { .. } => None,
    }
}

pub fn command_affects_file(cmd: &Command, filter: &str) -> bool {
    match cmd {
        Command::Insert { file, .. } | Command::Delete { file, .. }
        | Command::Replace { file, .. } | Command::SetCursor { file, .. }
        | Command::SetSelection { file, .. } => file.to_string_lossy() == filter,
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.to_string_lossy() == filter,
        Command::RenameFile { from, to } => from.to_string_lossy() == filter || to.to_string_lossy() == filter,
        Command::Batch { commands } => commands.iter().any(|c| command_affects_file(c, filter)),
    }
}

/// Helper: get a boxed AI provider based on current settings.
pub async fn get_provider(
    settings: &tauri::State<'_, AiSettingsWrapper>,
    mock_provider: &tauri::State<'_, MockProviderWrapper>,
) -> Result<Box<dyn ai::provider::AiProvider + Send + Sync>, String> {
    let (provider_kind, or_key, br_access, br_secret, br_region) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        (
            s.active_provider.clone(),
            s.openrouter_api_key.clone(),
            s.bedrock_access_key.clone(),
            s.bedrock_secret_key.clone(),
            s.bedrock_region.clone(),
        )
    };

    match provider_kind {
        ai::ProviderKind::OpenRouter => {
            let key = or_key.ok_or("OpenRouter API key not set")?;
            Ok(Box::new(ai::openrouter::OpenRouterProvider::new(key)))
        }
        ai::ProviderKind::Bedrock => {
            let access = br_access.ok_or("Bedrock access key not set")?;
            let secret = br_secret.ok_or("Bedrock secret key not set")?;
            let region = br_region.unwrap_or_else(|| "us-east-1".into());
            Ok(Box::new(ai::bedrock::BedrockProvider::new(access, secret, region)))
        }
        ai::ProviderKind::Mock => {
            let mut guard = mock_provider.0.lock().await;
            if guard.is_none() {
                let (p, _rx) = ai::mock::MockProvider::new();
                *guard = Some(std::sync::Arc::new(p));
            }
            drop(guard);
            Ok(Box::new(SharedMockProvider { inner: mock_provider.0.clone() }))
        }
    }
}

/// Thin wrapper around the shared MockProvider Arc.
struct SharedMockProvider {
    inner: std::sync::Arc<tokio::sync::Mutex<Option<std::sync::Arc<ai::mock::MockProvider>>>>,
}

#[async_trait::async_trait]
impl ai::provider::AiProvider for SharedMockProvider {
    async fn complete(&self, request: &ai::AiRequest) -> Result<ai::AiResponse, ai::provider::AiError> {
        let provider = {
            let guard = self.inner.lock().await;
            guard.as_ref().ok_or_else(|| ai::provider::AiError {
                kind: ai::provider::AiErrorKind::ProviderError,
                message: "Mock provider not initialized".into(),
                retryable: false,
            })?.clone()
        };
        provider.complete(request).await
    }

    fn name(&self) -> &str { "Mock (Debug)" }

    async fn list_models(&self) -> Result<Vec<ai::ModelConfig>, ai::provider::AiError> {
        Ok(vec![ai::ModelConfig {
            provider: ai::ProviderKind::Mock,
            model_id: "mock-debug".into(),
            display_name: "Mock Agent (Debug)".into(),
            max_tokens: 99999,
            temperature: 0.0,
            input_cost_per_m: 0.0,
            output_cost_per_m: 0.0,
            cached_input_cost_per_m: 0.0,
            extra_params: None,
        }])
    }
}

// ─── App Entry Point ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppStateWrapper(Mutex::new(AppState::new())))
        .manage(SymbolTableWrapper(Mutex::new(SymbolTable::new())))
        .manage(TraceGraphWrapper(Mutex::new(TraceGraph::new())))
        .manage(UndoTreeCacheWrapper(Mutex::new(UndoTreeCache::new())))
        .manage(AiSettingsWrapper(Mutex::new(AiSettings::default())))
        .manage(AiLogWrapper(Mutex::new(InteractionLog::new())))
        .manage(AiSessionStatsWrapper(Mutex::new(SessionStats::default())))
        .manage(MockPendingWrapper(Mutex::new(Vec::new())))
        .manage(MockProviderWrapper(std::sync::Arc::new(tokio::sync::Mutex::new(None))))
        .manage(PendingDiffsWrapper(Mutex::new(Vec::new())))
        .manage(McpHostPermissionsWrapper(Mutex::new(ai::AgentPermissions::full_access("mcp-host"))))
        .manage(McpClientWrapper(tokio::sync::Mutex::new(ai::mcp_client::McpClientManager::new())))
        .manage(AgentPermissionsStore(Mutex::new(HashMap::new())))
        .invoke_handler(tauri::generate_handler![
            // Editor
            ipc::editor::apply_command,
            ipc::editor::undo,
            ipc::editor::redo,
            ipc::editor::get_file_content,
            ipc::editor::open_project,
            ipc::editor::list_files,
            ipc::editor::open_file,
            ipc::editor::save_file,
            ipc::editor::save_checkpoint,
            ipc::editor::get_undo_tree,
            ipc::editor::get_command_log,
            ipc::editor::jump_to_node,
            ipc::editor::clear_undo_tree,
            ipc::editor::get_undo_node_diff,
            ipc::editor::parse_file_symbols,
            ipc::editor::get_file_symbols,
            ipc::editor::get_highlights,
            ipc::editor::get_highlights_legacy,
            ipc::editor::parse_project,
            ipc::editor::get_initial_project,
            // Trace & Requirements
            ipc::trace::build_trace_graph,
            ipc::trace::query_requirement_trace,
            ipc::trace::query_code_trace,
            ipc::trace::update_trace_graph_file,
            ipc::trace::get_trace_graph_stats,
            ipc::trace::get_full_trace_graph,
            ipc::trace::list_requirements,
            ipc::trace::update_requirement_status,
            ipc::trace::create_requirement,
            ipc::trace::check_lean_spec,
            ipc::trace::get_editor_mode,
            ipc::trace::navigate_trace_link,
            // AI
            ipc::ai_commands::get_ai_settings,
            ipc::ai_commands::update_ai_settings,
            ipc::ai_commands::get_ai_models,
            ipc::ai_commands::get_ai_session_stats,
            ipc::ai_commands::get_ai_interaction_log,
            ipc::ai_commands::get_ai_interaction_detail,
            ipc::ai_commands::get_mock_pending,
            ipc::ai_commands::mock_submit_response,
            ipc::ai_commands::ai_chat,
            ipc::ai_commands::get_prompt_templates,
            ipc::ai_commands::assemble_context_for_requirement,
            ipc::ai_commands::assemble_context_for_file,
            ipc::ai_commands::run_agent_elicitation,
            ipc::ai_commands::run_agent_formalisation,
            ipc::ai_commands::run_agent_implementation,
            ipc::ai_commands::run_agent_repair,
            ipc::ai_commands::get_pending_diffs,
            ipc::ai_commands::accept_diff_hunk,
            ipc::ai_commands::reject_diff_hunk,
            ipc::ai_commands::apply_accepted_hunks,
            ipc::ai_commands::discard_pending_diff,
            // MCP & Permissions & Streaming
            ipc::mcp_commands::mcp_list_tools,
            ipc::mcp_commands::mcp_call_tool,
            ipc::mcp_commands::mcp_handle_jsonrpc,
            ipc::mcp_commands::mcp_client_connect,
            ipc::mcp_commands::mcp_client_list_tools,
            ipc::mcp_commands::mcp_client_call_tool,
            ipc::mcp_commands::mcp_client_disconnect,
            ipc::mcp_commands::mcp_client_status,
            ipc::mcp_commands::get_agent_permissions,
            ipc::mcp_commands::set_agent_permissions,
            ipc::mcp_commands::list_agent_permissions,
            ipc::mcp_commands::ai_chat_stream,
            ipc::mcp_commands::get_agent_tools,
            ipc::mcp_commands::get_agent_tools_prompt,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
