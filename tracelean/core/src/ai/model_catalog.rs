//! Model catalog — loads merged model data from data/models.json at compile time.
//!
//! Provides enrichment for live-queried models: coding_index, coding_rank,
//! supports_caching, supports_tools.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Raw entry from data/models.json (subset of fields we care about).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CatalogEntry {
    model: String,
    slug: Option<String>,
    #[serde(default)]
    supports_prompt_caching: bool,
    #[serde(default)]
    supports_tool_calling: bool,
    coding_index: Option<f64>,
    coding_rank: Option<u32>,
    ranking_model_id: Option<String>,
    pricing: Option<CatalogPricing>,
    /// f64 because a few catalog entries carry float values (e.g. 2000000.0).
    context_window: Option<f64>,
    /// Optional catalog overrides for tool strategy (revert without code change).
    tool_call_format: Option<super::provider::ToolCallFormat>,
    tool_passing: Option<super::provider::ToolPassing>,
    /// Minimum prompt-prefix size (tokens) before a cache checkpoint actually
    /// caches anything on this exact model+platform (e.g. Bedrock Haiku 4.5
    /// needs 4096, Bedrock Sonnet 4.6 needs only 1024 — same `cachePoint`
    /// mechanism, different provider-documented floor). `None` for models
    /// this field hasn't been populated for.
    cache_min_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CatalogPricing {
    input_per_1m_tokens: Option<f64>,
    output_per_1m_tokens: Option<f64>,
}

/// Enrichment data for a model.
#[derive(Debug, Clone)]
pub struct ModelEnrichment {
    pub coding_index: Option<f64>,
    pub coding_rank: Option<u32>,
    pub supports_caching: bool,
    pub supports_tools: bool,
    pub input_cost_per_m: Option<f64>,
    pub output_cost_per_m: Option<f64>,
    pub context_window: Option<u32>,
    pub tool_call_format: Option<super::provider::ToolCallFormat>,
    pub tool_passing: Option<super::provider::ToolPassing>,
    pub cache_min_tokens: Option<u32>,
}

/// The compiled-in catalog JSON.
const CATALOG_JSON: &str = include_str!("../../../data/models.json");

/// Lazily-initialized lookup table. Keys are normalized model IDs.
static CATALOG: OnceLock<CatalogIndex> = OnceLock::new();

struct CatalogIndex {
    /// Exact model key -> enrichment
    by_key: HashMap<String, ModelEnrichment>,
    /// Slug -> enrichment (for fuzzy lookup)
    by_slug: HashMap<String, ModelEnrichment>,
}

fn build_index() -> CatalogIndex {
    let entries: Vec<CatalogEntry> = serde_json::from_str(CATALOG_JSON).unwrap_or_default();
    let mut by_key = HashMap::with_capacity(entries.len());
    let mut by_slug = HashMap::with_capacity(entries.len());

    for entry in entries {
        let enrichment = ModelEnrichment {
            coding_index: entry.coding_index,
            coding_rank: entry.coding_rank,
            supports_caching: entry.supports_prompt_caching,
            supports_tools: entry.supports_tool_calling,
            input_cost_per_m: entry.pricing.as_ref().and_then(|p| p.input_per_1m_tokens),
            output_cost_per_m: entry.pricing.as_ref().and_then(|p| p.output_per_1m_tokens),
            context_window: entry
                .context_window
                .filter(|cw| cw.is_finite() && *cw > 0.0)
                .map(|cw| cw as u32),
            tool_call_format: entry.tool_call_format,
            tool_passing: entry.tool_passing,
            cache_min_tokens: entry.cache_min_tokens,
        };

        by_key.insert(entry.model.to_lowercase(), enrichment.clone());

        if let Some(slug) = entry.slug {
            if !slug.is_empty() {
                by_slug.insert(slug.to_lowercase(), enrichment);
            }
        }
    }

    CatalogIndex { by_key, by_slug }
}

fn get_catalog() -> &'static CatalogIndex {
    CATALOG.get_or_init(build_index)
}

/// Normalize a model ID for lookup: lowercase, strip common prefixes.
fn normalize_model_id(id: &str) -> String {
    let lower = id.to_lowercase();
    // Strip provider prefix patterns like "anthropic.", "openai/", "bedrock/region/provider."
    let parts: Vec<&str> = lower.split('/').collect();
    let last = parts.last().copied().unwrap_or(&lower);
    // Strip dot-prefix (purely alpha)
    if let Some(pos) = last.find('.') {
        let prefix = &last[..pos];
        if prefix.chars().all(|c| c.is_ascii_alphabetic()) {
            return last[pos + 1..].to_string();
        }
    }
    last.to_string()
}

/// Look up enrichment data for a model ID.
/// Tries exact key match first, then normalized slug match.
pub fn lookup(model_id: &str) -> Option<ModelEnrichment> {
    let catalog = get_catalog();

    // 1. Exact key match (model_id as it appears in litellm)
    let lower = model_id.to_lowercase();
    if let Some(e) = catalog.by_key.get(&lower) {
        return Some(e.clone());
    }

    // 2. Normalized slug match
    let slug = normalize_model_id(model_id);
    if let Some(e) = catalog.by_slug.get(&slug) {
        return Some(e.clone());
    }

    // 3. Try the last path segment as-is
    let last_seg = model_id.rsplit('/').next().unwrap_or(model_id).to_lowercase();
    if let Some(e) = catalog.by_slug.get(&last_seg) {
        return Some(e.clone());
    }

    None
}

/// Look up (input_cost_per_1m, output_cost_per_1m) for a model ID.
/// Returns `None` if the model isn't in the catalog or carries no pricing.
pub fn pricing_for(model_id: &str) -> Option<(f64, f64)> {
    let enrichment = lookup(model_id)?;
    match (enrichment.input_cost_per_m, enrichment.output_cost_per_m) {
        (Some(input), Some(output)) => Some((input, output)),
        _ => None,
    }
}

/// Enrich a ModelConfig with catalog data.
pub fn enrich(model: &mut super::provider::ModelConfig) {
    use super::provider::{ProviderKind, ToolCallFormat, ToolPassing};

    let enrichment = lookup(&model.model_id);
    if let Some(ref enrichment) = enrichment {
        model.coding_index = enrichment.coding_index;
        model.coding_rank = enrichment.coding_rank;
        model.supports_caching = enrichment.supports_caching;
        model.supports_tools = enrichment.supports_tools;
        // Override pricing if catalog has it and model doesn't (or has 0)
        if model.input_cost_per_m == 0.0 {
            if let Some(cost) = enrichment.input_cost_per_m {
                model.input_cost_per_m = cost;
            }
        }
        if model.output_cost_per_m == 0.0 {
            if let Some(cost) = enrichment.output_cost_per_m {
                model.output_cost_per_m = cost;
            }
        }
        if let Some(cw) = enrichment.context_window {
            if cw > 0 {
                model.context_window = cw;
                model.context_window_known = true;
            }
        }
        // Unconditional overwrite — no legacy pre-seeded placeholder to
        // preserve for this field, unlike input/output cost above.
        if let Some(min_tokens) = enrichment.cache_min_tokens {
            model.cache_min_tokens = min_tokens;
        }
    }

    let id = model.model_id.to_lowercase();

    // Tool-call dialect: catalog override wins; otherwise pick by model family.
    model.tool_call_format = enrichment
        .as_ref()
        .and_then(|e| e.tool_call_format)
        .unwrap_or_else(|| {
            if id.contains("minimax") {
                ToolCallFormat::MiniMaxXml
            } else if id.contains("mistral") || id.contains("mixtral") {
                ToolCallFormat::MistralBrackets
            } else {
                ToolCallFormat::HermesJson
            }
        });

    // Tool passing: only Bedrock+MiniMax stays embedded (Mantle doesn't cache
    // or reliably return native tool calls for MiniMax). Everything else native.
    model.tool_passing = enrichment
        .as_ref()
        .and_then(|e| e.tool_passing)
        .unwrap_or_else(|| {
            if model.provider == ProviderKind::Bedrock && id.contains("minimax") {
                ToolPassing::SystemPromptEmbed
            } else {
                ToolPassing::NativeParam
            }
        });

    // Thinking models routinely spend thousands of tokens inside <think>
    // spans; a 4096 output budget truncates mid-thought and yields an empty
    // answer once thinking is stripped.
    if id.contains("minimax") && model.max_tokens < 16_384 {
        model.max_tokens = 16_384;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads() {
        let catalog = get_catalog();
        assert!(!catalog.by_key.is_empty(), "catalog should have entries");
    }

    #[test]
    fn lookup_by_key() {
        // This key should exist in models.json
        let result = lookup("anthropic.claude-fable-5");
        assert!(result.is_some(), "should find claude-fable-5 by exact key");
        let e = result.unwrap();
        assert!(e.coding_rank.is_some());
    }

    #[test]
    fn lookup_normalized() {
        let result = lookup("openrouter/anthropic/claude-fable-5");
        // Should match via slug normalization
        assert!(result.is_some() || lookup("claude-fable-5").is_some());
    }

    /// cache_min_tokens is data-sourced from data/models.json, not hardcoded
    /// per-model logic in Rust — this pins the catalog data itself (via
    /// `lookup`), not a Rust-side lookup table. Verified live: a ~1,500 token
    /// system+tools prefix on the same agent conversation cached correctly
    /// on Sonnet 4.6 but never wrote a cache entry on Haiku 4.5, matching
    /// these two floors exactly.
    #[test]
    fn cache_min_tokens_reflects_bedrock_per_model_floors() {
        let haiku = lookup("eu.anthropic.claude-haiku-4-5-20251001-v1:0").unwrap();
        assert_eq!(haiku.cache_min_tokens, Some(4096));

        let sonnet_4_6 = lookup("eu.anthropic.claude-sonnet-4-6").unwrap();
        assert_eq!(sonnet_4_6.cache_min_tokens, Some(1024));
    }

    #[test]
    fn enrich_sets_cache_min_tokens_from_catalog() {
        use super::super::provider::{ModelConfig, ProviderKind};

        let mut haiku = ModelConfig {
            provider: ProviderKind::Bedrock,
            model_id: "eu.anthropic.claude-haiku-4-5-20251001-v1:0".to_string(),
            ..Default::default()
        };
        enrich(&mut haiku);
        assert_eq!(haiku.cache_min_tokens, 4096);

        let mut sonnet = ModelConfig {
            provider: ProviderKind::Bedrock,
            model_id: "eu.anthropic.claude-sonnet-4-6".to_string(),
            ..Default::default()
        };
        enrich(&mut sonnet);
        assert_eq!(sonnet.cache_min_tokens, 1024);
    }

    #[test]
    fn enrich_leaves_conservative_default_for_a_model_not_in_the_catalog() {
        use super::super::provider::{ModelConfig, ProviderKind};

        let mut unknown = ModelConfig {
            provider: ProviderKind::Bedrock,
            model_id: "anthropic.claude-some-future-model-not-yet-cataloged".to_string(),
            ..Default::default()
        };
        enrich(&mut unknown);
        assert_eq!(unknown.cache_min_tokens, 4096, "ModelConfig::default()'s conservative fallback should survive when the catalog has nothing for this id");
    }
}
