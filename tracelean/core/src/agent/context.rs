//! AgentContext — all dependencies the agent loop needs, no Tauri.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::{
    AiSettings, AppState, EventSink, InteractionLog, NullSink, SharedApp,
    SymbolTable, TraceGraph,
    ai::tool_executor::AgentPermissions,
    ai::tools::ToolDefinition,
    ai::tracking::SessionStats,
    ai::{RetentionEngine, TurnTimingTracker},
};

/// Everything the agent loop needs — no Tauri, no UI framework.
pub struct AgentContext {
    pub state: Arc<Mutex<AppState>>,
    pub symbols: Arc<Mutex<SymbolTable>>,
    pub graph: Arc<Mutex<TraceGraph>>,
    pub settings: Arc<Mutex<AiSettings>>,
    pub stats: Arc<Mutex<SessionStats>>,
    pub log: Arc<Mutex<InteractionLog>>,
    /// Extra tool definitions (e.g. from MCP servers). Merged with builtins at runtime.
    pub extra_tools: Vec<ToolDefinition>,
    pub permissions: AgentPermissions,
    pub project_root: PathBuf,
    pub event_sink: Arc<dyn EventSink>,
    pub spend_cap_usd: f64,
    /// Optional channel for tool-loop pause/resume (UI can inject).
    pub pause_handler: Option<Arc<dyn PauseHandler>>,
    /// Print prompts, responses, and tool calls to stderr.
    pub verbose: bool,
    /// Cost-aware retention engine for context compaction decisions.
    pub retention_engine: Arc<Mutex<RetentionEngine>>,
    /// Turn timing tracker (idle time → cache cold detection).
    pub timing_tracker: Arc<Mutex<TurnTimingTracker>>,
    /// P10 review mode: staged agent edits awaiting per-hunk user approval.
    pub pending_diffs: Arc<Mutex<Vec<crate::PendingDiff>>>,
}

/// Abstraction for tool-loop pause behavior.
/// GUI injects one that emits events + waits for user; headless returns true always.
#[async_trait::async_trait]
pub trait PauseHandler: Send + Sync {
    /// Called every N tool calls. Return true to continue, false to abort.
    async fn should_continue(&self, tool_calls_so_far: usize) -> bool;
}

impl AgentContext {
    /// Build from SharedApp (used by both Tauri and TUI).
    pub fn from_shared_app(app: &SharedApp, project_root: PathBuf) -> Self {
        let spend_cap = app
            .ai_settings
            .lock()
            .map(|s| s.spend_cap_usd)
            .unwrap_or(1.0);

        Self {
            state: Arc::clone(&app.state),
            symbols: Arc::clone(&app.symbols),
            graph: Arc::clone(&app.graph),
            settings: Arc::clone(&app.ai_settings),
            stats: Arc::clone(&app.ai_stats),
            log: Arc::clone(&app.ai_log),
            extra_tools: Vec::new(), // Caller can set MCP tools after construction
            permissions: {
                let mut p = AgentPermissions::full_access("chat");
                p.review_edits = app.ai_settings.lock().map(|s| s.review_edits).unwrap_or(false);
                p
            },
            project_root,
            event_sink: Arc::clone(&app.event_sink),
            spend_cap_usd: spend_cap,
            pause_handler: None,
            verbose: false,
            retention_engine: Arc::new(Mutex::new(RetentionEngine::with_defaults())),
            timing_tracker: Arc::new(Mutex::new(TurnTimingTracker::new())),
            pending_diffs: Arc::clone(&app.pending_diffs),
        }
    }

    /// Build from environment (standalone binary / benchmarks).
    pub fn from_env(project_root: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(AppState::new())),
            symbols: Arc::new(Mutex::new(SymbolTable::new())),
            graph: Arc::new(Mutex::new(TraceGraph::new())),
            settings: Arc::new(Mutex::new(AiSettings::default())),
            stats: Arc::new(Mutex::new(SessionStats::default())),
            log: Arc::new(Mutex::new(InteractionLog::new())),
            extra_tools: Vec::new(),
            permissions: AgentPermissions::full_access("headless"),
            project_root,
            event_sink: Arc::new(NullSink),
            spend_cap_usd: 1.0,
            pause_handler: None,
            verbose: false,
            retention_engine: Arc::new(Mutex::new(RetentionEngine::with_defaults())),
            timing_tracker: Arc::new(Mutex::new(TurnTimingTracker::new())),
            pending_diffs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}
