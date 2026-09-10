//! Sandboxed-session IPC: create and observe a long-lived, reflink-copied
//! workspace the user can run external tools (Claude Code, most notably)
//! inside themselves. tracelean never launches the external tool — these
//! commands only manage the workspace, its watcher, and its transcript
//! tail; entering the sandbox is `tracelean-sandbox shell ...`, run by the
//! user in their own terminal (see `core/src/bin/tracelean-sandbox.rs`).
//! See the design doc for the full rationale (reflink copy vs overlay,
//! external-terminal-first, mirror-live, Tier-1-only transcript
//! observability).

use crate::{AiLogWrapper, AiSessionStatsWrapper, AppStateWrapper, TauriEventSink};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, State};
use tracelean_core::ai::log::InteractionLog;
use tracelean_core::ai::tracking::SessionStats;
use tracelean_core::sandbox::cost;
use tracelean_core::sandbox::mirror::{SandboxLink, SelfWrites};
use tracelean_core::sandbox::session::{self, SessionSpec};
use tracelean_core::sandbox::transcript::{transcript_path_excluding, TranscriptEvent, TranscriptTail};
use tracelean_core::sandbox::{collect_tree_mutations, SandboxWatcher};
use tracelean_core::EventSink;

/// Transcript file paths already claimed by some session's tail, process-
/// wide. Sessions for the same project alias to the same Claude Code slug
/// directory (see `transcript::transcript_path`'s doc comment), so without
/// this a second session could lock onto the first session's still-live
/// file. Released on `sandbox_destroy_session` so a `claude --resume` of
/// the same transcript after that can be picked up again.
fn claimed_transcripts() -> &'static Mutex<HashSet<PathBuf>> {
    static CLAIMED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    CLAIMED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Agent tag external-agent Log entries carry — mirrors how the built-in
/// agent tags its own entries ("chat", "elicitation", ...), so the Log tab
/// can distinguish them.
const EXTERNAL_AGENT_TAG: &str = "claude-code (sandbox)";

/// One session this process is actively watching: the spec, the live
/// filesystem watcher (dropping it stops the watcher thread), the stop
/// flag for its transcript-tail thread (started lazily once a transcript
/// file appears — see `spawn_transcript_tail`), and a capped backfill
/// buffer of everything the tail has parsed so far — so a frontend panel
/// opened after the session started can catch up instead of only seeing
/// events emitted from the moment it subscribed.
pub(crate) struct ActiveSession {
    spec: SessionSpec,
    _watcher: SandboxWatcher,
    transcript_stop: Arc<AtomicBool>,
    transcript_history: Arc<Mutex<Vec<serde_json::Value>>>,
    /// The transcript file this session's tail has claimed (`None` until a
    /// `claude` invocation actually starts writing one) — released from
    /// the process-wide `claimed_transcripts()` set on destroy.
    claimed_transcript: Arc<Mutex<Option<PathBuf>>>,
}

/// Backfill buffer cap — generous enough for a long session's transcript
/// tab, small enough not to matter for memory.
const TRANSCRIPT_HISTORY_CAP: usize = 4000;

#[derive(Default)]
pub struct SandboxSessionWrapper(Mutex<HashMap<String, ActiveSession>>);

fn project_root(state: &State<'_, AppStateWrapper>) -> Result<PathBuf, String> {
    let s = state.0.lock().map_err(|e| e.to_string())?;
    s.project_root().cloned().ok_or_else(|| "no project open".to_string())
}

#[tauri::command]
pub fn sandbox_capabilities(state: State<'_, AppStateWrapper>) -> Result<session::SandboxCapabilities, String> {
    Ok(session::capabilities(&project_root(&state)?))
}

/// Create a session (reflink-copy the project), start watching it, and
/// attach it as `AppState`'s active sandbox link so `service::save_file`
/// starts mirroring IDE saves into it. Also starts the Phase-8 transcript
/// tail once Claude Code's own JSONL file for this project appears.
///
/// SAFETY: destroys any *other* active session for this same project
/// first. Two sessions for one project share the same real tree as their
/// mirror target (`spec.project_root`) but each has its own, independently
/// diverging `work_dir` — two live watchers reconciling the same real tree
/// against two different "truths" fight each other (each treats its own
/// `work_dir` as authoritative and "corrects" the real tree back to it),
/// which was observed corrupting file content. Only one watched session
/// per project may be active at a time; the CLI (`tracelean-sandbox
/// shell`) can still be re-run any number of times against that one
/// session, which is what makes multiple *terminals* into it safe.
#[tauri::command]
pub fn sandbox_create_session(
    app: AppHandle,
    state: State<'_, AppStateWrapper>,
    sandbox: State<'_, SandboxSessionWrapper>,
    ai_log: State<'_, AiLogWrapper>,
    ai_stats: State<'_, AiSessionStatsWrapper>,
    allow_network: bool,
) -> Result<SessionSpec, String> {
    let root = project_root(&state)?;

    let stale_ids: Vec<String> = sandbox
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|(_, active)| active.spec.project_root == root)
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale_ids {
        let _ = destroy_one(&state, &sandbox, &root, &id);
    }

    let spec = session::create_session(&root, allow_network)?;

    let event_sink: Arc<dyn EventSink> = Arc::new(TauriEventSink { app_handle: app });
    let self_writes = SelfWrites::new();

    let watcher = SandboxWatcher::spawn(spec.clone(), Arc::clone(&state.0), self_writes.clone(), Arc::clone(&event_sink))?;

    {
        let mut s = state.0.lock().map_err(|e| e.to_string())?;
        s.set_sandbox_link(Some(SandboxLink { work_dir: spec.work_dir.clone(), self_writes }));
    }

    let transcript_history = Arc::new(Mutex::new(Vec::new()));
    let claimed_transcript = Arc::new(Mutex::new(None));
    let transcript_stop = spawn_transcript_tail(
        spec.clone(),
        Arc::clone(&event_sink),
        Arc::clone(&transcript_history),
        Arc::clone(&ai_log.0),
        Arc::clone(&ai_stats.0),
        Arc::clone(&claimed_transcript),
    );

    sandbox.0.lock().map_err(|e| e.to_string())?.insert(
        spec.id.clone(),
        ActiveSession { spec: spec.clone(), _watcher: watcher, transcript_stop, transcript_history, claimed_transcript },
    );

    Ok(spec)
}

/// Everything the transcript tail has parsed for this session so far — for
/// a frontend panel opened after the session started to catch up before it
/// subscribes to live `sandbox-transcript` events.
#[tauri::command]
pub fn sandbox_transcript_history(
    sandbox: State<'_, SandboxSessionWrapper>,
    session_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let sessions = sandbox.0.lock().map_err(|e| e.to_string())?;
    let active = sessions.get(&session_id).ok_or_else(|| format!("session '{}' is not active", session_id))?;
    let history = active.transcript_history.lock().map_err(|e| e.to_string())?.clone();
    Ok(history)
}

/// Stop watching (if this process is watching it) and delete a session's
/// work dir. Works for sessions from a *previous* run of the app too —
/// e.g. after a dev-mode rebuild restarted the whole app, `SandboxPanel`
/// still lists sessions left on disk from before the restart, and this
/// must be able to clean those up even though there's no in-memory
/// `ActiveSession`/watcher for them to stop.
#[tauri::command]
pub fn sandbox_destroy_session(
    state: State<'_, AppStateWrapper>,
    sandbox: State<'_, SandboxSessionWrapper>,
    session_id: String,
) -> Result<(), String> {
    let root = project_root(&state)?;
    destroy_one(&state, &sandbox, &root, &session_id)
}

fn destroy_one(
    state: &State<'_, AppStateWrapper>,
    sandbox: &State<'_, SandboxSessionWrapper>,
    project_root: &std::path::Path,
    session_id: &str,
) -> Result<(), String> {
    let active = sandbox.0.lock().map_err(|e| e.to_string())?.remove(session_id);

    let spec = match active {
        Some(active) => {
            active.transcript_stop.store(true, Ordering::SeqCst);
            // active._watcher drops here, stopping the filesystem watcher thread.
            if let Some(path) = active.claimed_transcript.lock().map_err(|e| e.to_string())?.take() {
                claimed_transcripts().lock().map_err(|e| e.to_string())?.remove(&path);
            }
            {
                let mut s = state.0.lock().map_err(|e| e.to_string())?;
                if s.sandbox_link().map(|l| l.work_dir == active.spec.work_dir).unwrap_or(false) {
                    s.set_sandbox_link(None);
                }
            }
            active.spec
        }
        // Not active in this process (e.g. left over from before a
        // dev-mode restart) — nothing to stop, just clean up the on-disk
        // session directory.
        None => session::load_session(project_root, session_id)?,
    };

    session::destroy_session(&spec)
}

/// Destroy every session (active or left over from a previous run)
/// persisted for the open project — a bulk "clean up my sandboxes" for
/// when several have piled up. Best-effort: keeps going on a per-session
/// failure and reports which ones it couldn't remove, rather than
/// aborting the whole cleanup on the first error.
#[tauri::command]
pub fn sandbox_destroy_all_sessions(
    state: State<'_, AppStateWrapper>,
    sandbox: State<'_, SandboxSessionWrapper>,
) -> Result<Vec<String>, String> {
    let root = project_root(&state)?;
    let ids: Vec<String> = session::list_sessions(&root).into_iter().map(|s| s.id).collect();
    let mut failures = Vec::new();
    for id in ids {
        if let Err(e) = destroy_one(&state, &sandbox, &root, &id) {
            failures.push(format!("{}: {}", id, e));
        }
    }
    Ok(failures)
}

/// All sessions persisted for the open project, newest first — including
/// ones from a previous run of the app that are not currently being
/// watched (re-attaching a watcher to those is not yet implemented; they
/// show up for visibility/cleanup, not live mirroring).
#[tauri::command]
pub fn sandbox_list_sessions(state: State<'_, AppStateWrapper>) -> Result<Vec<SessionSpec>, String> {
    Ok(session::list_sessions(&project_root(&state)?))
}

/// The `tracelean-sandbox shell ...` invocation the user runs in their own
/// terminal to enter this session.
#[tauri::command]
pub fn sandbox_shell_command(state: State<'_, AppStateWrapper>, session_id: String) -> Result<String, String> {
    let root = project_root(&state)?;
    let bin = sandbox_binary_path();
    Ok(format!("{} shell {} {}", bin, shell_quote(&root.to_string_lossy()), shell_quote(&session_id)))
}

/// Best-effort: launch the user's terminal emulator directly into the
/// session shell, for people who don't want to copy/paste
/// `sandbox_shell_command`'s output.
#[tauri::command]
pub fn sandbox_open_terminal(state: State<'_, AppStateWrapper>, session_id: String) -> Result<(), String> {
    let root = project_root(&state)?;
    let bin = sandbox_binary_path();
    let args = vec!["shell".to_string(), root.to_string_lossy().to_string(), session_id];

    if let Ok(term) = std::env::var("TERMINAL") {
        if try_spawn_terminal(&term, &bin, &args) {
            return Ok(());
        }
    }
    for term in ["gnome-terminal", "konsole", "kitty", "alacritty", "xterm"] {
        if try_spawn_terminal(term, &bin, &args) {
            return Ok(());
        }
    }
    Err("no terminal emulator found — set $TERMINAL, or run the sandbox_shell_command output manually".into())
}

fn try_spawn_terminal(term: &str, bin: &str, args: &[String]) -> bool {
    // `-e <cmd> <args...>` is understood by every emulator in the fallback
    // list above; a custom $TERMINAL that needs different flags should be
    // set to a wrapper script instead.
    let mut cmd = std::process::Command::new(term);
    cmd.arg("-e").arg(bin).args(args);
    cmd.spawn().is_ok()
}

fn sandbox_binary_path() -> String {
    // Prefer the binary sitting next to this process (how the packaged app
    // ships it); fall back to PATH for `cargo run`/dev setups.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("tracelean-sandbox");
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    "tracelean-sandbox".to_string()
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[derive(Debug, Clone, Serialize)]
pub struct SandboxChangeEntry {
    pub path: String,
    pub kind: String,
}

/// Current A/M/D list for a session, computed on demand (mutations aren't
/// persisted between watcher ticks — this is the same diff the watcher
/// itself runs).
#[tauri::command]
pub fn sandbox_changes(
    state: State<'_, AppStateWrapper>,
    sandbox: State<'_, SandboxSessionWrapper>,
    session_id: String,
) -> Result<Vec<SandboxChangeEntry>, String> {
    let _ = &state; // project root comes from the session spec itself
    let sessions = sandbox.0.lock().map_err(|e| e.to_string())?;
    let active = sessions.get(&session_id).ok_or_else(|| format!("session '{}' is not active", session_id))?;
    Ok(collect_tree_mutations(&active.spec.work_dir, &active.spec.project_root)
        .into_iter()
        .map(|m| SandboxChangeEntry {
            path: m.path.to_string_lossy().to_string(),
            kind: match m.kind {
                tracelean_core::ai::shell_sandbox::MutationKind::Created => "created",
                tracelean_core::ai::shell_sandbox::MutationKind::Modified => "modified",
                tracelean_core::ai::shell_sandbox::MutationKind::Deleted => "deleted",
            }
            .to_string(),
        })
        .collect())
}

/// Restore one file in the session's work dir to match the real tree
/// (discarding whatever the sandboxed tool did to it there).
#[tauri::command]
pub fn sandbox_revert_file(
    sandbox: State<'_, SandboxSessionWrapper>,
    session_id: String,
    path: String,
) -> Result<(), String> {
    let sessions = sandbox.0.lock().map_err(|e| e.to_string())?;
    let active = sessions.get(&session_id).ok_or_else(|| format!("session '{}' is not active", session_id))?;
    let rel = PathBuf::from(&path);
    let real = active.spec.project_root.join(&rel);
    let work = active.spec.work_dir.join(&rel);
    if real.exists() {
        if let Some(parent) = work.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::copy(&real, &work).map_err(|e| e.to_string())?;
    } else {
        let _ = std::fs::remove_file(&work);
    }
    Ok(())
}

/// Accumulates one assistant turn's pieces (text, thinking, tool calls)
/// between `Usage` events — Claude Code's transcript reports these as
/// separate content-block records, but usage/cost is reported once per
/// turn, so a `Usage` event is the natural "flush" point for one
/// `InteractionEntry` (mirroring one entry per provider call, the way the
/// built-in agent's own `record_success` works).
#[derive(Default)]
struct TurnBuffer {
    text: Vec<String>,
    thinking: Vec<String>,
    tool_names: Vec<String>,
}

impl TurnBuffer {
    fn take(&mut self) -> (Option<String>, Option<String>, Vec<String>) {
        let text = if self.text.is_empty() { None } else { Some(self.text.join("\n")) };
        let thinking = if self.thinking.is_empty() { None } else { Some(self.thinking.join("\n")) };
        (text, thinking, std::mem::take(&mut self.tool_names))
    }
}

/// Poll `transcript_path` for this session's Claude Code JSONL file (it
/// may not exist yet — a shell can sit open with no `claude` invocation
/// started) and, once found, tail it: emit `sandbox-transcript` for every
/// new line (live view in the AI chat panel), and on each `Usage` event,
/// flush the turn accumulated since the last one into `ai_log` +
/// `ai_stats` as an estimated-cost interaction (Phase 8 — Tier 1
/// observability only, no capture; cost is always an estimate, see
/// `sandbox::cost`).
fn spawn_transcript_tail(
    spec: SessionSpec,
    event_sink: Arc<dyn EventSink>,
    history: Arc<Mutex<Vec<serde_json::Value>>>,
    ai_log: Arc<Mutex<InteractionLog>>,
    ai_stats: Arc<Mutex<SessionStats>>,
    claimed_transcript: Arc<Mutex<Option<PathBuf>>>,
) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    std::thread::spawn(move || {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
        // Claude Code honors `$CLAUDE_CONFIG_DIR` to relocate its whole
        // state directory off the default `~/.claude` — check that first
        // (it's where writes actually land when set), then fall back.
        let mut claude_homes = Vec::new();
        if let Some(custom) = std::env::var_os("CLAUDE_CONFIG_DIR") {
            claude_homes.push(PathBuf::from(custom));
        }
        claude_homes.push(home.join(".claude"));

        let mut tail: Option<TranscriptTail> = None;
        let mut turn = TurnBuffer::default();
        loop {
            if stop_thread.load(Ordering::SeqCst) {
                return;
            }

            // Re-checked every tick, not just once: a single sandbox
            // session can host several `claude` process lifetimes over
            // time (exit and relaunch, `--resume`, "Welcome back") — each
            // one gets its own new transcript file rather than appending
            // to the previous one, so staying locked onto the first file
            // ever found would silently stop following the conversation
            // after the first `claude` process ends. Pick the newest file
            // not claimed by some *other* session (this session's own
            // current claim is always eligible against itself).
            {
                let mut claimed_guard = claimed_transcripts().lock().unwrap_or_else(|e| e.into_inner());
                let mut my_claim = claimed_transcript.lock().unwrap_or_else(|e| e.into_inner());

                let mut exclude = claimed_guard.clone();
                if let Some(mine) = my_claim.as_ref() {
                    exclude.remove(mine);
                }
                let found = claude_homes.iter().find_map(|h| transcript_path_excluding(h, &spec.project_root, &exclude));

                match found {
                    Some(path) if my_claim.as_ref() != Some(&path) => {
                        if let Some(old) = my_claim.take() {
                            claimed_guard.remove(&old);
                        }
                        claimed_guard.insert(path.clone());
                        *my_claim = Some(path.clone());
                        drop(claimed_guard);
                        drop(my_claim);
                        tail = Some(TranscriptTail::new(path));
                        // Any turn accumulated against the abandoned file
                        // belongs to a `claude` process that's gone now —
                        // don't fold its leftovers into the new one's turns.
                        turn = TurnBuffer::default();
                    }
                    Some(_) => {}
                    None => {
                        drop(claimed_guard);
                        drop(my_claim);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                }
            }
            if let Some(t) = tail.as_mut() {
                for (uuid, event) in t.poll() {
                    match &event {
                        TranscriptEvent::AssistantText { text } => turn.text.push(text.clone()),
                        TranscriptEvent::Thinking { text } => turn.thinking.push(text.clone()),
                        TranscriptEvent::ToolUse { name, .. } => turn.tool_names.push(name.clone()),
                        TranscriptEvent::Usage {
                            model,
                            input_tokens,
                            output_tokens,
                            cache_read_tokens,
                            cache_write_5m_tokens,
                            cache_write_1h_tokens,
                        } => {
                            if let Some((usage, cost_est)) = cost::estimate_cost(
                                model,
                                *input_tokens,
                                *output_tokens,
                                *cache_read_tokens,
                                *cache_write_5m_tokens,
                                *cache_write_1h_tokens,
                            ) {
                                let (text, thinking, tool_names) = turn.take();
                                if let Ok(mut log) = ai_log.lock() {
                                    log.record_external(EXTERNAL_AGENT_TAG, model, text, thinking, tool_names, usage.clone(), cost_est.clone());
                                }
                                if let Ok(mut stats) = ai_stats.lock() {
                                    stats.record_external(&usage, &cost_est);
                                }
                                event_sink.emit("ai-stats-updated", "");
                            }
                            // Model not in the pricing catalog (a brand-new
                            // release not yet in data/models.json): the
                            // turn's text/thinking/tools are simply dropped
                            // from the Log/Stats tabs rather than logged
                            // with an invented cost — the live transcript
                            // view (below) still shows them regardless.
                        }
                        _ => {}
                    }

                    let payload = serde_json::json!({ "session_id": spec.id, "uuid": uuid, "event": event });
                    if let Ok(mut h) = history.lock() {
                        h.push(payload.clone());
                        if h.len() > TRANSCRIPT_HISTORY_CAP {
                            let drop = h.len() - TRANSCRIPT_HISTORY_CAP;
                            h.drain(0..drop);
                        }
                    }
                    event_sink.emit("sandbox-transcript", &payload.to_string());
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    stop
}
