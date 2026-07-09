//! Bedrock model pricing (USD per 1M tokens).
//!
//! Source: pricepertoken.com/endpoints/bedrock (US East N. Virginia)
//! Last updated: 2026-07-09
//!
//! Prices vary by region; these are base US-East prices.
//! Update this file when AWS changes pricing.

/// Returns (input_cost_per_1M, output_cost_per_1M) for a bedrock-mantle model ID.
/// Returns (0.0, 0.0) for unknown models.
pub fn bedrock_pricing(model_id: &str) -> (f64, f64) {
    match model_id {
        // ─── Anthropic ───────────────────────────────────────────────────
        "anthropic.claude-sonnet-5" => (3.00, 15.00),
        "anthropic.claude-opus-4-7" | "anthropic.claude-opus-4-8" => (15.00, 75.00),
        "anthropic.claude-haiku-4-5" => (0.80, 4.00),
        "anthropic.claude-fable-5" => (8.00, 40.00),

        // ─── OpenAI ─────────────────────────────────────────────────────
        "openai.gpt-5.4" | "openai.gpt-5.4-2026-03-05" => (3.00, 15.00),
        "openai.gpt-5.5" | "openai.gpt-5.5-2026-04-23" => (3.00, 15.00),
        "openai.gpt-oss-120b" | "openai.gpt-oss-safeguard-120b" => (0.15, 0.60),
        "openai.gpt-oss-20b" => (0.09, 0.39),
        "openai.gpt-oss-safeguard-20b" => (0.07, 0.20),

        // ─── Qwen ───────────────────────────────────────────────────────
        "qwen.qwen3-32b" => (0.20, 0.78),
        "qwen.qwen3-235b-a22b-2507" => (0.11, 0.45),
        "qwen.qwen3-coder-30b-a3b-instruct" => (0.20, 0.78),
        "qwen.qwen3-coder-480b-a35b-instruct" => (0.80, 3.20),
        "qwen.qwen3-coder-next" => (0.80, 3.20),
        "qwen.qwen3-next-80b-a3b-instruct" => (0.15, 1.20),
        "qwen.qwen3-vl-235b-a22b-instruct" => (0.11, 0.45),

        // ─── Mistral ────────────────────────────────────────────────────
        "mistral.mistral-large-3-675b-instruct" => (0.50, 1.50),
        "mistral.devstral-2-123b" => (0.40, 2.00),
        "mistral.magistral-small-2509" => (0.50, 1.50),
        "mistral.ministral-3-14b-instruct" => (0.20, 0.20),
        "mistral.ministral-3-8b-instruct" => (0.15, 0.15),
        "mistral.ministral-3-3b-instruct" => (0.10, 0.10),
        "mistral.voxtral-mini-3b-2507" | "mistral.voxtral-small-24b-2507" => (0.15, 0.15),

        // ─── Google Gemma ───────────────────────────────────────────────
        "google.gemma-3-4b-it" => (0.04, 0.08),
        "google.gemma-3-12b-it" => (0.09, 0.29),
        "google.gemma-3-27b-it" => (0.23, 0.38),
        "google.gemma-4-26b-a4b" => (0.13, 0.40),
        "google.gemma-4-31b" => (0.14, 0.40),
        "google.gemma-4-e2b" => (0.04, 0.08),

        // ─── DeepSeek ───────────────────────────────────────────────────
        "deepseek.v3.1" => (0.30, 0.87),
        "deepseek.v3.2" => (0.62, 1.85),

        // ─── Nvidia ─────────────────────────────────────────────────────
        "nvidia.nemotron-super-3-120b" => (0.15, 0.65),
        "nvidia.nemotron-nano-3-30b" => (0.06, 0.24),
        "nvidia.nemotron-nano-12b-v2" | "nvidia.nemotron-nano-9b-v2" => (0.06, 0.23),

        // ─── MiniMax ────────────────────────────────────────────────────
        "minimax.minimax-m2" | "minimax.minimax-m2.1" | "minimax.minimax-m2.5" => (0.30, 1.20),

        // ─── Moonshot ───────────────────────────────────────────────────
        "moonshotai.kimi-k2-thinking" => (0.60, 2.50),
        "moonshotai.kimi-k2.5" => (0.60, 3.00),

        // ─── Z AI ───────────────────────────────────────────────────────
        "zai.glm-4.6" | "zai.glm-4.7" => (0.60, 2.20),
        "zai.glm-4.7-flash" => (0.07, 0.40),
        "zai.glm-5" => (1.00, 3.20),

        // ─── xAI ────────────────────────────────────────────────────────
        "xai.grok-4.3" => (3.00, 15.00),

        // ─── Writer ─────────────────────────────────────────────────────
        "writer.palmyra-vision-7b" => (0.10, 0.10),

        // ─── Unknown ────────────────────────────────────────────────────
        _ => (0.0, 0.0),
    }
}
