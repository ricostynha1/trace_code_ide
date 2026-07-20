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
use crate::ai::provider::{AiProvider, AiResponse, ChatMessage, MessageRole, ModelConfig, ProviderKind};
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
    /// Hard-stop token (T12) shared with the stop_agent_run entry point.
    pub cancel: crate::agent::CancelToken,
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
                if let Ok(s) = self.settings.lock() {
                    p.review_edits = s.review_edits;
                    p.review_commands = s.review_commands;
                }
                p
            },
            project_root,
            event_sink: self.event_sink.clone(),
            spend_cap_usd: spend_cap,
            pause_handler,
            cancel: self.cancel.clone(),
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
        // A fresh run must not inherit a stale Stop from the previous one.
        self.cancel.reset();
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

    /// bugs.md Feature 2: user-triggered summarization of a session's model
    /// view (the "summarize now" button next to context reset).
    pub async fn summarize_session(&self, session_id: &str) -> Result<(), String> {
        let ctx = self.agent_context(None, false).await?;
        let mut store = self.sessions.lock().await;
        let session = store
            .get_mut(session_id)
            .ok_or_else(|| format!("no session {}", session_id))?;
        crate::agent::force_summarize(&ctx, &mut session.model_view).await?;
        session.compacted = true;
        session.compaction_count += 1;
        // bugs.md Bug 1.4: refresh the context-size estimate NOW so the chat
        // panel's context bar updates immediately instead of on the next turn.
        session.last_prompt_tokens = session
            .model_view
            .iter()
            .map(crate::agent::runtime::estimate_msg_tokens)
            .sum::<usize>() as u32;
        session.last_completion_tokens = 0;
        session.updated_at = chrono::Utc::now().to_rfc3339();
        if let Some(root) = self.project_root() {
            save_session(&root, session);
        }
        Ok(())
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
        self.cancel.reset();
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

/// Rebuild a model's full `ModelConfig` from just its identity
/// (provider + model_id) — no network call. Bedrock and Mock have local
/// catalog/pricing data (`bedrock_pricing`, `provider_cache`, `model_catalog`)
/// and resolve to accurate, up-to-date figures; OpenRouter has no local
/// pricing source (its own `/models` response IS the source of truth), so it
/// resolves to an identity-only placeholder until `list_models_for` next runs.
///
/// This is what makes it safe to persist only `{provider, model_id}` in
/// settings (see `persisted_model_ref` below) instead of a frozen pricing
/// snapshot that goes stale the moment the catalog data improves.
pub fn resolve_model_config(provider: &ProviderKind, model_id: &str) -> ModelConfig {
    match provider {
        ProviderKind::Mock => crate::ai::mock::model_for_id(model_id),
        ProviderKind::Bedrock => crate::ai::bedrock::model_for_id(model_id),
        ProviderKind::OpenRouter => crate::ai::openrouter::model_for_id(model_id),
    }
}

/// bugs.md: the settings *file* must only ever contain a model's identity
/// (provider + model_id), never its pricing/catalog snapshot — otherwise a
/// selection made before a pricing fix keeps showing the stale number
/// forever (MiniMax's cached price stayed wrong in settings after the
/// discount table was corrected, because the whole `ModelConfig` — including
/// the old price — round-tripped to/from disk verbatim). This is deliberately
/// a JSON-`Value` transform applied only at the file I/O boundary in
/// `write_settings`/`load_settings`/`load_settings_from` below, not a change
/// to `AiSettings`'s own (de)serialization — IPC callers (the frontend) still
/// get/send the full `ModelConfig` they need to display pricing.
mod persisted_model_fields {
    const KEYS: [&str; 2] = ["selected_model", "summary_model"];

    /// Before writing to disk: collapse each model field down to identity.
    pub fn strip_pricing(settings_json: &mut serde_json::Value) {
        let Some(obj) = settings_json.as_object_mut() else { return };
        for key in KEYS {
            let Some(model) = obj.get(key).and_then(|v| v.as_object()) else { continue };
            let mut identity = serde_json::Map::new();
            if let Some(p) = model.get("provider") {
                identity.insert("provider".to_string(), p.clone());
            }
            if let Some(id) = model.get("model_id") {
                identity.insert("model_id".to_string(), id.clone());
            }
            obj.insert(key.to_string(), serde_json::Value::Object(identity));
        }
    }

    /// After reading from disk: rebuild each model field's full pricing from
    /// its identity, via `resolve_model_config`, before deserializing into
    /// `AiSettings` (which expects the full `ModelConfig` shape).
    pub fn resolve_pricing(settings_json: &mut serde_json::Value) {
        let Some(obj) = settings_json.as_object_mut() else { return };
        for key in KEYS {
            let Some(model) = obj.get(key) else { continue };
            let (provider, model_id) = match (model.get("provider"), model.get("model_id").and_then(|v| v.as_str())) {
                (Some(p), Some(id)) => (p.clone(), id.to_string()),
                _ => continue,
            };
            let Ok(provider) = serde_json::from_value::<super::ProviderKind>(provider) else { continue };
            let resolved = super::resolve_model_config(&provider, &model_id);
            if let Ok(value) = serde_json::to_value(resolved) {
                obj.insert(key.to_string(), value);
            }
        }
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

// ─── Settings persistence ────────────────────────────────────────────────────
// All TraceLean artifacts live in {project}/.tracelean once a project is open;
// ~/.tracelean/ai_settings.json is only the pre-project fallback (settings
// load at startup, before any project exists).

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".tracelean").join("ai_settings.json"))
}

/// Project-scoped settings file: {project}/.tracelean/ai_settings.json
pub fn settings_path_in(project_root: &Path) -> PathBuf {
    project_root.join(".tracelean").join("ai_settings.json")
}

/// Parse a settings JSON string, resolving `selected_model`/`summary_model`
/// pricing fresh from their persisted identity (bugs.md — see
/// `persisted_model_fields`) before deserializing into `AiSettings`.
fn parse_settings(content: &str) -> Option<AiSettings> {
    let mut value: serde_json::Value = serde_json::from_str(content).ok()?;
    persisted_model_fields::resolve_pricing(&mut value);
    serde_json::from_value(value).ok()
}

/// Load persisted settings, falling back to defaults (home fallback — used
/// before a project is open).
pub fn load_settings() -> AiSettings {
    settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|content| parse_settings(&content))
        .unwrap_or_default()
}

/// Load settings from a project's .tracelean dir, if present.
pub fn load_settings_from(project_root: &Path) -> Option<AiSettings> {
    let content = std::fs::read_to_string(settings_path_in(project_root)).ok()?;
    parse_settings(&content)
}

fn write_settings(path: &Path, settings: &AiSettings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    persisted_model_fields::strip_pricing(&mut value);
    let json = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Persist settings to the home fallback.
pub fn save_settings(settings: &AiSettings) -> Result<(), String> {
    let Some(path) = settings_path() else {
        return Err("cannot resolve home directory".into());
    };
    write_settings(&path, settings)
}

/// Persist settings into the project's .tracelean dir (and the home fallback,
/// so a fresh app launch before opening a project keeps the same config).
pub fn save_settings_to(project_root: &Path, settings: &AiSettings) -> Result<(), String> {
    write_settings(&settings_path_in(project_root), settings)?;
    let _ = save_settings(settings);
    Ok(())
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

#[cfg(test)]
mod persisted_model_tests {
    use super::*;

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tracelean-settings-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn saved_settings_file_never_carries_pricing_fields() {
        let project = temp_project();
        let mut settings = AiSettings::default();
        settings.selected_model = Some(crate::ai::bedrock::model_for_id("minimax.minimax-m2.5"));
        // write_settings directly (not save_settings_to): the latter also
        // writes the real ~/.tracelean/ai_settings.json home fallback, which
        // a test must never touch.
        write_settings(&settings_path_in(&project), &settings).unwrap();

        let on_disk = std::fs::read_to_string(settings_path_in(&project)).unwrap();
        assert!(!on_disk.contains("cached_input_cost_per_m"), "pricing must never hit disk: {on_disk}");
        assert!(!on_disk.contains("input_cost_per_m"), "pricing must never hit disk: {on_disk}");
        assert!(on_disk.contains("minimax.minimax-m2.5"), "identity must still be persisted: {on_disk}");
    }

    #[test]
    fn loading_settings_resolves_pricing_fresh_even_if_disk_has_a_stale_value() {
        // Regression: a hand-edited (or pre-fix) settings file with a stale
        // cached_input_cost_per_m must not survive a load — the field is
        // ignored entirely; only provider+model_id are read back and
        // re-resolved from the current catalog/pricing tables.
        let project = temp_project();
        std::fs::create_dir_all(settings_path_in(&project).parent().unwrap()).unwrap();
        std::fs::write(
            settings_path_in(&project),
            r#"{
                "active_provider": "bedrock",
                "openrouter_api_key": null,
                "bedrock_api_key": null,
                "bedrock_region": null,
                "selected_model": {
                    "provider": "bedrock",
                    "model_id": "minimax.minimax-m2.5",
                    "cached_input_cost_per_m": 0.001
                }
            }"#,
        )
        .unwrap();

        let settings = load_settings_from(&project).expect("parses");
        let model = settings.selected_model.expect("model resolved");
        assert_eq!(model.cached_input_cost_per_m, 0.06, "must be freshly resolved, not the stale 0.001 on disk");
        assert_eq!(model.input_cost_per_m, 0.30);
    }

    #[test]
    fn round_trip_through_save_and_load_resolves_current_pricing() {
        let project = temp_project();
        let mut settings = AiSettings::default();
        settings.selected_model = Some(crate::ai::bedrock::model_for_id("minimax.minimax-m2.5"));
        write_settings(&settings_path_in(&project), &settings).unwrap();

        let loaded = load_settings_from(&project).expect("parses");
        let model = loaded.selected_model.expect("model resolved");
        assert_eq!(model.model_id, "minimax.minimax-m2.5");
        assert_eq!(model.cached_input_cost_per_m, 0.06);
    }
}
