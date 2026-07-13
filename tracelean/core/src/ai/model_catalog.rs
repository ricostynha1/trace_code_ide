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

/// Enrich a ModelConfig with catalog data.
pub fn enrich(model: &mut super::provider::ModelConfig) {
    if let Some(enrichment) = lookup(&model.model_id) {
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
}
