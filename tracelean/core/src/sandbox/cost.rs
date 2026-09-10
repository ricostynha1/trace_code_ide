//! Estimated USD cost for an external Claude Code session's own reported
//! token usage (`sandbox::transcript::TranscriptEvent::Usage`).
//!
//! Claude Code isn't a provider tracelean called itself, so
//! `ai::provider_cache`'s per-request cache-config machinery (built around
//! a full `ModelConfig`) doesn't apply here. This prices directly against
//! Anthropic's documented, model-independent cache ratios — cache reads at
//! 0.1x the input rate, cache writes at 1.25x (5-minute TTL) or 2x
//! (1-hour TTL) — using whatever base input/output rate `ai::model_catalog`
//! has for the model the transcript itself reports.
//!
//! This is always an *estimate*: it has no visibility into whether the
//! session is actually billed per-token (an API key) or is running under a
//! subscription plan where "cost" isn't a real invoice line. Callers should
//! label it as such.

use crate::ai::model_catalog::pricing_for;
use crate::ai::tracking::{CostEstimate, TokenUsage};

pub const CACHE_READ_DISCOUNT: f64 = 0.1;
pub const CACHE_WRITE_5M_MULTIPLIER: f64 = 1.25;
pub const CACHE_WRITE_1H_MULTIPLIER: f64 = 2.0;

/// Price one Claude Code `Usage` event. Returns `None` if the reported
/// model isn't in the pricing catalog (e.g. a brand-new model not yet added
/// to `data/models.json` — see `ai::model_catalog`).
///
/// `input_tokens` is the *fresh* (non-cached) count, matching Anthropic's
/// own usage object — cache reads/writes are separate, additional pools,
/// not subsets of it. The returned `TokenUsage.input_tokens` is the total
/// across all three, matching this codebase's own convention (see its
/// doc comment: cached/write tokens are a subset of `input_tokens`) so it
/// displays sensibly next to built-in-agent entries in the Log tab.
pub fn estimate_cost(
    model_id: &str,
    fresh_input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_5m_tokens: u64,
    cache_write_1h_tokens: u64,
) -> Option<(TokenUsage, CostEstimate)> {
    let (input_rate, output_rate) = pricing_for(model_id)?;

    let cache_write_tokens = cache_write_5m_tokens + cache_write_1h_tokens;
    let total_input_tokens = fresh_input_tokens + cache_read_tokens + cache_write_tokens;

    let usage = TokenUsage {
        input_tokens: total_input_tokens.min(u32::MAX as u64) as u32,
        output_tokens: output_tokens.min(u32::MAX as u64) as u32,
        thinking_tokens: 0,
        cached_tokens: cache_read_tokens.min(u32::MAX as u64) as u32,
        cache_write_tokens: cache_write_tokens.min(u32::MAX as u64) as u32,
    };

    let input_cost = (fresh_input_tokens as f64 / 1_000_000.0) * input_rate;
    let read_cost = (cache_read_tokens as f64 / 1_000_000.0) * input_rate * CACHE_READ_DISCOUNT;
    let write_5m_cost = (cache_write_5m_tokens as f64 / 1_000_000.0) * input_rate * CACHE_WRITE_5M_MULTIPLIER;
    let write_1h_cost = (cache_write_1h_tokens as f64 / 1_000_000.0) * input_rate * CACHE_WRITE_1H_MULTIPLIER;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * output_rate;
    let write_cost = write_5m_cost + write_1h_cost;

    let cost = CostEstimate {
        total_usd: input_cost + read_cost + write_cost + output_cost,
        input_cost,
        output_cost,
        // No "would have cost uncached" baseline is meaningful here — the
        // built-in agent's cached_savings compares against replaying the
        // same prefix uncached, which doesn't apply to a session tracelean
        // never sent the request for.
        cached_savings: 0.0,
        write_cost,
    };

    Some((usage, cost))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_a_real_usage_shape_for_claude_opus_5() {
        // The exact shape captured from a real transcript earlier in this
        // feature's development (input_tokens=2, cache_read=18469,
        // cache_creation all under the 1h TTL).
        let (usage, cost) = estimate_cost("claude-opus-5", 2, 280, 18469, 0, 8842).unwrap();
        assert_eq!(usage.input_tokens, 2 + 18469 + 8842);
        assert_eq!(usage.cached_tokens, 18469);
        assert_eq!(usage.cache_write_tokens, 8842);

        // input $5/1M, output $25/1M (data/models.json).
        let expected_input = 2.0 / 1_000_000.0 * 5.0;
        let expected_read = 18469.0 / 1_000_000.0 * 5.0 * CACHE_READ_DISCOUNT;
        let expected_write = 8842.0 / 1_000_000.0 * 5.0 * CACHE_WRITE_1H_MULTIPLIER;
        let expected_output = 280.0 / 1_000_000.0 * 25.0;
        let expected_total = expected_input + expected_read + expected_write + expected_output;
        assert!((cost.total_usd - expected_total).abs() < 1e-12);
    }

    #[test]
    fn five_minute_and_one_hour_writes_are_priced_differently() {
        let (_, cheap) = estimate_cost("claude-opus-5", 0, 0, 0, 1_000_000, 0).unwrap();
        let (_, pricey) = estimate_cost("claude-opus-5", 0, 0, 0, 0, 1_000_000).unwrap();
        assert!(pricey.total_usd > cheap.total_usd, "1h TTL writes must cost more than 5m TTL writes");
        assert!((cheap.total_usd - 5.0 * CACHE_WRITE_5M_MULTIPLIER).abs() < 1e-9);
        assert!((pricey.total_usd - 5.0 * CACHE_WRITE_1H_MULTIPLIER).abs() < 1e-9);
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(estimate_cost("some-model-not-in-the-catalog", 1, 1, 0, 0, 0).is_none());
    }
}
