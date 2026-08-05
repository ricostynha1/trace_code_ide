//! TTL Tracking & Cache Marker Placement — critical for explicit-cache providers.
//!
//! Tracks inter-turn timing to detect cache coldness and optimally place
//! cache markers for explicit providers (Anthropic, MiniMax explicit).
//!
//! Key insight: if user think-time > provider TTL, cache was already cold.
//! In that regime, penalty = 0, so pruning is always profitable.

use serde::{Deserialize, Serialize};
use super::provider_cache::{CacheMode, ProviderCacheConfig};

// ─── Turn Timing ──────────────────────────────────────────────────────────────

/// Timing data for a single turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnTiming {
    pub turn_id: usize,
    /// Unix timestamp ms when request was sent.
    pub request_sent_at_ms: u64,
    /// Unix timestamp ms when response was received.
    pub response_received_at_ms: u64,
    /// Idle time before this turn (time between previous response and this request).
    pub idle_before_ms: u64,
    /// Whether cache was predicted cold for this turn.
    pub cache_predicted_cold: bool,
}

impl TurnTiming {
    /// Duration of the LLM call itself.
    pub fn llm_duration_ms(&self) -> u64 {
        self.response_received_at_ms.saturating_sub(self.request_sent_at_ms)
    }
}

/// Tracks turn timing across the session for TTL-aware decisions.
#[derive(Debug, Clone)]
pub struct TurnTimingTracker {
    pub timings: Vec<TurnTiming>,
    /// Last response timestamp for computing idle time.
    last_response_at_ms: Option<u64>,
    /// Session start time.
    session_start_ms: u64,
}

impl TurnTimingTracker {
    pub fn new() -> Self {
        Self {
            timings: Vec::new(),
            last_response_at_ms: None,
            session_start_ms: Self::now_ms(),
        }
    }

    /// Record that a request is being sent now.
    pub fn record_request_sent(&mut self, turn_id: usize) -> u64 {
        let now = Self::now_ms();
        let idle = match self.last_response_at_ms {
            Some(last) => now.saturating_sub(last),
            None => now.saturating_sub(self.session_start_ms),
        };

        self.timings.push(TurnTiming {
            turn_id,
            request_sent_at_ms: now,
            response_received_at_ms: 0, // filled later
            idle_before_ms: idle,
            cache_predicted_cold: false, // filled by cache predictor
        });

        idle
    }

    /// Record that a response was received.
    pub fn record_response_received(&mut self, turn_id: usize) {
        let now = Self::now_ms();
        self.last_response_at_ms = Some(now);

        if let Some(timing) = self.timings.iter_mut().rev().find(|t| t.turn_id == turn_id) {
            timing.response_received_at_ms = now;
        }
    }

    /// Mark a turn's cache prediction.
    pub fn mark_cache_cold(&mut self, turn_id: usize) {
        if let Some(timing) = self.timings.iter_mut().rev().find(|t| t.turn_id == turn_id) {
            timing.cache_predicted_cold = true;
        }
    }

    /// Get idle time before the most recent turn (in seconds).
    pub fn last_idle_secs(&self) -> u64 {
        self.timings.last()
            .map(|t| t.idle_before_ms / 1000)
            .unwrap_or(0)
    }

    /// Get idle time in ms for the last turn.
    pub fn last_idle_ms(&self) -> u64 {
        self.timings.last()
            .map(|t| t.idle_before_ms)
            .unwrap_or(0)
    }

    /// Check if cache is likely cold for the current turn given a provider's TTL.
    pub fn is_cache_cold(&self, provider: &ProviderCacheConfig) -> bool {
        provider.cache_is_cold(self.last_idle_secs())
    }

    /// Average idle time across all turns (for session characterization).
    pub fn avg_idle_ms(&self) -> u64 {
        if self.timings.is_empty() {
            return 0;
        }
        let total: u64 = self.timings.iter().map(|t| t.idle_before_ms).sum();
        total / self.timings.len() as u64
    }

    /// Percentage of turns where cache was cold.
    pub fn cold_turn_ratio(&self) -> f64 {
        if self.timings.is_empty() {
            return 0.0;
        }
        let cold = self.timings.iter().filter(|t| t.cache_predicted_cold).count();
        cold as f64 / self.timings.len() as f64
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Default for TurnTimingTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Cache Marker Placement ───────────────────────────────────────────────────

/// A cache marker position in the request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMarker {
    /// Position in tokens from start of payload.
    pub position_tokens: usize,
    /// What content block this marker is after.
    pub after_block: CacheBlock,
    /// Estimated token savings from caching up to this point.
    pub estimated_savings: f64,
}

/// Logical blocks in the request payload where markers can be placed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheBlock {
    SystemPrompt,
    StaticTools,
    DynamicTools,
    /// A stable conversation prefix (e.g. after turn N).
    ConversationPrefix { up_to_turn: usize },
    /// After a specific message index.
    AfterMessage { index: usize },
}

/// Determines optimal cache marker positions for explicit-cache providers.
#[derive(Debug, Clone)]
pub struct CacheMarkerPlanner {
    /// Provider config.
    pub provider: ProviderCacheConfig,
    /// Max markers allowed (Anthropic allows up to 4).
    pub max_markers: usize,
    /// Minimum prefix size (tokens) a candidate position must clear before
    /// it's worth marking at all — sourced from the caller's resolved
    /// `ModelConfig::cache_min_tokens` (itself populated from
    /// `data/models.json`'s per-model `cache_min_tokens` field; see
    /// `model_catalog::enrich`). A marker below this floor is a paid write
    /// that the provider will never actually cache — Bedrock silently serves
    /// the request without caching rather than erroring, so without this
    /// check the planner keeps recommending markers that can never be read
    /// back. 0 disables the floor (used by tests exercising the
    /// marker-position economics in isolation, not modeling a specific real
    /// model).
    pub min_cacheable_tokens: usize,
}

/// D9b.1 write-if-worth-it value of a marker at `prefix_tokens`:
/// each expected reuse saves `(1 - read_discount)` of the prefix price;
/// the one-time write costs the surcharge `(write_multiplier - 1)`.
fn marker_value(prefix_tokens: usize, read_discount: f64, write_multiplier: f64, n_expected: usize) -> f64 {
    let p = prefix_tokens as f64;
    p * (1.0 - read_discount) * n_expected as f64 - p * (write_multiplier - 1.0).max(0.0)
}

impl CacheMarkerPlanner {

    pub fn new(provider: ProviderCacheConfig, min_cacheable_tokens: usize) -> Self {
        let max_markers = match provider.cache_mode {
            CacheMode::Explicit => 4,
            CacheMode::Automatic => 0, // no markers needed
        };
        Self { provider, max_markers, min_cacheable_tokens }
    }

    /// Plan cache marker positions given the request structure.
    ///
    /// Strategy: place markers at boundaries of stable content blocks.
    /// The cost engine optimizes for maximum cache hit value:
    ///   value = tokens_cached * cache_read_discount * expected_reuse_turns
    ///         - tokens_cached * cache_write_multiplier (one-time write cost)
    pub fn plan_markers(
        &self,
        system_prompt_tokens: usize,
        static_tools_tokens: usize,
        dynamic_tools_tokens: usize,
        _conversation_tokens: usize,
        stable_conversation_prefix_tokens: usize,
        n_expected: usize,
    ) -> Vec<CacheMarker> {
        if self.max_markers == 0 {
            return Vec::new(); // automatic provider, no markers
        }

        let d = self.provider.cache_read_discount;
        let w = self.provider.cache_write_multiplier;

        let mut candidates: Vec<CacheMarker> = Vec::new();

        // Candidate 1: After system prompt
        let pos1 = system_prompt_tokens;
        let savings1 = marker_value(pos1, d, w, n_expected);
        if savings1 > 0.0 && pos1 >= self.min_cacheable_tokens {
            candidates.push(CacheMarker {
                position_tokens: pos1,
                after_block: CacheBlock::SystemPrompt,
                estimated_savings: savings1,
            });
        }

        // Candidate 2: After system prompt + static tools
        let pos2 = system_prompt_tokens + static_tools_tokens;
        let savings2 = marker_value(pos2, d, w, n_expected);
        if savings2 > savings1.max(0.0) && pos2 >= self.min_cacheable_tokens {
            candidates.push(CacheMarker {
                position_tokens: pos2,
                after_block: CacheBlock::StaticTools,
                estimated_savings: savings2,
            });
        }

        // Candidate 3: After all tools (if dynamic tools are stable)
        if dynamic_tools_tokens > 0 {
            let pos3 = pos2 + dynamic_tools_tokens;
            let savings3 = marker_value(pos3, d, w, n_expected);
            if savings3 > savings2.max(0.0) && pos3 >= self.min_cacheable_tokens {
                candidates.push(CacheMarker {
                    position_tokens: pos3,
                    after_block: CacheBlock::DynamicTools,
                    estimated_savings: savings3,
                });
            }
        }

        // Candidate 4: Deep in conversation (stable prefix)
        if stable_conversation_prefix_tokens > 100 {
            let pos4 = pos2 + dynamic_tools_tokens + stable_conversation_prefix_tokens;
            let savings4 = marker_value(pos4, d, w, n_expected);
            if savings4 > 0.0 && pos4 >= self.min_cacheable_tokens {
                candidates.push(CacheMarker {
                    position_tokens: pos4,
                    after_block: CacheBlock::ConversationPrefix { up_to_turn: 0 },
                    estimated_savings: savings4,
                });
            }
        }

        // Sort by savings descending, take top max_markers
        candidates.sort_by(|a, b| b.estimated_savings.partial_cmp(&a.estimated_savings).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(self.max_markers);

        // Re-sort by position (markers must be in order)
        candidates.sort_by_key(|m| m.position_tokens);
        candidates
    }
}

// ─── TTL-Aware Penalty Discount ───────────────────────────────────────────────

/// Compute the effective penalty given TTL state.
/// If cache is cold (idle > TTL), penalty = 0.
/// If cache is warm but approaching TTL, apply partial discount.
///
/// # Math
/// ```text
/// When T_idle > TTL_provider:
///   effective_penalty = 0   (cache already expired)
///
/// When T_idle ≤ TTL_provider:
///   effective_penalty = base_penalty   (cache is warm, full penalty applies)
///
/// Future refinement: linear discount as idle approaches TTL:
///   discount = min(1.0, T_idle / TTL)
///   effective_penalty = base_penalty * (1.0 - discount)
/// ```
pub fn ttl_adjusted_penalty(
    base_penalty: f64,
    idle_secs: u64,
    provider: &ProviderCacheConfig,
) -> f64 {
    match provider.ttl_seconds {
        Some(ttl) => {
            if idle_secs >= ttl {
                0.0 // Cache cold — no penalty
            } else {
                // Cache warm — full penalty
                // Future: could apply partial discount as we approach TTL
                base_penalty
            }
        }
        None => base_penalty, // Automatic — no TTL concept
    }
}

/// Determine if current session is "cold-dominated" — useful for switching
/// to more aggressive pruning strategy.
pub fn is_session_cold_dominated(tracker: &TurnTimingTracker, threshold: f64) -> bool {
    tracker.cold_turn_ratio() > threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit_300s() -> ProviderCacheConfig {
        ProviderCacheConfig {
            cache_mode: CacheMode::Explicit,
            cache_read_discount: 0.1,
            cache_write_multiplier: 1.25,
            ttl_seconds: Some(300),
            requires_markers: true,
            notes: None,
        }
    }

    fn automatic() -> ProviderCacheConfig {
        ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.5,
            cache_write_multiplier: 0.0,
            ttl_seconds: None,
            requires_markers: false,
            notes: None,
        }
    }

    #[test]
    fn test_ttl_adjusted_penalty_cold() {
        let provider = explicit_300s();
        assert_eq!(ttl_adjusted_penalty(1.0, 400, &provider), 0.0);
    }

    #[test]
    fn test_ttl_adjusted_penalty_warm() {
        let provider = explicit_300s();
        assert_eq!(ttl_adjusted_penalty(1.0, 100, &provider), 1.0);
    }

    #[test]
    fn test_ttl_adjusted_penalty_automatic() {
        let provider = automatic();
        assert_eq!(ttl_adjusted_penalty(1.0, 99999, &provider), 1.0);
    }

    #[test]
    fn test_marker_planner_explicit() {
        let provider = explicit_300s();
        let planner = CacheMarkerPlanner::new(provider, 0);

        // With Anthropic (d=0.1, w=1.25), break-even is N*d > w → N > 12.5
        // So we need n_expected >= 13 for markers to be profitable
        let markers = planner.plan_markers(
            500,   // system prompt
            300,   // static tools
            100,   // dynamic tools
            2000,  // conversation
            800,   // stable prefix
            15,    // n_expected — high enough for Anthropic markers to be profitable
        );

        assert!(!markers.is_empty());
        assert!(markers.len() <= 4);
        // Markers should be in ascending position order
        for w in markers.windows(2) {
            assert!(w[0].position_tokens <= w[1].position_tokens);
        }
    }

    #[test]
    fn zero_tool_tokens_collapses_static_tools_candidate_onto_system_prompt() {
        // Regression test for the runtime's cache-prediction Bug B: feeding
        // 0 for static/dynamic tool tokens (as `plan_cache_breakpoints` used
        // to) put the "after static tools" candidate at the exact same
        // position as "after system prompt alone", so it could never win the
        // `>` comparison and the tool schemas' real wire cost was invisible
        // to every downstream candidate's position math.
        let planner = CacheMarkerPlanner::new(explicit_300s(), 0);
        let with_zero_tools = planner.plan_markers(500, 0, 0, 2000, 0, 15);
        assert!(
            with_zero_tools.iter().all(|m| m.position_tokens == 500),
            "with no tool tokens, only the system-prompt position should appear: {:?}",
            with_zero_tools
        );

        let with_real_tools = planner.plan_markers(500, 300, 100, 2000, 0, 15);
        assert!(
            with_real_tools.iter().any(|m| m.position_tokens == 800),
            "static tools should push a candidate to system+static tokens: {:?}",
            with_real_tools
        );
        assert!(
            with_real_tools.iter().any(|m| m.position_tokens == 900),
            "dynamic tools should push a candidate to system+static+dynamic tokens: {:?}",
            with_real_tools
        );
    }

    #[test]
    fn test_marker_planner_automatic() {
        let provider = automatic();
        let planner = CacheMarkerPlanner::new(provider, 0);

        let markers = planner.plan_markers(500, 300, 100, 2000, 800, 4);
        assert!(markers.is_empty()); // no markers for automatic providers
    }

    /// The floor exists specifically because Bedrock silently no-ops a
    /// cachePoint under the model's minimum instead of erroring — without
    /// this gate the planner would keep recommending (and paying the write
    /// surcharge for) markers that can never be read back. Verified live:
    /// this exact scenario (a ~900-token system+tools prefix) cached
    /// correctly on Sonnet 4.6 (1,024 floor) but never wrote on Haiku 4.5
    /// (4,096 floor) in the same agent conversation.
    #[test]
    fn candidates_below_the_model_floor_are_not_marked() {
        let provider = explicit_300s();

        // Below a 4096-token floor (e.g. Haiku 4.5 on Bedrock): even though
        // the write-vs-read economics alone would recommend a marker, none
        // should be emitted because Bedrock would silently skip the write.
        let haiku_floor = CacheMarkerPlanner::new(provider.clone(), 4096);
        let markers = haiku_floor.plan_markers(500, 300, 100, 0, 0, 15);
        assert!(
            markers.is_empty(),
            "a ~900 token prefix must not be marked under a 4096 floor: {:?}",
            markers
        );

        // The exact same request shape, but under a 1024-token floor (e.g.
        // Sonnet 4.6 on Bedrock): the system+tools position (800) still
        // doesn't clear it, so it's still correctly excluded — this isn't
        // "any floor accepts everything", it's a real per-position check.
        let sonnet_floor = CacheMarkerPlanner::new(provider.clone(), 1024);
        let markers = sonnet_floor.plan_markers(500, 300, 100, 0, 0, 15);
        assert!(
            markers.is_empty(),
            "800 tokens must not clear a 1024 floor either: {:?}",
            markers
        );

        // Push the system+tools position (800) just over a genuinely low
        // floor and it should be marked.
        let low_floor = CacheMarkerPlanner::new(provider, 700);
        let markers = low_floor.plan_markers(500, 300, 100, 0, 0, 15);
        assert!(
            markers.iter().any(|m| m.position_tokens == 800),
            "800 tokens should clear a 700-token floor: {:?}",
            markers
        );
    }

    #[test]
    fn test_turn_timing_tracker() {
        let mut tracker = TurnTimingTracker::new();

        // Simulate: first turn
        tracker.record_request_sent(0);
        tracker.record_response_received(0);

        // Wait and second turn
        // (In test, timestamps are very close but that's fine)
        let idle = tracker.record_request_sent(1);
        assert!(idle <= 10); // very small since no real delay in test

        tracker.record_response_received(1);
        assert_eq!(tracker.timings.len(), 2);
    }

    #[test]
    fn test_cache_cold_detection() {
        let mut tracker = TurnTimingTracker::new();

        // Manually set a timing with high idle
        tracker.timings.push(TurnTiming {
            turn_id: 0,
            request_sent_at_ms: 1000000,
            response_received_at_ms: 1001000,
            idle_before_ms: 400_000, // 400 seconds idle
            cache_predicted_cold: false,
        });

        let provider = explicit_300s();
        assert!(tracker.is_cache_cold(&provider)); // 400s > 300s TTL
    }
}
