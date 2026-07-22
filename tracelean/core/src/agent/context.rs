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
    /// Hard-stop token (T12): checked every loop iteration, raced against the
    /// in-flight LLM request, and polled during shell execution. Callers
    /// starting a new run must `reset()` it first.
    pub cancel: super::CancelToken,
    /// Print prompts, responses, and tool calls to stderr.
    pub verbose: bool,
    /// Cost-aware retention engine for context compaction decisions.
    pub retention_engine: Arc<Mutex<RetentionEngine>>,
    /// Turn timing tracker (idle time → cache cold detection).
    pub timing_tracker: Arc<Mutex<TurnTimingTracker>>,
    /// P10 review mode: staged agent edits awaiting per-hunk user approval.
    pub pending_diffs: Arc<Mutex<Vec<crate::PendingDiff>>>,
}

/// User's answer to a per-command shell approval prompt (bugs.md Feature 4 +
/// sandboxing_better.md T4: the prompt carries a network opt-in checkbox).
#[derive(Debug, Clone, Copy)]
pub struct CommandApproval {
    pub approved: bool,
    /// Grant network access to this one command (sandbox policy "ask").
    pub allow_network: bool,
}

/// Abstraction for tool-loop pause behavior.
/// GUI injects one that emits events + waits for user; headless returns true always.
#[async_trait::async_trait]
pub trait PauseHandler: Send + Sync {
    /// Called every N tool calls. Return true to continue, false to abort.
    async fn should_continue(&self, tool_calls_so_far: usize) -> bool;

    /// bugs.md Feature 4: ask the user to approve one shell command before it
    /// runs. Default allows without a network grant — headless runners and
    /// tests are unaffected.
    async fn approve_command(&self, command: &str) -> CommandApproval {
        let _ = command;
        CommandApproval { approved: true, allow_network: false }
    }

    /// bugs.md Bug 0 / Bug 2: block the tool loop after a tool call stages
    /// new pending diffs (an `edit_file`/`run_shell` in review mode), until
    /// the user has accepted or rejected all of them. Without this, the
    /// agent keeps calling tools — and can read/edit files — against a
    /// project state the staged hunks haven't actually reached yet.
    /// `diff_ids` are the ids staged by this tool call; returns the real
    /// per-diff outcome (accepted/rejected/partial) so the caller can report
    /// what actually happened instead of a placeholder "staged" message, or
    /// `None` to abort the run (user hit Stop while reviewing).
    async fn wait_for_review(&self, diff_ids: &[String]) -> Option<Vec<crate::ResolvedDiffOutcome>> {
        let _ = diff_ids;
        Some(Vec::new())
    }
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
                if let Ok(s) = app.ai_settings.lock() {
                    p.review_edits = s.review_edits;
                    p.review_commands = s.review_commands;
                    p.shell_sandbox = s.shell_sandbox.clone();
                    p.shell_network = s.shell_network.clone();
                }
                p
            },
            project_root,
            event_sink: Arc::clone(&app.event_sink),
            spend_cap_usd: spend_cap,
            pause_handler: None,
            cancel: app.agent_cancel.clone(),
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
            cancel: super::CancelToken::new(),
            verbose: false,
            retention_engine: Arc::new(Mutex::new(RetentionEngine::with_defaults())),
            timing_tracker: Arc::new(Mutex::new(TurnTimingTracker::new())),
            pending_diffs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}
