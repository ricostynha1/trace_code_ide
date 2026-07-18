//! AiService (P8, D8.1) — the application-side AI orchestration, in core.
//!
//! Owns everything `gui_backend/ipc/ai_commands.rs` used to implement inline:
//! provider/agent-turn entry points, the chat-session store, settings
//! load/save, session stats + spend cap, model listing. Frontends (Tauri IPC,
//! TUI) are thin dispatchers over this; the `EventSink` trait remains the only
//! outbound channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::agent::{run_agent_turn, run_agent_turn_session, AgentContext, AgentTurnResult, PauseHandler};
use crate::ai::mcp_client::McpClientManager;
use crate::ai::provider::{AiProvider, AiResponse, ChatMessage, MessageRole, ModelConfig};
use crate::ai::tracking::SessionStats;
use crate::ai::InteractionLog;
use crate::state::AppState;
use crate::{AiSettings, ChatSession, ChatSessionInfo, EventSink, SymbolTable, TraceGraph};

/// Shared, cheaply clonable AI application service.
#[derive(Clone)]
pub struct AiService {
    pub state: Arc<Mutex<AppState>>,
    pub symbols: Arc<Mutex<SymbolTable>>,
    pub graph: Arc<Mutex<TraceGraph>>,
    pub settings: Arc<Mutex<AiSettings>>,
    pub stats: Arc<Mutex<SessionStats>>,
    pub log: Arc<Mutex<InteractionLog>>,
    pub sessions: Arc<tokio::sync::Mutex<HashMap<String, ChatSession>>>,
    pub mcp_client: Arc<tokio::sync::Mutex<McpClientManager>>,
    pub event_sink: Arc<dyn EventSink>,
    /// P10 review mode: staged agent edits awaiting per-hunk user approval.
    pub pending_diffs: Arc<Mutex<Vec<crate::PendingDiff>>>,
}

impl AiService {
    /// Build the AgentContext for one turn from the current service state.
    async fn agent_context(
        &self,
        pause_handler: Option<Arc<dyn PauseHandler>>,
        verbose: bool,
    ) -> Result<AgentContext, String> {
        let project_root = {
            let s = self.state.lock().map_err(|e| e.to_string())?;
            s.project_root().cloned().unwrap_or_default()
        };
        let extra_tools = {
            let mgr = self.mcp_client.lock().await;
            mgr.all_tools().into_iter().map(|(_server, tool)| tool).collect()
        };
        let spend_cap = self.settings.lock().map_err(|e| e.to_string())?.spend_cap_usd;

        Ok(AgentContext {
            state: self.state.clone(),
            symbols: self.symbols.clone(),
            graph: self.graph.clone(),
            settings: self.settings.clone(),
            stats: self.stats.clone(),
            log: self.log.clone(),
            extra_tools,
            permissions: {
                let mut p = crate::AgentPermissions::full_access("chat");
                p.review_edits = self
                    .settings
                    .lock()
                    .map(|s| s.review_edits)
                    .unwrap_or(false);
                p
            },
            project_root,
            event_sink: self.event_sink.clone(),
            spend_cap_usd: spend_cap,
            pause_handler,
            verbose,
            retention_engine: Arc::new(Mutex::new(crate::ai::RetentionEngine::with_defaults())),
            timing_tracker: Arc::new(Mutex::new(crate::ai::TurnTimingTracker::new())),
            pending_diffs: self.pending_diffs.clone(),
        })
    }

    /// One persistent-session chat turn: appends the user message, runs the
    /// agent tool loop, persists the evolved model view.
    pub async fn chat_turn(
        &self,
        session_id: &str,
        user_message: &str,
        pause_handler: Option<Arc<dyn PauseHandler>>,
        verbose: bool,
    ) -> Result<AgentTurnResult, String> {
        let ctx = self.agent_context(pause_handler, verbose).await?;

        let mut store = self.sessions.lock().await;
        let session = store
            .entry(session_id.to_string())
            .or_insert_with(|| ChatSession::new(session_id.to_string()));
        session.append(ChatMessage {
            role: MessageRole::User,
            content: user_message.to_string(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        });
        let cost_before = self.stats.lock().map(|s| s.total_cost_usd).unwrap_or(0.0);
        let result = run_agent_turn_session(&ctx, session)
            .await
            .map_err(|e| e.to_string())?;

        // P11: persist the session every turn (cost delta from session stats).
        let cost_after = self.stats.lock().map(|s| s.total_cost_usd).unwrap_or(cost_before);
        session.total_cost_usd += (cost_after - cost_before).max(0.0);
        session.updated_at = chrono::Utc::now().to_rfc3339();
        if let Some(root) = self.project_root() {
            save_session(&root, session);
        }
        Ok(result)
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.state.lock().ok().and_then(|s| s.project_root().cloned())
    }

    /// List sessions for the switcher (P11): hydrates the in-memory store
    /// from .tracelean/sessions/ so restarts restore prior conversations.
    pub async fn list_sessions(&self) -> Vec<crate::agent::ChatSessionSummary> {
        let mut store = self.sessions.lock().await;
        if let Some(root) = self.project_root() {
            for session in load_sessions(&root) {
                store.entry(session.id.clone()).or_insert(session);
            }
        }
        let mut summaries: Vec<_> = store.values().map(|s| s.summary()).collect();
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        summaries
    }

    /// Stateless agent turn over explicit messages (legacy chat / one-shots).
    pub async fn one_shot_turn(
        &self,
        messages: Vec<ChatMessage>,
        pause_handler: Option<Arc<dyn PauseHandler>>,
        verbose: bool,
    ) -> Result<AiResponse, String> {
        let ctx = self.agent_context(pause_handler, verbose).await?;
        run_agent_turn(&ctx, messages)
            .await
            .map(|r| r.response)
            .map_err(|e| e.to_string())
    }

    /// Reset a session's model context (P7): visible transcript is kept.
    pub async fn reset_session(&self, session_id: &str) {
        let mut store = self.sessions.lock().await;
        if let Some(session) = store.get_mut(session_id) {
            session.reset_context();
        }
    }

    /// Context-utilization snapshot for a session (P7).
    pub async fn session_info(&self, session_id: &str) -> Result<ChatSessionInfo, String> {
        let (context_window, known) = {
            let s = self.settings.lock().map_err(|e| e.to_string())?;
            match &s.selected_model {
                Some(m) => (m.context_window, m.context_window_known),
                None => (128_000, false),
            }
        };
        let store = self.sessions.lock().await;
        Ok(match store.get(session_id) {
            Some(session) => session.info(context_window, known),
            None => ChatSession::new(session_id.to_string()).info(context_window, known),
        })
    }

    /// The visible transcript of a session (user_view).
    pub async fn session_messages(&self, session_id: &str) -> Vec<ChatMessage> {
        let store = self.sessions.lock().await;
        store
            .get(session_id)
            .map(|s| s.user_view.clone())
            .unwrap_or_default()
    }

    /// List models across all configured providers, catalog-enriched and
    /// sorted by coding rank.
    pub async fn list_models(&self) -> Result<Vec<ModelConfig>, String> {
        let snapshot = self.settings.lock().map_err(|e| e.to_string())?.clone();
        Ok(list_models_for(&snapshot).await)
    }

    /// Replace settings and persist them.
    pub fn update_settings(&self, new_settings: AiSettings) -> Result<(), String> {
        {
            let mut s = self.settings.lock().map_err(|e| e.to_string())?;
            *s = new_settings.clone();
        }
        save_settings(&new_settings)
    }
}

/// List models for a settings snapshot (mock + OpenRouter + Bedrock, keys
/// from settings or environment), catalog-enriched, sorted by coding rank.
pub async fn list_models_for(settings: &AiSettings) -> Vec<ModelConfig> {
    let mut models = Vec::new();

    let mock = crate::ai::mock::MockProvider::new().0;
    if let Ok(m) = mock.list_models().await {
        models.extend(m);
    }

    if let Some(key) = settings
        .openrouter_api_key
        .clone()
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
    {
        let or = crate::ai::openrouter::OpenRouterProvider::new(key);
        match or.list_models().await {
            Ok(m) => models.extend(m),
            Err(e) => eprintln!("OpenRouter list_models failed: {}", e.message),
        }
    }

    if let Some(token) = settings
        .bedrock_api_key
        .clone()
        .or_else(|| std::env::var("AWS_BEARER_TOKEN_BEDROCK").ok())
    {
        let br = crate::ai::bedrock::BedrockProvider::new(token, settings.bedrock_region.clone());
        match br.list_models().await {
            Ok(m) => models.extend(m),
            Err(e) => eprintln!("Bedrock list_models failed: {}", e.message),
        }
    }

    for model in &mut models {
        crate::ai::model_catalog::enrich(model);
    }

    models.sort_by(|a, b| match (a.coding_rank, b.coding_rank) {
        (Some(ra), Some(rb)) => ra.cmp(&rb),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.display_name.cmp(&b.display_name),
    });

    models
}

// ─── Settings persistence (~/.tracelean/ai_settings.json) ────────────────────

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".tracelean").join("ai_settings.json"))
}

/// Load persisted settings, falling back to defaults.
pub fn load_settings() -> AiSettings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// Persist settings (best-effort directory creation).
pub fn save_settings(settings: &AiSettings) -> Result<(), String> {
    let Some(path) = settings_path() else {
        return Err("cannot resolve home directory".into());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

// ─── Session persistence (P11) ───────────────────────────────────────────────

/// Directory where chat sessions persist: {project}/.tracelean/sessions/
pub fn sessions_dir(project_root: &Path) -> PathBuf {
    project_root.join(".tracelean").join("sessions")
}

/// Write a session to disk (best-effort; failures only log).
pub fn save_session(project_root: &Path, session: &ChatSession) {
    let dir = sessions_dir(project_root);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[sessions] cannot create {}: {}", dir.display(), e);
        return;
    }
    let path = dir.join(format!("{}.json", session.id));
    match serde_json::to_string_pretty(session) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("[sessions] cannot write {}: {}", path.display(), e);
            }
        }
        Err(e) => eprintln!("[sessions] cannot serialize session {}: {}", session.id, e),
    }
}

/// Load all persisted sessions (unparseable files are skipped).
pub fn load_sessions(project_root: &Path) -> Vec<ChatSession> {
    let dir = sessions_dir(project_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|content| serde_json::from_str::<ChatSession>(&content).ok())
        .collect()
}
