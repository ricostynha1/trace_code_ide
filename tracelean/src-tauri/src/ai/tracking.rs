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
}

/// Estimated cost for a single interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Total estimated cost in USD
    pub total_usd: f64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cached_savings: f64,
}

impl TokenUsage {
    /// Compute cost given per-million-token prices.
    pub fn estimate_cost(
        &self,
        input_cost_per_m: f64,
        output_cost_per_m: f64,
        cached_input_cost_per_m: f64,
    ) -> CostEstimate {
        let non_cached_input = self.input_tokens.saturating_sub(self.cached_tokens);
        let input_cost = (non_cached_input as f64 / 1_000_000.0) * input_cost_per_m;
        let cached_cost = (self.cached_tokens as f64 / 1_000_000.0) * cached_input_cost_per_m;
        let output_cost = (self.output_tokens as f64 / 1_000_000.0) * output_cost_per_m;
        let full_input_would_cost = (self.input_tokens as f64 / 1_000_000.0) * input_cost_per_m;
        let cached_savings = full_input_would_cost - input_cost - cached_cost;

        CostEstimate {
            total_usd: input_cost + cached_cost + output_cost,
            input_cost: input_cost + cached_cost,
            output_cost,
            cached_savings: cached_savings.max(0.0),
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
    pub total_cost_usd: f64,
}

impl SessionStats {
    pub fn record(&mut self, usage: &TokenUsage, cost: &CostEstimate) {
        self.total_requests += 1;
        self.total_input_tokens += usage.input_tokens as u64;
        self.total_output_tokens += usage.output_tokens as u64;
        self.total_thinking_tokens += usage.thinking_tokens as u64;
        self.total_cached_tokens += usage.cached_tokens as u64;
        self.total_cost_usd += cost.total_usd;
    }
}
