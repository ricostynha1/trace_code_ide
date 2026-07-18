//! TraceLean IDE — Tauri GUI backend.
//! Thin shell: state wrappers, IPC dispatch, Tauri app entry point.
//! All domain logic lives in `tracelean_core`.

pub mod ipc;

// Re-export core for IPC modules to use
pub use tracelean_core as core;
pub use tracelean_core::{
    ai, commands, parser, persistence, requirements, service, state, surgical_edit, trace_graph, undo_tree,
    Command, AppState, SymbolTable, TraceGraph,
    AiSettings, InteractionLog, SessionStats, PendingDiff, McpClientManager, AgentPermissions,
    FileEntry, TraceGraphStats, UndoTreeView, UndoNodeView, CommandLogEntry,
    ToolCallEvent, ToolCallStatus,
    command_summary, command_file, command_affects_file,
    SharedApp, EventSink,
};

use std::sync::{Arc, Mutex};

// ─── Tauri State Wrappers ────────────────────────────────────────────────────
// These wrap the core types for Tauri's managed state system.

pub struct AppStateWrapper(pub Arc<Mutex<AppState>>);
pub struct SymbolTableWrapper(pub Arc<Mutex<SymbolTable>>);
pub struct TraceGraphWrapper(pub Arc<Mutex<TraceGraph>>);

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

// ─── AI Tauri State Wrappers ─────────────────────────────────────────────────

pub struct AiSettingsWrapper(pub Arc<Mutex<AiSettings>>);
pub struct AiLogWrapper(pub Arc<Mutex<InteractionLog>>);
pub struct AiSessionStatsWrapper(pub Arc<Mutex<SessionStats>>);
pub struct MockPendingWrapper(pub Mutex<Vec<ai::mock::MockPendingRequest>>);
pub struct MockProviderWrapper(pub Arc<tokio::sync::Mutex<Option<Arc<ai::mock::MockProvider>>>>);
pub struct PendingDiffsWrapper(pub Mutex<Vec<PendingDiff>>);

/// Channel for tool loop pause/resume. Frontend sends true=continue, false=abort.
pub struct ToolLoopResumeWrapper(pub Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<bool>>>>);

/// Persistent chat session store — maps session_id → ChatSession.
/// Sessions hold both user_view (raw) and model_view (compacted) across calls.
pub struct ChatSessionStoreWrapper(pub Arc<tokio::sync::Mutex<std::collections::HashMap<String, tracelean_core::ChatSession>>>);

// ─── MCP & Permissions State ─────────────────────────────────────────────────

pub struct McpHostPermissionsWrapper(pub Mutex<AgentPermissions>);
pub struct McpClientWrapper(pub tokio::sync::Mutex<McpClientManager>);
pub struct AgentPermissionsStore(pub Mutex<std::collections::HashMap<String, AgentPermissions>>);

// ─── ACP State ───────────────────────────────────────────────────────────────

pub struct AcpManagerWrapper(pub std::sync::Arc<tracelean_core::acp::AcpClientManager>);

// ─── Tauri EventSink Implementation ─────────────────────────────────────────

pub struct TauriEventSink {
    pub app_handle: tauri::AppHandle,
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: &str, payload: &str) {
        use tauri::Emitter;
        // Emit JSON payloads as structured objects, not double-encoded strings —
        // frontend listeners destructure `event.payload` directly.
        match serde_json::from_str::<serde_json::Value>(payload) {
            Ok(value) => {
                let _ = self.app_handle.emit(event, value);
            }
            Err(_) => {
                let _ = self.app_handle.emit(event, payload);
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn invalidate_undo_cache(cache: &tauri::State<'_, UndoTreeCacheWrapper>) {
    if let Ok(mut c) = cache.0.lock() {
        c.view = None;
    }
}

/// Helper: get a boxed AI provider based on current settings.
pub async fn get_provider(
    settings: &tauri::State<'_, AiSettingsWrapper>,
    mock_provider: &tauri::State<'_, MockProviderWrapper>,
) -> Result<Box<dyn ai::provider::AiProvider + Send + Sync>, String> {
    let (provider_kind, or_key, br_token, br_region) = {
        let s = settings.0.lock().map_err(|e| e.to_string())?;
        (
            s.active_provider.clone(),
            s.openrouter_api_key.clone(),
            s.bedrock_api_key.clone(),
            s.bedrock_region.clone(),
        )
    };

    match provider_kind {
        ai::ProviderKind::OpenRouter => {
            let key = or_key
                .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
                .ok_or("OpenRouter API key not set (set in settings or OPENROUTER_API_KEY env var)")?;
            Ok(Box::new(ai::openrouter::OpenRouterProvider::new(key)))
        }
        ai::ProviderKind::Bedrock => {
            let token = br_token
                .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok())
                .ok_or("Bedrock bearer token not set (set in settings or AWS_BEARER_TOKEN_BEDROCK env var)")?;
            Ok(Box::new(ai::bedrock::BedrockProvider::new(token, br_region)))
        }
        ai::ProviderKind::Mock => {
            let mut guard = mock_provider.0.lock().await;
            if guard.is_none() {
                let (p, _rx) = ai::mock::MockProvider::new();
                *guard = Some(Arc::new(p));
            }
            drop(guard);
            Ok(Box::new(SharedMockProvider { inner: mock_provider.0.clone() }))
        }
    }
}

/// Thin wrapper around the shared MockProvider Arc.
struct SharedMockProvider {
    inner: Arc<tokio::sync::Mutex<Option<Arc<ai::mock::MockProvider>>>>,
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
            input_cost_per_m: 15.0,
            output_cost_per_m: 75.0,
            cached_input_cost_per_m: 1.875,
            extra_params: None,
                coding_index: None,
                coding_rank: None,
                supports_caching: false,
                supports_tools: false,
            ..Default::default()
        }])
    }
}

// ─── App Entry Point ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppStateWrapper(Arc::new(Mutex::new(AppState::new()))))
        .manage(SymbolTableWrapper(Arc::new(Mutex::new(SymbolTable::new()))))
        .manage(TraceGraphWrapper(Arc::new(Mutex::new(TraceGraph::new()))))
        .manage(UndoTreeCacheWrapper(Mutex::new(UndoTreeCache::new())))
        .manage(AiSettingsWrapper(Arc::new(Mutex::new(AiSettings::default()))))
        .manage(AiLogWrapper(Arc::new(Mutex::new(InteractionLog::new()))))
        .manage(AiSessionStatsWrapper(Arc::new(Mutex::new(SessionStats::default()))))
        .manage(MockPendingWrapper(Mutex::new(Vec::new())))
        .manage(MockProviderWrapper(Arc::new(tokio::sync::Mutex::new(None))))
        .manage(PendingDiffsWrapper(Mutex::new(Vec::new())))
        .manage(ToolLoopResumeWrapper(Arc::new(tokio::sync::Mutex::new(None))))
        .manage(ChatSessionStoreWrapper(Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()))))
        .manage(McpHostPermissionsWrapper(Mutex::new(AgentPermissions::full_access("mcp-host"))))
        .manage(McpClientWrapper(tokio::sync::Mutex::new(McpClientManager::new())))
        .manage(AgentPermissionsStore(Mutex::new(std::collections::HashMap::new())))
        .manage(AcpManagerWrapper(std::sync::Arc::new(
            tracelean_core::acp::AcpClientManager::new(Arc::new(tracelean_core::SharedApp::new_headless()))
        )))
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
            ipc::ai_commands::detect_env_keys,
            ipc::ai_commands::update_ai_settings,
            ipc::ai_commands::get_ai_models,
            ipc::ai_commands::get_ai_session_stats,
            ipc::ai_commands::get_ai_interaction_log,
            ipc::ai_commands::get_ai_interaction_detail,
            ipc::ai_commands::get_mock_pending,
            ipc::ai_commands::mock_submit_response,
            ipc::ai_commands::ai_chat,
            ipc::ai_commands::ai_chat_session,
            ipc::ai_commands::reset_chat_session,
            ipc::ai_commands::get_chat_session_messages,
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
            ipc::ai_commands::resume_tool_loop,
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
            // ACP
            ipc::acp_commands::acp_connect_agent,
            ipc::acp_commands::acp_disconnect_agent,
            ipc::acp_commands::acp_list_agents,
            ipc::acp_commands::acp_status,
            ipc::acp_commands::acp_prompt,
            ipc::acp_commands::acp_cancel,
            ipc::acp_commands::acp_permission_respond,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
