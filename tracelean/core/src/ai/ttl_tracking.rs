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
}

impl CacheMarkerPlanner {
    pub fn new(provider: ProviderCacheConfig) -> Self {
        let max_markers = match provider.cache_mode {
            CacheMode::Explicit => 4,
            CacheMode::Automatic => 0, // no markers needed
        };
        Self { provider, max_markers }
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
        let savings1 = pos1 as f64 * d * n_expected as f64 - pos1 as f64 * w;
        if savings1 > 0.0 {
            candidates.push(CacheMarker {
                position_tokens: pos1,
                after_block: CacheBlock::SystemPrompt,
                estimated_savings: savings1,
            });
        }

        // Candidate 2: After system prompt + static tools
        let pos2 = system_prompt_tokens + static_tools_tokens;
        let savings2 = pos2 as f64 * d * n_expected as f64 - pos2 as f64 * w;
        if savings2 > savings1.max(0.0) {
            candidates.push(CacheMarker {
                position_tokens: pos2,
                after_block: CacheBlock::StaticTools,
                estimated_savings: savings2,
            });
        }

        // Candidate 3: After all tools (if dynamic tools are stable)
        if dynamic_tools_tokens > 0 {
            let pos3 = pos2 + dynamic_tools_tokens;
            let savings3 = pos3 as f64 * d * n_expected as f64 - pos3 as f64 * w;
            if savings3 > savings2.max(0.0) {
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
            let savings4 = pos4 as f64 * d * n_expected as f64 - pos4 as f64 * w;
            if savings4 > 0.0 {
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
        let planner = CacheMarkerPlanner::new(provider);

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
    fn test_marker_planner_automatic() {
        let provider = automatic();
        let planner = CacheMarkerPlanner::new(provider);

        let markers = planner.plan_markers(500, 300, 100, 2000, 800, 4);
        assert!(markers.is_empty()); // no markers for automatic providers
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
