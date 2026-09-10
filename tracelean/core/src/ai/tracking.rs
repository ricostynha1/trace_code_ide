//! Token usage tracking and cost estimation.

use serde::{Deserialize, Serialize};

/// Token counts from a single AI call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens used in "thinking" / chain-of-thought (if provider reports separately)
    pub thinking_tokens: u32,
    /// Tokens served from cache (subset of input_tokens)
    pub cached_tokens: u32,
    /// Tokens newly written to cache this turn (subset of input_tokens,
    /// disjoint from cached_tokens). Bedrock/Anthropic bill these at a write
    /// premium (cache_write_multiplier), not the plain input rate.
    #[serde(default)]
    pub cache_write_tokens: u32,
}

/// Estimated cost for a single interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Total estimated cost in USD
    pub total_usd: f64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cached_savings: f64,
    /// Cost of tokens newly written to cache this turn (0 if none written).
    #[serde(default)]
    pub write_cost: f64,
}

impl TokenUsage {
    /// Compute cost given per-million-token prices and the provider's cache
    /// write premium multiplier (e.g. 1.25 = 25% surcharge over the plain
    /// input rate; 0.0 for providers with no explicit write cost).
    pub fn estimate_cost(
        &self,
        input_cost_per_m: f64,
        output_cost_per_m: f64,
        cached_input_cost_per_m: f64,
        cache_write_multiplier: f64,
    ) -> CostEstimate {
        let non_cached_input = self
            .input_tokens
            .saturating_sub(self.cached_tokens)
            .saturating_sub(self.cache_write_tokens);
        let input_cost = (non_cached_input as f64 / 1_000_000.0) * input_cost_per_m;
        let cached_cost = (self.cached_tokens as f64 / 1_000_000.0) * cached_input_cost_per_m;
        let write_cost = (self.cache_write_tokens as f64 / 1_000_000.0)
            * input_cost_per_m
            * cache_write_multiplier;
        let output_cost = (self.output_tokens as f64 / 1_000_000.0) * output_cost_per_m;
        let full_input_would_cost = (self.input_tokens as f64 / 1_000_000.0) * input_cost_per_m;
        // Not clamped at 0: a write-heavy, read-light turn can legitimately
        // cost MORE than an uncached call would have (write premium with no
        // offsetting read discount yet) — that net loss is exactly what the
        // write-visibility fix exists to surface, not hide.
        let cached_savings = full_input_would_cost - input_cost - cached_cost - write_cost;

        CostEstimate {
            total_usd: input_cost + cached_cost + write_cost + output_cost,
            input_cost: input_cost + cached_cost,
            output_cost,
            cached_savings,
            write_cost,
        }
    }
}

/// Cumulative session statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_requests: u32,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_thinking_tokens: u64,
    pub total_cached_tokens: u64,
    /// Tokens newly written to cache (subset of total_input_tokens, disjoint
    /// from total_cached_tokens) — billed at the provider's write premium.
    #[serde(default)]
    pub total_cache_write_tokens: u64,
    pub total_cost_usd: f64,
    /// Output-token share of total_cost_usd (lets the UI show output-cost %).
    #[serde(default)]
    pub total_output_cost_usd: f64,
    /// Estimated spend from *external* agent sessions (Claude Code running
    /// in a tracelean sandbox — `sandbox::cost`), tracked separately and
    /// deliberately NOT folded into `total_cost_usd`: it's a modeled
    /// estimate against a session tracelean never called and never paid
    /// for directly (which may not even be metered per-token, e.g. a
    /// subscription plan), and blending it in would let external-agent
    /// usage silently trip the built-in agent's own spend cap.
    #[serde(default)]
    pub external_requests: u32,
    #[serde(default)]
    pub external_input_tokens: u64,
    #[serde(default)]
    pub external_output_tokens: u64,
    #[serde(default)]
    pub external_estimated_cost_usd: f64,
}

impl SessionStats {
    pub fn record(&mut self, usage: &TokenUsage, cost: &CostEstimate) {
        self.total_requests += 1;
        self.total_input_tokens += usage.input_tokens as u64;
        self.total_output_tokens += usage.output_tokens as u64;
        self.total_thinking_tokens += usage.thinking_tokens as u64;
        self.total_cached_tokens += usage.cached_tokens as u64;
        self.total_cache_write_tokens += usage.cache_write_tokens as u64;
        self.total_cost_usd += cost.total_usd;
        self.total_output_cost_usd += cost.output_cost;
    }

    /// Record one external-agent turn. See `external_estimated_cost_usd`'s
    /// doc comment for why this is a separate counter, not folded into
    /// `record`'s totals.
    pub fn record_external(&mut self, usage: &TokenUsage, cost: &CostEstimate) {
        self.external_requests += 1;
        self.external_input_tokens += usage.input_tokens as u64;
        self.external_output_tokens += usage.output_tokens as u64;
        self.external_estimated_cost_usd += cost.total_usd;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: external-agent (sandboxed Claude Code) spend must never
    /// land in `total_cost_usd` — that field gates the built-in agent's own
    /// spend cap, and external usage is an estimate against a session
    /// tracelean neither made nor necessarily pays per-token for.
    #[test]
    fn record_external_never_touches_the_built_in_totals() {
        let mut stats = SessionStats::default();
        let usage = TokenUsage { input_tokens: 1000, output_tokens: 200, thinking_tokens: 0, cached_tokens: 0, cache_write_tokens: 0 };
        let cost = CostEstimate { total_usd: 5.0, input_cost: 3.0, output_cost: 2.0, cached_savings: 0.0, write_cost: 0.0 };

        stats.record_external(&usage, &cost);

        assert_eq!(stats.total_cost_usd, 0.0, "external spend must not count toward the spend-cap total");
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_input_tokens, 0);
        assert_eq!(stats.external_requests, 1);
        assert_eq!(stats.external_input_tokens, 1000);
        assert_eq!(stats.external_output_tokens, 200);
        assert_eq!(stats.external_estimated_cost_usd, 5.0);
    }

    #[test]
    fn write_tokens_are_billed_at_the_write_premium_not_the_plain_rate() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            thinking_tokens: 0,
            cached_tokens: 0,
            cache_write_tokens: 1_000_000,
        };
        // $3/M input, 1.25x write multiplier (Anthropic's documented premium).
        let cost = usage.estimate_cost(3.0, 15.0, 0.3, 1.25);
        assert_eq!(cost.write_cost, 3.75);
        assert_eq!(cost.total_usd, 3.75);
        // A pure-write turn costs MORE than an uncached call would have —
        // must show as a real net loss, not be clamped to 0.
        assert!(cost.cached_savings < 0.0, "expected a net loss, got {}", cost.cached_savings);
    }

    #[test]
    fn read_and_write_tokens_are_mutually_exclusive_subsets_of_input_tokens() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 0,
            thinking_tokens: 0,
            cached_tokens: 40,
            cache_write_tokens: 30,
        };
        let cost = usage.estimate_cost(10.0, 0.0, 1.0, 1.25);
        // non-cached, non-write remainder: 100 - 40 - 30 = 30 tokens @ $10/M
        let expected_plain_input = 30.0 / 1_000_000.0 * 10.0;
        let expected_cached = 40.0 / 1_000_000.0 * 1.0;
        let expected_write = 30.0 / 1_000_000.0 * 10.0 * 1.25;
        assert!((cost.total_usd - (expected_plain_input + expected_cached + expected_write)).abs() < 1e-12);
    }

    #[test]
    fn zero_write_multiplier_bills_writes_at_plain_input_rate() {
        let usage = TokenUsage {
            input_tokens: 100,
            output_tokens: 0,
            thinking_tokens: 0,
            cached_tokens: 0,
            cache_write_tokens: 100,
        };
        let cost = usage.estimate_cost(10.0, 0.0, 1.0, 0.0);
        assert_eq!(cost.write_cost, 0.0);
        assert_eq!(cost.total_usd, 0.0);
    }
}
