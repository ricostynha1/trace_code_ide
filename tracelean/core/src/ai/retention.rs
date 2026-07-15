//! Context-Retention Engine — dual-view architecture with policy-driven lifecycle.
//!
//! Maintains UserView (append-only, immutable) and ModelView (derived, mutable).
//! Entries carry retention metadata and are managed by the policy table
//! loaded from data/retention_policy.json.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── Entry Types ───────────────────────────────────────────────────────────────

/// Kind of entry in the conversation history.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    UserMsg,
    AssistantMsg,
    ToolCall,
    ToolResult,
    ToolData,
    Summary,
    OverflowPtr,
}

/// What action the retention engine should take on an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionAction {
    Keep,
    StripData,
    Prune,
    CacheResult,
    Offload,
    Summarize,
    /// Eligible for cost-based removal (not yet decided).
    Eligible,
}

/// A resource that an entry depends on.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    File { path: PathBuf, start_line: Option<usize>, end_line: Option<usize> },
    Directory { path: PathBuf },
    Command { cmd: String, cwd: Option<String> },
    DynamicTool { name: String },
    SearchQuery { query: String, scope: Option<String> },
}

/// Events that can invalidate an entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationEvent {
    EditOverlapsRegion { path: PathBuf, start: usize, end: usize },
    NewerReadCoversRegion { path: PathBuf },
    NewerListing { path: PathBuf },
    WriteInDir { path: PathBuf },
    DeleteInDir { path: PathBuf },
    EditInSearchedScope { path: PathBuf },
    NewerEquivalentSearch,
    NewerEditSupersedes,
    FreshReadOrTest,
    RelevantEditChangesInputs,
    NewerCommandSupersedes,
    RetryProduced,
    SchemasInPayload,
    DynamicSetPurged,
    FollowupCallMade,
    UnrelatedAction,
    TtlExpired,
}

/// Metadata for a single entry in the model view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionEntry {
    pub id: u64,
    pub kind: EntryKind,
    pub content: String,
    pub resources: Vec<Resource>,
    pub created_turn: usize,
    pub last_used_turn: usize,
    pub approx_tokens: usize,
    pub ttl: Option<usize>,
    pub invalidation_events: Vec<InvalidationEvent>,
    pub action: RetentionAction,
    /// Tool call arguments hash for dedup.
    pub args_hash: Option<u64>,
    /// Whether this is an ephemeral error (send once, never persist).
    pub ephemeral: bool,
    /// Whether content was offloaded to tmp file.
    pub offloaded: bool,
    /// Path to offloaded content if applicable.
    pub offload_path: Option<PathBuf>,
}

// ─── State Tracking ────────────────────────────────────────────────────────────

/// Tracks file state for invalidation detection.
#[derive(Debug, Clone)]
pub struct FileState {
    pub path: PathBuf,
    pub mtime_ms: u64,
    pub size_bytes: u64,
    pub content_hash: Option<u64>,
}

/// Tracks directory state.
#[derive(Debug, Clone)]
pub struct DirState {
    pub path: PathBuf,
    pub listing_hash: u64,
}

/// Resource state tracker for invalidation.
#[derive(Debug, Clone, Default)]
pub struct StateTracker {
    pub files: HashMap<PathBuf, FileState>,
    pub dirs: HashMap<PathBuf, DirState>,
}

impl StateTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record file state at current point.
    pub fn record_file(&mut self, path: PathBuf, mtime_ms: u64, size_bytes: u64, hash: Option<u64>) {
        self.files.insert(path.clone(), FileState { path, mtime_ms, size_bytes, content_hash: hash });
    }

    /// Record directory state.
    pub fn record_dir(&mut self, path: PathBuf, listing_hash: u64) {
        self.dirs.insert(path.clone(), DirState { path, listing_hash });
    }

    /// Check if file has changed since recorded state.
    pub fn file_changed(&self, path: &Path, new_mtime: u64, new_size: u64) -> bool {
        match self.files.get(path) {
            Some(state) => state.mtime_ms != new_mtime || state.size_bytes != new_size,
            None => true, // unknown = assume changed
        }
    }

    /// Check if dir listing changed.
    pub fn dir_changed(&self, path: &Path, new_hash: u64) -> bool {
        match self.dirs.get(path) {
            Some(state) => state.listing_hash != new_hash,
            None => true,
        }
    }
}

// ─── Dual-View Architecture ───────────────────────────────────────────────────

/// The full user view — append-only, never mutated by retention engine.
#[derive(Debug, Clone, Default)]
pub struct UserView {
    pub entries: Vec<RetentionEntry>,
    next_id: u64,
}

impl UserView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a new entry.
    pub fn append(&mut self, mut entry: RetentionEntry) -> u64 {
        let id = self.next_id;
        entry.id = id;
        self.next_id += 1;
        self.entries.push(entry);
        id
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The model view — derived from user view, sent to LLM.
#[derive(Debug, Clone, Default)]
pub struct ModelView {
    pub entries: Vec<RetentionEntry>,
    pub total_tokens: usize,
}

impl ModelView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn recalculate_tokens(&mut self) {
        self.total_tokens = self.entries.iter().map(|e| e.approx_tokens).sum();
    }
}

// ─── Retention Policy Config ──────────────────────────────────────────────────

/// Policy defaults from retention_policy.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionDefaults {
    pub n_expected: usize,
    pub recent_turns_protected: usize,
    pub compression_ratio: f64,
    pub overflow_threshold_tokens: usize,
}

impl Default for RetentionDefaults {
    fn default() -> Self {
        Self {
            n_expected: 4,
            recent_turns_protected: 4,
            compression_ratio: 0.25,
            overflow_threshold_tokens: 4000,
        }
    }
}

/// Per-entry-type policy from config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPolicy {
    #[serde(default)]
    pub ttl_turns: Option<usize>,
    #[serde(default)]
    pub invalidate_on: Vec<String>,
    #[serde(default)]
    pub default_action: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub threshold_tokens: Option<usize>,
}

/// Full retention policy loaded from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicyConfig {
    #[serde(default)]
    pub defaults: RetentionDefaults,
    #[serde(default)]
    pub entry_policies: HashMap<String, EntryPolicy>,
    #[serde(default)]
    pub dynamic_tool_retention: Option<DynamicToolRetentionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicToolRetentionConfig {
    pub retention_turns: usize,
    pub max_schema_chars: usize,
}

impl RetentionPolicyConfig {
    /// Load from file.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read retention_policy.json: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse retention_policy.json: {}", e))
    }

    /// Load from default data dir.
    pub fn load_default(data_dir: &Path) -> Result<Self, String> {
        Self::load_from_file(&data_dir.join("retention_policy.json"))
    }

    /// Get policy for a given entry type key.
    pub fn get_policy(&self, key: &str) -> Option<&EntryPolicy> {
        self.entry_policies.get(key)
    }

    /// Get TTL for an entry type, or None if no TTL configured.
    pub fn get_ttl(&self, key: &str) -> Option<usize> {
        self.entry_policies.get(key).and_then(|p| p.ttl_turns)
    }
}

// ─── Deduplication Cache ──────────────────────────────────────────────────────

/// Cache for deduplicating repeated tool calls.
#[derive(Debug, Clone, Default)]
pub struct DedupCache {
    /// Map from (tool_name, args_hash) -> (entry_id, result_content)
    cache: HashMap<(String, u64), (u64, String)>,
}

impl DedupCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if we have a cached result for this tool call.
    pub fn get(&self, tool_name: &str, args_hash: u64) -> Option<&str> {
        self.cache.get(&(tool_name.to_string(), args_hash))
            .map(|(_, content)| content.as_str())
    }

    /// Store a result.
    pub fn insert(&mut self, tool_name: String, args_hash: u64, entry_id: u64, content: String) {
        self.cache.insert((tool_name, args_hash), (entry_id, content));
    }

    /// Invalidate entries touching a resource.
    pub fn invalidate_for_file(&mut self, _path: &Path) {
        // For now, conservative: clear all. Future: track deps per cache entry.
        self.cache.clear();
    }

    /// Clear all cached results.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

// ─── Retention Engine ─────────────────────────────────────────────────────────

/// The main retention engine managing the dual-view lifecycle.
#[derive(Debug, Clone)]
pub struct RetentionEngine {
    pub user_view: UserView,
    pub model_view: ModelView,
    pub policy: RetentionPolicyConfig,
    pub state_tracker: StateTracker,
    pub dedup_cache: DedupCache,
    pub current_turn: usize,
    pub defaults: RetentionDefaults,
}

impl RetentionEngine {
    /// Create a new engine with loaded policy.
    pub fn new(policy: RetentionPolicyConfig) -> Self {
        let defaults = policy.defaults.clone();
        Self {
            user_view: UserView::new(),
            model_view: ModelView::new(),
            policy,
            state_tracker: StateTracker::new(),
            dedup_cache: DedupCache::new(),
            current_turn: 0,
            defaults,
        }
    }

    /// Create with default policy (no file load).
    pub fn with_defaults() -> Self {
        Self::new(RetentionPolicyConfig {
            defaults: RetentionDefaults::default(),
            entry_policies: HashMap::new(),
            dynamic_tool_retention: None,
        })
    }

    /// Advance to next turn.
    pub fn advance_turn(&mut self) {
        self.current_turn += 1;
    }

    /// Add an entry to the user view.
    pub fn add_entry(&mut self, entry: RetentionEntry) -> u64 {
        self.user_view.append(entry)
    }

    /// Add a tool result, checking for dedup first.
    /// Returns Some(cached_content) if deduped, None if new entry added.
    pub fn add_tool_result(
        &mut self,
        tool_name: &str,
        args_hash: u64,
        content: String,
        resources: Vec<Resource>,
        approx_tokens: usize,
    ) -> Option<String> {
        // Check dedup
        if let Some(cached) = self.dedup_cache.get(tool_name, args_hash) {
            return Some(cached.to_string());
        }

        let policy_key = self.tool_to_policy_key(tool_name);
        let ttl = self.policy.get_ttl(&policy_key);

        let entry = RetentionEntry {
            id: 0,
            kind: EntryKind::ToolResult,
            content: content.clone(),
            resources,
            created_turn: self.current_turn,
            last_used_turn: self.current_turn,
            approx_tokens,
            ttl,
            invalidation_events: Vec::new(),
            action: RetentionAction::Keep,
            args_hash: Some(args_hash),
            ephemeral: false,
            offloaded: false,
            offload_path: None,
        };

        let id = self.add_entry(entry);
        self.dedup_cache.insert(tool_name.to_string(), args_hash, id, content);
        None
    }

    /// Add an ephemeral error (send once, never persist).
    pub fn add_ephemeral_error(&mut self, content: String, approx_tokens: usize) -> u64 {
        let entry = RetentionEntry {
            id: 0,
            kind: EntryKind::ToolResult,
            content,
            resources: Vec::new(),
            created_turn: self.current_turn,
            last_used_turn: self.current_turn,
            approx_tokens,
            ttl: Some(0),
            invalidation_events: Vec::new(),
            action: RetentionAction::Prune,
            args_hash: None,
            ephemeral: true,
            offloaded: false,
            offload_path: None,
        };
        self.add_entry(entry)
    }

    /// Check if content should be offloaded (> threshold tokens).
    pub fn should_offload(&self, approx_tokens: usize) -> bool {
        approx_tokens > self.defaults.overflow_threshold_tokens
    }

    /// Offload oversized content to temp file, return preview + path.
    pub fn offload_content(
        &mut self,
        full_content: &str,
        approx_tokens: usize,
        resources: Vec<Resource>,
    ) -> (u64, PathBuf) {
        let preview_chars = 500.min(full_content.len());
        let preview = &full_content[..preview_chars];

        // Use a unique filename based on timestamp + counter to avoid collisions
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = std::env::temp_dir().join(format!("tracelean_offload_{}_{}.txt", ts, self.user_view.next_id));

        let write_ok = std::fs::write(&tmp_path, full_content).is_ok();

        let total_chars = full_content.len();
        let content = if write_ok {
            format!(
                "{}\n\n... [OFFLOADED: {} chars total, {} tokens est. Full content at: {}]\nUse read_file on the tmp path to retrieve.",
                preview, total_chars, approx_tokens, tmp_path.display()
            )
        } else {
            // Write failed — keep a larger preview instead of claiming offload worked
            let fallback_chars = 2000.min(full_content.len());
            format!(
                "{}\n\n... [TRUNCATED: {} chars total, offload to disk failed]",
                &full_content[..fallback_chars], total_chars
            )
        };

        let entry = RetentionEntry {
            id: 0,
            kind: EntryKind::OverflowPtr,
            content,
            resources,
            created_turn: self.current_turn,
            last_used_turn: self.current_turn,
            approx_tokens: preview_chars / 4 + 50, // preview tokens only
            ttl: None,
            invalidation_events: Vec::new(),
            action: RetentionAction::Keep,
            args_hash: None,
            ephemeral: false,
            offloaded: true,
            offload_path: Some(tmp_path.clone()),
        };

        let id = self.add_entry(entry);
        (id, tmp_path)
    }

    /// Notify the engine of a file edit (for invalidation).
    pub fn notify_file_edit(&mut self, path: &Path, start_line: usize, end_line: usize) {
        let event = InvalidationEvent::EditOverlapsRegion {
            path: path.to_path_buf(),
            start: start_line,
            end: end_line,
        };

        // Mark affected entries
        for entry in &mut self.user_view.entries {
            for resource in &entry.resources {
                if let Resource::File { path: rpath, start_line: rs, end_line: re } = resource {
                    if rpath == path {
                        // Check line overlap
                        let r_start = rs.unwrap_or(0);
                        let r_end = re.unwrap_or(usize::MAX);
                        if start_line < r_end && end_line > r_start {
                            entry.invalidation_events.push(event.clone());
                            entry.action = RetentionAction::Eligible;
                        }
                    }
                }
            }
        }

        // Invalidate dedup cache for this file
        self.dedup_cache.invalidate_for_file(path);

        // Invalidate search results touching this file
        for entry in &mut self.user_view.entries {
            for resource in &entry.resources {
                if let Resource::SearchQuery { scope, .. } = resource {
                    if let Some(s) = scope {
                        if path.starts_with(s) || s == "." {
                            entry.invalidation_events.push(InvalidationEvent::EditInSearchedScope {
                                path: path.to_path_buf(),
                            });
                            entry.action = RetentionAction::Eligible;
                        }
                    }
                }
            }
        }
    }

    /// Notify of directory change.
    pub fn notify_dir_change(&mut self, dir_path: &Path) {
        for entry in &mut self.user_view.entries {
            for resource in &entry.resources {
                if let Resource::Directory { path } = resource {
                    if path == dir_path {
                        entry.invalidation_events.push(InvalidationEvent::WriteInDir {
                            path: dir_path.to_path_buf(),
                        });
                        entry.action = RetentionAction::Eligible;
                    }
                }
            }
        }
    }

    /// Build the model view from user view, applying retention policies.
    /// This is the main per-request pipeline.
    pub fn build_model_view(&mut self) -> &ModelView {
        let mut entries: Vec<RetentionEntry> = Vec::new();
        let protected_turn = self.current_turn.saturating_sub(self.defaults.recent_turns_protected);

        for entry in &self.user_view.entries {
            // Skip ephemeral entries that have been sent
            if entry.ephemeral && entry.created_turn < self.current_turn {
                continue;
            }

            // Skip fully pruned entries
            if entry.action == RetentionAction::Prune && !entry.ephemeral {
                // Unless it's recent (protected)
                if entry.created_turn < protected_turn {
                    continue;
                }
            }

            // Apply TTL check
            if let Some(ttl) = entry.ttl {
                if ttl > 0 && self.current_turn - entry.created_turn > ttl {
                    // TTL expired — mark eligible but still include if recent
                    if entry.created_turn < protected_turn {
                        continue;
                    }
                }
            }

            // Strip data from old entries (tool hints, pagination)
            let mut view_entry = entry.clone();
            if entry.kind == EntryKind::ToolData && entry.created_turn < self.current_turn.saturating_sub(1) {
                view_entry.action = RetentionAction::StripData;
                view_entry.content = String::new();
                view_entry.approx_tokens = 0;
                continue; // Actually skip ToolData older than 1 turn
            }

            entries.push(view_entry);
        }

        self.model_view = ModelView { entries, total_tokens: 0 };
        self.model_view.recalculate_tokens();
        &self.model_view
    }

    /// Map tool name to policy key.
    fn tool_to_policy_key(&self, tool_name: &str) -> String {
        match tool_name {
            "discover_tools" => "discover_tools_result".to_string(),
            "list_directory" => "list_directory_result".to_string(),
            "find" => "find_result".to_string(),
            "read_file" => "read_file_content".to_string(),
            "edit_file" | "replace_str" => "edit_result".to_string(),
            "run_shell" => "run_shell_result".to_string(),
            _ => "run_shell_result".to_string(), // default
        }
    }

    /// Get entries eligible for cost-based pruning (invalidated or TTL-expired, not protected).
    pub fn eligible_for_pruning(&self) -> Vec<&RetentionEntry> {
        let protected_turn = self.current_turn.saturating_sub(self.defaults.recent_turns_protected);
        self.user_view.entries.iter()
            .filter(|e| {
                e.action == RetentionAction::Eligible
                    && e.created_turn < protected_turn
                    && !e.ephemeral
            })
            .collect()
    }

    /// Get entries in the "middle" that could be summarized (not recent, not already summarized).
    pub fn summarizable_entries(&self) -> Vec<&RetentionEntry> {
        let protected_turn = self.current_turn.saturating_sub(self.defaults.recent_turns_protected);
        self.user_view.entries.iter()
            .filter(|e| {
                e.created_turn < protected_turn
                    && e.kind != EntryKind::Summary
                    && e.action != RetentionAction::Prune
                    && !e.ephemeral
            })
            .collect()
    }

    /// Mark entries as pruned (called by cost engine after deciding).
    pub fn prune_entries(&mut self, entry_ids: &[u64]) {
        for entry in &mut self.user_view.entries {
            if entry_ids.contains(&entry.id) {
                entry.action = RetentionAction::Prune;
            }
        }
    }

    /// Insert a summary replacing a range of entries.
    pub fn insert_summary(&mut self, replaced_ids: &[u64], summary_content: String, approx_tokens: usize) {
        // Mark replaced entries as pruned
        self.prune_entries(replaced_ids);

        // Add summary entry
        let entry = RetentionEntry {
            id: 0,
            kind: EntryKind::Summary,
            content: summary_content,
            resources: Vec::new(),
            created_turn: self.current_turn,
            last_used_turn: self.current_turn,
            approx_tokens,
            ttl: None,
            invalidation_events: Vec::new(),
            action: RetentionAction::Keep,
            args_hash: None,
            ephemeral: false,
            offloaded: false,
            offload_path: None,
        };
        self.add_entry(entry);
    }

    /// Total tokens in current model view.
    pub fn model_view_tokens(&self) -> usize {
        self.model_view.total_tokens
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> RetentionEngine {
        RetentionEngine::with_defaults()
    }

    #[test]
    fn test_add_entry_and_build_view() {
        let mut engine = make_engine();
        let entry = RetentionEntry {
            id: 0,
            kind: EntryKind::UserMsg,
            content: "Hello".to_string(),
            resources: Vec::new(),
            created_turn: 0,
            last_used_turn: 0,
            approx_tokens: 2,
            ttl: None,
            invalidation_events: Vec::new(),
            action: RetentionAction::Keep,
            args_hash: None,
            ephemeral: false,
            offloaded: false,
            offload_path: None,
        };
        engine.add_entry(entry);
        engine.build_model_view();
        assert_eq!(engine.model_view.entries.len(), 1);
        assert_eq!(engine.model_view.total_tokens, 2);
    }

    #[test]
    fn test_dedup_tool_result() {
        let mut engine = make_engine();
        let resources = vec![Resource::File {
            path: PathBuf::from("test.rs"),
            start_line: Some(0),
            end_line: Some(10),
        }];

        // First call — new entry
        let result = engine.add_tool_result("read_file", 12345, "file content".to_string(), resources.clone(), 50);
        assert!(result.is_none());

        // Second call same args — deduped
        let result = engine.add_tool_result("read_file", 12345, "file content".to_string(), resources, 50);
        assert_eq!(result, Some("file content".to_string()));

        // Only one entry in user view
        assert_eq!(engine.user_view.len(), 1);
    }

    #[test]
    fn test_ephemeral_error() {
        let mut engine = make_engine();
        engine.add_ephemeral_error("Invalid JSON".to_string(), 5);

        // Same turn: visible
        engine.build_model_view();
        assert_eq!(engine.model_view.entries.len(), 1);

        // Next turn: gone
        engine.advance_turn();
        engine.build_model_view();
        assert_eq!(engine.model_view.entries.len(), 0);
    }

    #[test]
    fn test_file_edit_invalidation() {
        let mut engine = make_engine();
        let resources = vec![Resource::File {
            path: PathBuf::from("src/main.rs"),
            start_line: Some(5),
            end_line: Some(20),
        }];

        engine.add_tool_result("read_file", 111, "lines 5-20".to_string(), resources, 100);

        // Edit overlapping region
        engine.notify_file_edit(Path::new("src/main.rs"), 10, 15);

        // Entry should be marked eligible
        assert_eq!(engine.user_view.entries[0].action, RetentionAction::Eligible);
        assert!(!engine.user_view.entries[0].invalidation_events.is_empty());
    }

    #[test]
    fn test_offload_large_content() {
        let mut engine = make_engine();
        let large = "x".repeat(20000);
        let (id, path) = engine.offload_content(&large, 5000, Vec::new());

        assert!(engine.user_view.entries[0].offloaded);
        assert!(engine.user_view.entries[0].content.contains("OFFLOADED"));
        assert!(engine.user_view.entries[0].approx_tokens < 5000); // preview only

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_summarizable_entries() {
        let mut engine = make_engine();
        engine.defaults.recent_turns_protected = 2;

        // Add entries at turns 0,1,2,3,4
        for i in 0..5 {
            engine.current_turn = i;
            let entry = RetentionEntry {
                id: 0,
                kind: EntryKind::AssistantMsg,
                content: format!("msg {}", i),
                resources: Vec::new(),
                created_turn: i,
                last_used_turn: i,
                approx_tokens: 50,
                ttl: None,
                invalidation_events: Vec::new(),
                action: RetentionAction::Keep,
                args_hash: None,
                ephemeral: false,
                offloaded: false,
                offload_path: None,
            };
            engine.add_entry(entry);
        }

        engine.current_turn = 5;
        let summarizable = engine.summarizable_entries();
        // Turns 0,1,2 are summarizable (5 - 2 = 3, so < 3)
        assert_eq!(summarizable.len(), 3);
    }
}
