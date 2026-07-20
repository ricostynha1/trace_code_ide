//! Cost Model — unified formalization for cache-aware prune/summarize decisions.
//!
//! Explicit-cache is the GENERAL case. Automatic-cache is special case with w=0.
//! All decisions use the same break-even formula:
//!   N · Δ · d ≥ P_invalidated · (1 - d + w)   [when cache warm]
//!   Always profitable                           [when cache cold]
//!
//! TTL expiry tracking is critical: if user idle time > provider TTL,
//! cache was already cold → penalty = 0 → always profitable to prune.

use serde::{Deserialize, Serialize};
use super::provider_cache::ProviderCacheConfig;
use super::provider::ModelConfig;
use super::retention::RetentionEntry;

// ─── Decision Types ───────────────────────────────────────────────────────────

/// Result of a prune/summarize cost decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CostDecision {
    Prune {
        savings_per_turn: f64,
        penalty: f64,
        n_expected: usize,
        net_benefit: f64,
    },
    Summarize {
        savings_per_turn: f64,
        total_penalty: f64,
        summarization_cost: f64,
        n_expected: usize,
        net_benefit: f64,
    },
    Keep {
        reason: String,
    },
}

impl CostDecision {
    pub fn is_prune(&self) -> bool {
        matches!(self, CostDecision::Prune { .. })
    }

    pub fn is_summarize(&self) -> bool {
        matches!(self, CostDecision::Summarize { .. })
    }

    pub fn is_keep(&self) -> bool {
        matches!(self, CostDecision::Keep { .. })
    }
}

// ─── Prune Context ────────────────────────────────────────────────────────────

/// Context needed for cost-based decisions.
#[derive(Debug, Clone)]
pub struct PruneContext {
    /// Provider cache configuration (mode, discounts, TTL, write cost).
    pub provider: ProviderCacheConfig,
    /// Base cost per token (uncached input).
    pub cost_per_token: f64,
    /// Expected remaining turns (tunable, default 4).
    pub n_expected: usize,
    /// Time since last request in seconds (for TTL discount).
    pub time_since_last_request_secs: u64,
    /// Current total cached prefix tokens.
    pub cached_prefix_tokens: usize,
    /// Compression ratio for summarization (default 0.25).
    pub compression_ratio: f64,
    /// Summarizer model input cost per token (brain or cheap — engine picks cheapest).
    pub summarizer_input_cost: f64,
    /// Summarizer model output cost per token.
    pub summarizer_output_cost: f64,
}

impl PruneContext {
    /// Create with sensible defaults for testing.
    pub fn default_for_provider(provider: ProviderCacheConfig, cost_per_token: f64) -> Self {
        Self {
            provider,
            cost_per_token,
            n_expected: 4,
            time_since_last_request_secs: 0,
            cached_prefix_tokens: 0,
            compression_ratio: 0.25,
            summarizer_input_cost: cost_per_token * 0.5, // assume cached or cheap
            summarizer_output_cost: cost_per_token * 3.0, // output is expensive
        }
    }

    /// Build from main model + optional summary model config.
    /// If `summary_model` is Some, uses its pricing for summarization cost.
    /// Otherwise falls back to main model pricing with heuristic multipliers.
    pub fn from_models(
        provider: ProviderCacheConfig,
        main_model: &ModelConfig,
        summary_model: Option<&ModelConfig>,
    ) -> Self {
        let cost_per_token = main_model.input_cost_per_m / 1_000_000.0;
        let (sum_in, sum_out) = match summary_model {
            Some(sm) => (
                sm.input_cost_per_m / 1_000_000.0,
                sm.output_cost_per_m / 1_000_000.0,
            ),
            None => (
                cost_per_token * 0.5,   // heuristic: cached reads are ~50% cheaper
                cost_per_token * 3.0,   // heuristic: output ≈ 3x input
            ),
        };
        Self {
            provider,
            cost_per_token,
            n_expected: 4,
            time_since_last_request_secs: 0,
            cached_prefix_tokens: 0,
            compression_ratio: 0.25,
            summarizer_input_cost: sum_in,
            summarizer_output_cost: sum_out,
        }
    }

    /// Whether the cache is cold (idle time > TTL).
    fn cache_is_cold(&self) -> bool {
        self.provider.cache_is_cold(self.time_since_last_request_secs)
    }

    /// Pick cheapest summarizer costs.
    /// Returns the configured values (populated from summary_model if set, else heuristic).
    pub fn pick_cheapest_summarizer(&self) -> (f64, f64) {
        (self.summarizer_input_cost, self.summarizer_output_cost)
    }
}

// ─── Core Decision Functions ──────────────────────────────────────────────────

/// Unified prune decision. Works for both automatic (w=0) and explicit (w>0) providers.
///
/// # Math
/// ```text
/// savings_per_turn = Δ · d · c
///   Each future turn sends Δ fewer cached tokens, saving Δ·d·c per turn.
///
/// penalty = P_invalidated · (1 - d + w) · c    [when cache warm]
///   One-time cost: invalidated prefix re-processed at full price + cache re-write cost.
///   For automatic providers: w=0, so penalty = P_invalidated · (1-d) · c
///   For explicit providers: w>0, adds write cost on top.
///   When cache cold (idle > TTL): penalty = 0.
///
/// Break-even: N · savings_per_turn ≥ penalty
/// Simplified: N · Δ · d ≥ P_invalidated · (1 - d + w)
/// ```
pub fn should_prune(entry: &RetentionEntry, p_invalidated: usize, ctx: &PruneContext) -> CostDecision {
    let delta = entry.approx_tokens;
    let d = ctx.provider.cache_read_discount;
    let w = ctx.provider.cache_write_multiplier;
    let c = ctx.cost_per_token;
    let n = ctx.n_expected;

    // savings_per_turn = Δ · d · c
    let savings_per_turn = delta as f64 * d * c;

    // penalty = P_invalidated · (1 - d + w) · c  [when cache warm]
    //         = 0                                  [when cache cold]
    let penalty = if ctx.cache_is_cold() {
        0.0 // Cache already expired — no miss penalty
    } else {
        p_invalidated as f64 * (1.0 - d + w) * c
    };

    let net_benefit = n as f64 * savings_per_turn - penalty;

    // Break-even: N · savings ≥ penalty
    if net_benefit >= 0.0 {
        CostDecision::Prune {
            savings_per_turn,
            penalty,
            n_expected: n,
            net_benefit,
        }
    } else {
        CostDecision::Keep {
            reason: format!(
                "penalty ({:.6}) exceeds expected savings ({:.6}), deficit={:.6}",
                penalty, n as f64 * savings_per_turn, -net_benefit
            ),
        }
    }
}

/// Summarization decision. Same unified model + summarization LLM cost.
///
/// # Math
/// ```text
/// Δ_net = R - S    (net tokens saved, where S ≈ compression_ratio · R)
///
/// savings_per_turn = Δ_net · d · c
///
/// cache_penalty = P_invalidated · (1 - d + w) · c   [when warm]
///               = 0                                   [when cold]
///
/// summarization_cost = R · c_sum_in + S · c_sum_out
///   (LLM call cost to produce the summary)
///
/// total_penalty = cache_penalty + summarization_cost
///
/// Break-even: N · savings_per_turn ≥ total_penalty
/// Expanded: N · (R-S) · d · c ≥ P_invalidated · (1-d+w) · c + R·c_sum_in + S·c_sum_out
/// ```
pub fn should_summarize(
    entries: &[&RetentionEntry],
    p_invalidated: usize,
    ctx: &PruneContext,
) -> CostDecision {
    let r: usize = entries.iter().map(|e| e.approx_tokens).sum();
    if r == 0 {
        return CostDecision::Keep { reason: "nothing to summarize".to_string() };
    }

    let s = (r as f64 * ctx.compression_ratio) as usize;
    let delta_net = r.saturating_sub(s);
    let d = ctx.provider.cache_read_discount;
    let w = ctx.provider.cache_write_multiplier;
    let c = ctx.cost_per_token;
    let n = ctx.n_expected;

    // savings_per_turn = Δ_net · d · c
    let savings_per_turn = delta_net as f64 * d * c;

    // Cache miss penalty
    let cache_penalty = if ctx.cache_is_cold() {
        0.0
    } else {
        p_invalidated as f64 * (1.0 - d + w) * c
    };

    // Summarization LLM call cost = R · c_sum_in + S · c_sum_out
    let (sum_in_cost, sum_out_cost) = ctx.pick_cheapest_summarizer();
    let summarization_cost = r as f64 * sum_in_cost + s as f64 * sum_out_cost;

    let total_penalty = cache_penalty + summarization_cost;
    let net_benefit = n as f64 * savings_per_turn - total_penalty;

    if net_benefit >= 0.0 {
        CostDecision::Summarize {
            savings_per_turn,
            total_penalty,
            summarization_cost,
            n_expected: n,
            net_benefit,
        }
    } else {
        CostDecision::Keep {
            reason: format!(
                "summarization cost ({:.6}) + cache penalty ({:.6}) exceeds expected savings ({:.6})",
                summarization_cost, cache_penalty, n as f64 * savings_per_turn
            ),
        }
    }
}

// ─── Batch Decision Engine ────────────────────────────────────────────────────

/// Decision log entry for observability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionLog {
    pub turn: usize,
    pub entry_id: u64,
    pub entry_kind: String,
    pub resource: String,
    pub tokens: usize,
    pub decision: String,
    pub savings_per_turn: f64,
    pub penalty: f64,
    pub n_expected: usize,
    pub provider: String,
    pub cache_was_cold: bool,
    pub time_since_last_request_s: u64,
}

/// Prefix invalidation estimator.
/// Computes how many cached prefix tokens would be invalidated by removing an entry.
pub fn estimate_prefix_invalidation(
    _entry: &RetentionEntry,
    cached_prefix_tokens: usize,
    entry_position_tokens: usize,
) -> usize {
    // If entry is in the cached prefix region, removing it invalidates everything after.
    // If entry is after the prefix (in conversation), removing from middle = 0 invalidation.
    if entry_position_tokens < cached_prefix_tokens {
        // Entry is within cached prefix — invalidation = prefix after this point
        cached_prefix_tokens - entry_position_tokens
    } else {
        // Entry is in conversation body (after prefix) — no prefix invalidation
        // Middle prune: always profitable
        0
    }
}

/// Run cost-based decisions on a batch of eligible entries.
/// Returns (entries_to_prune, decision_logs).
pub fn batch_prune_decisions(
    eligible: &[&RetentionEntry],
    ctx: &PruneContext,
    current_turn: usize,
) -> (Vec<u64>, Vec<DecisionLog>) {
    let mut to_prune: Vec<u64> = Vec::new();
    let mut logs: Vec<DecisionLog> = Vec::new();

    for entry in eligible {
        // Estimate prefix invalidation (most entries are in conversation body = 0)
        let p_invalidated = estimate_prefix_invalidation(entry, ctx.cached_prefix_tokens, 0);

        let decision = should_prune(entry, p_invalidated, ctx);

        let log = DecisionLog {
            turn: current_turn,
            entry_id: entry.id,
            entry_kind: format!("{:?}", entry.kind),
            resource: entry.resources.first()
                .map(|r| format!("{:?}", r))
                .unwrap_or_else(|| "none".to_string()),
            tokens: entry.approx_tokens,
            decision: match &decision {
                CostDecision::Prune { .. } => "prune".to_string(),
                CostDecision::Keep { reason } => format!("keep: {}", reason),
                CostDecision::Summarize { .. } => "summarize".to_string(),
            },
            savings_per_turn: match &decision {
                CostDecision::Prune { savings_per_turn, .. } => *savings_per_turn,
                _ => 0.0,
            },
            penalty: match &decision {
                CostDecision::Prune { penalty, .. } => *penalty,
                _ => 0.0,
            },
            n_expected: ctx.n_expected,
            provider: format!("{:?}", ctx.provider.cache_mode),
            cache_was_cold: ctx.cache_is_cold(),
            time_since_last_request_s: ctx.time_since_last_request_secs,
        };

        if decision.is_prune() {
            to_prune.push(entry.id);
        }

        logs.push(log);
    }

    (to_prune, logs)
}

/// Run summarization decision on a batch of entries.
/// Returns CostDecision indicating whether to summarize.
pub fn summarization_decision(
    entries: &[&RetentionEntry],
    ctx: &PruneContext,
) -> CostDecision {
    // For middle-prune (conversation body), p_invalidated = 0
    should_summarize(entries, 0, ctx)
}

// ─── Dynamic tool injection cost (bugs.md Feature 3) ─────────────────────────

/// Cache-invalidation penalty of ADDING a dynamic tool schema.
///
/// Body-injected schemas (embedded-text path: the schema rides in a trailing
/// user message) are ordinary appended content — the cached prefix is
/// untouched, so the penalty is 0 regardless of prefix size.
///
/// Native-path additions (schema merged into the `tools` request param) change
/// the prefix itself, so the whole cached prefix re-processes at the usual
/// `P_invalidated · (1 − d + w) · c` rate.
pub fn dynamic_tool_add_penalty(
    body_injected: bool,
    cached_prefix_tokens: usize,
    ctx: &PruneContext,
) -> f64 {
    if body_injected || ctx.cache_is_cold() {
        return 0.0;
    }
    let d = ctx.provider.cache_read_discount;
    let w = ctx.provider.cache_write_multiplier;
    cached_prefix_tokens as f64 * (1.0 - d + w) * ctx.cost_per_token
}

// ─── Token Prediction & Anomaly Detection ─────────────────────────────────────

/// Tracks predicted vs actual cached tokens for anomaly detection.
#[derive(Debug, Clone, Default)]
pub struct CachePredictionTracker {
    pub history: Vec<CachePrediction>,
    pub anomalies: Vec<CacheAnomaly>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachePrediction {
    pub turn: usize,
    pub predicted_cached_tokens: usize,
    pub actual_cached_tokens: Option<usize>,
    pub total_input_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheAnomaly {
    pub turn: usize,
    pub predicted: usize,
    pub actual: usize,
    pub deviation_pct: f64,
}

impl CachePredictionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a prediction before sending request.
    pub fn predict(&mut self, turn: usize, predicted_cached: usize, total_input: usize) {
        self.history.push(CachePrediction {
            turn,
            predicted_cached_tokens: predicted_cached,
            actual_cached_tokens: None,
            total_input_tokens: total_input,
        });
    }

    /// Record actual result after response (from usage stats).
    pub fn record_actual(&mut self, turn: usize, actual_cached: usize) {
        if let Some(entry) = self.history.iter_mut().rev().find(|e| e.turn == turn) {
            entry.actual_cached_tokens = Some(actual_cached);

            // Check for anomaly (>20% deviation)
            let predicted = entry.predicted_cached_tokens as f64;
            let actual_f = actual_cached as f64;
            if predicted > 0.0 {
                let deviation = ((actual_f - predicted) / predicted).abs();
                if deviation > 0.20 {
                    self.anomalies.push(CacheAnomaly {
                        turn,
                        predicted: entry.predicted_cached_tokens,
                        actual: actual_cached,
                        deviation_pct: deviation * 100.0,
                    });
                }
            }
        }
    }

    /// Get recent anomaly count (last 10 turns).
    pub fn recent_anomaly_count(&self) -> usize {
        let cutoff = self.history.len().saturating_sub(10);
        self.anomalies.iter()
            .filter(|a| a.turn >= cutoff)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::provider_cache::CacheMode;
    use super::super::retention::*;

    fn auto_provider() -> ProviderCacheConfig {
        ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.5,
            cache_write_multiplier: 0.0,
            ttl_seconds: None,
            requires_markers: false,
            notes: None,
        }
    }

    fn explicit_provider() -> ProviderCacheConfig {
        ProviderCacheConfig {
            cache_mode: CacheMode::Explicit,
            cache_read_discount: 0.1,
            cache_write_multiplier: 1.25,
            ttl_seconds: Some(300),
            requires_markers: true,
            notes: None,
        }
    }

    fn test_entry(tokens: usize) -> RetentionEntry {
        RetentionEntry {
            id: 1,
            kind: EntryKind::ToolResult,
            content: "test".to_string(),
            resources: Vec::new(),
            created_turn: 0,
            last_used_turn: 0,
            approx_tokens: tokens,
            ttl: None,
            invalidation_events: Vec::new(),
            action: RetentionAction::Eligible,
            args_hash: None,
            ephemeral: false,
            offloaded: false,
            offload_path: None,
        }
    }

    #[test]
    fn test_prune_automatic_no_invalidation() {
        // Middle prune (p_invalidated=0): always profitable
        let entry = test_entry(500);
        let ctx = PruneContext::default_for_provider(auto_provider(), 0.00001);
        let decision = should_prune(&entry, 0, &ctx);
        assert!(decision.is_prune());
    }

    #[test]
    fn test_prune_automatic_with_invalidation() {
        // Large invalidation vs small savings
        let entry = test_entry(10); // only 10 tokens saved
        let mut ctx = PruneContext::default_for_provider(auto_provider(), 0.00001);
        ctx.n_expected = 2;

        // 1000 tokens invalidated: penalty = 1000 * (1-0.5) * 0.00001 = 0.005
        // savings = 2 * 10 * 0.5 * 0.00001 = 0.0001
        // 0.0001 < 0.005 → Keep
        let decision = should_prune(&entry, 1000, &ctx);
        assert!(decision.is_keep());
    }

    #[test]
    fn test_prune_explicit_cache_cold() {
        // Cache cold → penalty=0 → always profitable
        let entry = test_entry(100);
        let mut ctx = PruneContext::default_for_provider(explicit_provider(), 0.00001);
        ctx.time_since_last_request_secs = 400; // > 300s TTL

        let decision = should_prune(&entry, 5000, &ctx);
        assert!(decision.is_prune()); // penalty=0 because cache was cold
    }

    #[test]
    fn test_prune_explicit_cache_warm() {
        // Small entry, large invalidation, warm cache → keep
        let entry = test_entry(5);
        let mut ctx = PruneContext::default_for_provider(explicit_provider(), 0.00001);
        ctx.time_since_last_request_secs = 100; // < 300s TTL (warm)
        ctx.n_expected = 4;

        // penalty = 2000 * (1 - 0.1 + 1.25) * 0.00001 = 2000 * 2.15 * 0.00001 = 0.043
        // savings = 4 * 5 * 0.1 * 0.00001 = 0.00002
        // 0.00002 < 0.043 → Keep
        let decision = should_prune(&entry, 2000, &ctx);
        assert!(decision.is_keep());
    }

    #[test]
    fn test_summarize_profitable() {
        // Many tokens in middle, no prefix invalidation
        let entries: Vec<RetentionEntry> = (0..5).map(|i| {
            let mut e = test_entry(200);
            e.id = i;
            e
        }).collect();
        let refs: Vec<&RetentionEntry> = entries.iter().collect();

        let mut ctx = PruneContext::default_for_provider(auto_provider(), 0.00001);
        ctx.n_expected = 4;
        ctx.compression_ratio = 0.25;
        ctx.summarizer_input_cost = 0.000005; // cheap
        ctx.summarizer_output_cost = 0.00003;

        // R = 1000, S = 250, Δ_net = 750
        // savings_per_turn = 750 * 0.5 * 0.00001 = 0.00375
        // sum_cost = 1000 * 0.000005 + 250 * 0.00003 = 0.005 + 0.0075 = 0.0125
        // N * savings = 4 * 0.00375 = 0.015
        // 0.015 >= 0.0125 → Summarize
        let decision = should_summarize(&refs, 0, &ctx);
        assert!(decision.is_summarize());
    }

    #[test]
    fn test_summarize_too_expensive() {
        let entries: Vec<RetentionEntry> = (0..2).map(|i| {
            let mut e = test_entry(50);
            e.id = i;
            e
        }).collect();
        let refs: Vec<&RetentionEntry> = entries.iter().collect();

        let mut ctx = PruneContext::default_for_provider(auto_provider(), 0.00001);
        ctx.n_expected = 2;
        ctx.compression_ratio = 0.25;
        ctx.summarizer_input_cost = 0.0001; // expensive summarizer
        ctx.summarizer_output_cost = 0.0003;

        // R = 100, S = 25, Δ_net = 75
        // savings_per_turn = 75 * 0.5 * 0.00001 = 0.000375
        // sum_cost = 100 * 0.0001 + 25 * 0.0003 = 0.01 + 0.0075 = 0.0175
        // N * savings = 2 * 0.000375 = 0.00075
        // 0.00075 < 0.0175 → Keep
        let decision = should_summarize(&refs, 0, &ctx);
        assert!(decision.is_keep());
    }

    #[test]
    fn test_prefix_invalidation_estimate() {
        let entry = test_entry(100);

        // Entry in prefix region
        assert_eq!(estimate_prefix_invalidation(&entry, 1000, 200), 800);

        // Entry after prefix (conversation body)
        assert_eq!(estimate_prefix_invalidation(&entry, 1000, 1500), 0);
    }

    #[test]
    fn dynamic_tool_body_injection_is_free() {
        // Feature 3: schemas appended in the message body never invalidate
        // the cached prefix — penalty must be exactly 0 even with a huge
        // warm prefix.
        let mut ctx = PruneContext::default_for_provider(explicit_provider(), 0.00001);
        ctx.time_since_last_request_secs = 10; // warm cache
        assert_eq!(dynamic_tool_add_penalty(true, 100_000, &ctx), 0.0);
    }

    #[test]
    fn dynamic_tool_native_add_pays_invalidation() {
        let mut ctx = PruneContext::default_for_provider(explicit_provider(), 0.00001);
        ctx.time_since_last_request_secs = 10; // warm cache
        let p = dynamic_tool_add_penalty(false, 10_000, &ctx);
        // 10_000 * (1 - 0.1 + 1.25) * 0.00001 = 0.215
        assert!((p - 0.215).abs() < 1e-9, "penalty {}", p);
        // …but a cold cache makes even the native add free.
        ctx.time_since_last_request_secs = 400;
        assert_eq!(dynamic_tool_add_penalty(false, 10_000, &ctx), 0.0);
    }

    #[test]
    fn test_cache_prediction_anomaly() {
        let mut tracker = CachePredictionTracker::new();
        tracker.predict(1, 1000, 2000);
        tracker.record_actual(1, 1000); // exact match, no anomaly
        assert_eq!(tracker.anomalies.len(), 0);

        tracker.predict(2, 1000, 2000);
        tracker.record_actual(2, 500); // 50% deviation
        assert_eq!(tracker.anomalies.len(), 1);
        assert!(tracker.anomalies[0].deviation_pct > 40.0);
    }
}
