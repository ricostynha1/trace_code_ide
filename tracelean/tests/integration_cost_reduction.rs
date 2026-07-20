//! Integration tests for the agent cost reduction system.
//!
//! Exercises the full pipeline: tool registry → retention engine → cost model →
//! TTL tracking → cache marker placement, simulating a multi-turn session.

use tracelean_core::ai::tool_registry::ToolRegistry;
use tracelean_core::ai::provider_cache::{ProviderCacheRegistry, ProviderCacheConfig, CacheMode};
use tracelean_core::ai::retention::{
    RetentionEngine, RetentionEntry, EntryKind, RetentionAction, Resource,
    RetentionPolicyConfig,
};
use tracelean_core::ai::cost_model::{
    should_prune, should_summarize, PruneContext, CostDecision,
    batch_prune_decisions, CachePredictionTracker,
};
use tracelean_core::ai::ttl_tracking::{
    TurnTimingTracker, CacheMarkerPlanner,
};
use std::path::PathBuf;

// ─── Helper Factories ─────────────────────────────────────────────────────────

fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("data")
}

/// Number of static tools declared in data/tools.json, read straight from the
/// file so the assertions below track tools.json instead of a hardcoded literal.
fn static_tool_count() -> usize {
    let raw = std::fs::read_to_string(test_data_dir().join("tools.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
    json["tools"].as_array().unwrap().len()
}

fn make_auto_provider() -> ProviderCacheConfig {
    ProviderCacheConfig {
        cache_mode: CacheMode::Automatic,
        cache_read_discount: 0.5,
        cache_write_multiplier: 0.0,
        ttl_seconds: None,
        requires_markers: false,
        notes: None,
    }
}

fn make_explicit_provider() -> ProviderCacheConfig {
    ProviderCacheConfig {
        cache_mode: CacheMode::Explicit,
        cache_read_discount: 0.1,
        cache_write_multiplier: 1.25,
        ttl_seconds: Some(300),
        requires_markers: true,
        notes: None,
    }
}

fn make_entry(id: u64, kind: EntryKind, tokens: usize, turn: usize) -> RetentionEntry {
    RetentionEntry {
        id,
        kind,
        content: format!("content_{}", id),
        resources: Vec::new(),
        created_turn: turn,
        last_used_turn: turn,
        approx_tokens: tokens,
        ttl: None,
        invalidation_events: Vec::new(),
        action: RetentionAction::Keep,
        args_hash: None,
        ephemeral: false,
        offloaded: false,
        offload_path: None,
    }
}

// ─── Integration Tests ────────────────────────────────────────────────────────

/// Test: Load tool registry from actual data/tools.json.
#[test]
fn test_load_tool_registry_from_data() {
    let data_dir = test_data_dir();
    let registry = ToolRegistry::load_default(&data_dir).unwrap();

    // Static tool count must match what data/tools.json declares
    assert_eq!(registry.static_tools.len(), static_tool_count());

    // Check key static tools exist
    let names: Vec<&str> = registry.static_tools.iter()
        .map(|s| s.function.name.as_str())
        .collect();
    assert!(names.contains(&"discover_tools"));
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"edit_file"));
    assert!(names.contains(&"replace_str"));
    assert!(names.contains(&"find"));
    assert!(names.contains(&"list_directory"));
    assert!(names.contains(&"run_shell"));
    assert!(names.contains(&"help_tool"));
}

/// Test: Load provider cache config from actual data/provider_cache.json.
#[test]
fn test_load_provider_cache_from_data() {
    let data_dir = test_data_dir();
    let registry = ProviderCacheRegistry::load_default(&data_dir).unwrap();

    let openai = registry.get("openai").unwrap();
    assert_eq!(openai.cache_mode, CacheMode::Automatic);
    assert_eq!(openai.cache_read_discount, 0.5);

    let anthropic = registry.get("anthropic_5min").unwrap();
    assert_eq!(anthropic.cache_mode, CacheMode::Explicit);
    assert_eq!(anthropic.ttl_seconds, Some(300));
    assert_eq!(anthropic.cache_write_multiplier, 1.25);
}

/// Test: Load retention policy from actual data/retention_policy.json.
#[test]
fn test_load_retention_policy_from_data() {
    let data_dir = test_data_dir();
    let policy = RetentionPolicyConfig::load_default(&data_dir).unwrap();

    assert_eq!(policy.defaults.n_expected, 4);
    assert_eq!(policy.defaults.recent_turns_protected, 4);
    assert_eq!(policy.defaults.compression_ratio, 0.25);

    let read_policy = policy.get_policy("read_file_content").unwrap();
    assert_eq!(read_policy.ttl_turns, Some(8));
}

/// Test: Multi-turn session with dynamic tool loading, usage, and eligibility.
#[test]
fn test_multi_turn_dynamic_tool_lifecycle() {
    let data_dir = test_data_dir();
    let mut registry = ToolRegistry::load_default(&data_dir).unwrap();
    registry.config.retention_turns = 3;

    // Turn 0: load dynamic tools
    let added = registry.load_dynamic_tools(&["delete_file".to_string(), "get_symbols".to_string()]);
    assert_eq!(added.len(), 2);
    assert_eq!(registry.request_schemas().len(), static_tool_count() + 2); // static + 2 dynamic

    // Turn 1: use delete_file
    registry.advance_turn();
    registry.mark_tool_used("delete_file");

    // Turn 2, 3: advance without using get_symbols
    registry.advance_turn();
    registry.advance_turn();

    // Turn 4: get_symbols should be eligible (4 > 3, last used at 0)
    registry.advance_turn();
    let eligible = registry.eligible_for_removal();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].name, "get_symbols");

    // delete_file not eligible yet (last used at turn 1, current=4, diff=3 not > 3)
    registry.advance_turn(); // turn 5: now diff=4 > 3
    let eligible = registry.eligible_for_removal();
    assert_eq!(eligible.len(), 2);
}

/// Test: Full retention engine pipeline with file edit invalidation.
#[test]
fn test_retention_engine_invalidation_pipeline() {
    let mut engine = RetentionEngine::with_defaults();
    engine.defaults.recent_turns_protected = 2;

    // Turn 0: read a file
    let resources = vec![Resource::File {
        path: PathBuf::from("src/main.rs"),
        start_line: Some(0),
        end_line: Some(50),
    }];
    engine.add_tool_result("read_file", 111, "file contents".to_string(), resources, 200);

    // Turn 1: edit overlapping region
    engine.advance_turn();
    engine.notify_file_edit(std::path::Path::new("src/main.rs"), 10, 20);

    // Entry should be invalidated (eligible)
    assert_eq!(engine.user_view.entries[0].action, RetentionAction::Eligible);

    // Turn 5: entry no longer protected
    for _ in 0..4 {
        engine.advance_turn();
    }

    let eligible = engine.eligible_for_pruning();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].approx_tokens, 200);
}

/// Test: Cost-based pruning with automatic provider (middle prune, no penalty).
#[test]
fn test_cost_prune_middle_always_profitable() {
    let entry = make_entry(1, EntryKind::ToolResult, 500, 0);
    let ctx = PruneContext::default_for_provider(make_auto_provider(), 0.00001);

    // p_invalidated = 0 (middle prune): always profitable
    let decision = should_prune(&entry, 0, &ctx);
    assert!(decision.is_prune());

    if let CostDecision::Prune { net_benefit, .. } = decision {
        assert!(net_benefit > 0.0);
    }
}

/// Test: TTL-aware pruning — cache cold makes everything profitable.
#[test]
fn test_ttl_cold_cache_always_profitable() {
    let entry = make_entry(1, EntryKind::ToolResult, 10, 0); // tiny entry
    let mut ctx = PruneContext::default_for_provider(make_explicit_provider(), 0.00001);
    ctx.time_since_last_request_secs = 400; // > 300s TTL

    // Even with huge prefix invalidation, penalty = 0 when cold
    let decision = should_prune(&entry, 10000, &ctx);
    assert!(decision.is_prune());
}

/// Test: Summarization decision with cost comparison.
#[test]
fn test_summarization_decision_profitable() {
    let entries: Vec<RetentionEntry> = (0..10).map(|i| {
        make_entry(i, EntryKind::AssistantMsg, 200, i as usize)
    }).collect();
    let refs: Vec<&RetentionEntry> = entries.iter().collect();

    let mut ctx = PruneContext::default_for_provider(make_auto_provider(), 0.00001);
    ctx.n_expected = 6;
    ctx.compression_ratio = 0.25;
    ctx.summarizer_input_cost = 0.000005;
    ctx.summarizer_output_cost = 0.00003;

    // R=2000, S=500, Δ_net=1500
    // savings_per_turn = 1500 * 0.5 * 0.00001 = 0.0075
    // sum_cost = 2000*0.000005 + 500*0.00003 = 0.01 + 0.015 = 0.025
    // N*savings = 6 * 0.0075 = 0.045 > 0.025 → Summarize!
    let decision = should_summarize(&refs, 0, &ctx);
    assert!(decision.is_summarize());
}

/// Test: Cache prediction tracker detects anomalies.
#[test]
fn test_cache_prediction_anomaly_detection() {
    let mut tracker = CachePredictionTracker::new();

    // Normal: predicted matches actual
    for i in 0..5 {
        tracker.predict(i, 1000, 2000);
        tracker.record_actual(i, 1000);
    }
    assert_eq!(tracker.anomalies.len(), 0);

    // Anomaly: predicted 1000, actual 200 (80% deviation)
    tracker.predict(5, 1000, 2000);
    tracker.record_actual(5, 200);
    assert_eq!(tracker.anomalies.len(), 1);
    assert!(tracker.anomalies[0].deviation_pct > 70.0);
}

/// Test: Cache marker placement for explicit provider.
#[test]
fn test_cache_marker_placement() {
    let provider = make_explicit_provider();
    let planner = CacheMarkerPlanner::new(provider);

    // With n_expected=20 (long session), markers should be profitable
    let markers = planner.plan_markers(
        1000,  // system prompt
        500,   // static tools
        200,   // dynamic tools
        5000,  // conversation
        2000,  // stable conv prefix
        20,    // n_expected
    );

    assert!(!markers.is_empty());
    // Verify ascending order
    for w in markers.windows(2) {
        assert!(w[0].position_tokens <= w[1].position_tokens);
    }
}

/// Test: Turn timing tracker records idle periods.
#[test]
fn test_turn_timing_cold_detection() {
    let mut tracker = TurnTimingTracker::new();

    // Simulate a turn with short idle
    tracker.record_request_sent(0);
    tracker.record_response_received(0);

    // For testing, manually push a timing with long idle
    use tracelean_core::ai::ttl_tracking::TurnTiming;
    tracker.timings.push(TurnTiming {
        turn_id: 1,
        request_sent_at_ms: 1000000,
        response_received_at_ms: 1001000,
        idle_before_ms: 400_000, // 400s idle
        cache_predicted_cold: true,
    });

    let provider = make_explicit_provider();
    assert!(tracker.is_cache_cold(&provider)); // 400s > 300s TTL
    assert_eq!(tracker.last_idle_secs(), 400);
}

/// Test: End-to-end pipeline — retention + cost model + pruning.
#[test]
fn test_end_to_end_pipeline() {
    let mut engine = RetentionEngine::with_defaults();
    engine.defaults.recent_turns_protected = 2;

    // Simulate 8 turns of activity
    for i in 0..8 {
        engine.current_turn = i;
        let resources = vec![Resource::File {
            path: PathBuf::from(format!("src/file_{}.rs", i)),
            start_line: Some(0),
            end_line: Some(50),
        }];
        engine.add_tool_result(
            "read_file",
            (i * 100) as u64,
            format!("content of file_{}", i),
            resources,
            150,
        );
    }
    engine.current_turn = 8;

    // Build model view
    engine.build_model_view();
    assert_eq!(engine.model_view.entries.len(), 8);
    assert_eq!(engine.model_view.total_tokens, 1200); // 8 * 150

    // Invalidate some entries (edit files 0-3)
    for i in 0..4 {
        engine.notify_file_edit(
            std::path::Path::new(&format!("src/file_{}.rs", i)),
            10, 20,
        );
    }

    // Check eligible entries
    let eligible = engine.eligible_for_pruning();
    assert_eq!(eligible.len(), 4); // files 0-3 invalidated and outside protection

    // Run cost-based pruning (automatic provider, middle prune)
    let ctx = PruneContext::default_for_provider(make_auto_provider(), 0.00001);
    let (to_prune, logs) = batch_prune_decisions(&eligible, &ctx, 8);

    // All should be pruned (middle prune, p_invalidated=0, always profitable)
    assert_eq!(to_prune.len(), 4);
    assert_eq!(logs.len(), 4);
    for log in &logs {
        assert_eq!(log.decision, "prune");
    }

    // Apply pruning
    engine.prune_entries(&to_prune);

    // Rebuild model view — pruned entries gone
    engine.build_model_view();
    assert_eq!(engine.model_view.entries.len(), 4); // only recent 4 remain
    assert_eq!(engine.model_view.total_tokens, 600); // 4 * 150
}

/// Test: Deduplication prevents duplicate content in conversation.
#[test]
fn test_dedup_across_turns() {
    let mut engine = RetentionEngine::with_defaults();

    let resources = vec![Resource::File {
        path: PathBuf::from("config.json"),
        start_line: None,
        end_line: None,
    }];

    // First read
    let r1 = engine.add_tool_result("read_file", 999, "config content".to_string(), resources.clone(), 100);
    assert!(r1.is_none()); // New entry

    // Same read again (same args hash) — deduped
    let r2 = engine.add_tool_result("read_file", 999, "config content".to_string(), resources.clone(), 100);
    assert_eq!(r2, Some("config content".to_string()));

    // Only 1 entry in user view
    assert_eq!(engine.user_view.len(), 1);
}

/// Test: help_tool returns embedded_txt for both static and dynamic tools.
#[test]
fn test_help_tool_coverage() {
    let data_dir = test_data_dir();
    let registry = ToolRegistry::load_default(&data_dir).unwrap();

    // Static tool
    let help = registry.help_tool("read_file").unwrap();
    assert!(help.contains("read_file"));
    assert!(help.contains("metadata")); // from embedded_txt

    // Dynamic tool (not loaded, but available in all_entries)
    let help = registry.help_tool("delete_file").unwrap();
    assert!(help.contains("delete_file"));

    // Unknown tool
    assert!(registry.help_tool("nonexistent_tool").is_none());
}
