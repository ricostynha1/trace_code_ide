//! Provider cache configuration — loads and exposes provider-specific cache parameters
//! to the retention/cost engine.
//!
//! Each provider has: cache_mode (automatic|explicit), read discount, write multiplier,
//! TTL, and whether it requires explicit cache markers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Cache mode — automatic (prefix matching) or explicit (requires markers).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    Automatic,
    Explicit,
}

/// Provider-specific cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCacheConfig {
    pub cache_mode: CacheMode,
    /// Fraction of input cost charged for cached reads (e.g. 0.1 = 10% of normal)
    pub cache_read_discount: f64,
    /// Multiplier on write cost for explicit cache (e.g. 1.25 = 25% surcharge). 0.0 for automatic.
    pub cache_write_multiplier: f64,
    /// TTL in seconds for explicit cache. None for automatic (managed by provider).
    pub ttl_seconds: Option<u64>,
    /// Whether provider requires explicit cache markers in the request.
    pub requires_markers: bool,
    /// Human-readable notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

impl ProviderCacheConfig {
    /// Cost per token for cached reads: base_cost * cache_read_discount
    pub fn cached_read_cost(&self, base_cost_per_token: f64) -> f64 {
        base_cost_per_token * self.cache_read_discount
    }

    /// Cost per token for cache writes (explicit only): base_cost * cache_write_multiplier
    pub fn cache_write_cost(&self, base_cost_per_token: f64) -> f64 {
        base_cost_per_token * self.cache_write_multiplier
    }

    /// Whether cache is likely cold given idle time.
    /// Returns true if idle_secs > ttl (meaning penalty should be 0).
    pub fn cache_is_cold(&self, idle_secs: u64) -> bool {
        match self.ttl_seconds {
            Some(ttl) => idle_secs > ttl,
            None => false, // automatic caches don't have user-visible TTL
        }
    }
}

/// Top-level config file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderCacheFile {
    #[serde(default)]
    _comment: Option<String>,
    providers: HashMap<String, ProviderCacheConfig>,
}

/// Loaded provider cache configuration set.
#[derive(Debug, Clone)]
pub struct ProviderCacheRegistry {
    providers: HashMap<String, ProviderCacheConfig>,
}

impl ProviderCacheRegistry {
    /// Load from a JSON file.
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read provider_cache.json: {}", e))?;
        let file: ProviderCacheFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse provider_cache.json: {}", e))?;
        Ok(Self { providers: file.providers })
    }

    /// Load from default data dir.
    pub fn load_default(data_dir: &Path) -> Result<Self, String> {
        Self::load_from_file(&data_dir.join("provider_cache.json"))
    }

    /// Get config for a specific provider key.
    pub fn get(&self, provider_key: &str) -> Option<&ProviderCacheConfig> {
        self.providers.get(provider_key)
    }

    /// Get config, falling back to a default automatic config if not found.
    pub fn get_or_default(&self, provider_key: &str) -> ProviderCacheConfig {
        self.providers.get(provider_key).cloned().unwrap_or(ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.5,
            cache_write_multiplier: 0.0,
            ttl_seconds: None,
            requires_markers: false,
            notes: None,
        })
    }

    /// List all provider keys.
    pub fn provider_keys(&self) -> Vec<&String> {
        self.providers.keys().collect()
    }
}

impl ProviderCacheConfig {
    /// Build cache config directly from a models.json entry.
    /// The merge script already attached these fields — no guessing needed.
    pub fn from_model_entry(entry: &serde_json::Value) -> Self {
        let cache_mode = match entry.get("cache_mode").and_then(|v| v.as_str()) {
            Some("explicit") => CacheMode::Explicit,
            _ => CacheMode::Automatic,
        };
        let cache_read_discount = entry.get("cache_read_discount")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.5);
        let cache_write_multiplier = entry.get("cache_write_multiplier")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let ttl_seconds = entry.get("cache_ttl_seconds")
            .and_then(|v| v.as_u64());
        let requires_markers = entry.get("cache_requires_markers")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Self {
            cache_mode,
            cache_read_discount,
            cache_write_multiplier,
            ttl_seconds,
            requires_markers,
            notes: None,
        }
    }

    /// Default fallback when model has no cache fields.
    pub fn default_automatic() -> Self {
        Self {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.5,
            cache_write_multiplier: 0.0,
            ttl_seconds: None,
            requires_markers: false,
            notes: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_config() -> &'static str {
        r#"{
  "providers": {
    "openai": {
      "cache_mode": "automatic",
      "cache_read_discount": 0.5,
      "cache_write_multiplier": 0.0,
      "ttl_seconds": null,
      "requires_markers": false
    },
    "anthropic_5min": {
      "cache_mode": "explicit",
      "cache_read_discount": 0.1,
      "cache_write_multiplier": 1.25,
      "ttl_seconds": 300,
      "requires_markers": true
    }
  }
}"#
    }

    #[test]
    fn test_load_config() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "{}", sample_config()).unwrap();
        let registry = ProviderCacheRegistry::load_from_file(tmp.path()).unwrap();

        let openai = registry.get("openai").unwrap();
        assert_eq!(openai.cache_mode, CacheMode::Automatic);
        assert_eq!(openai.cache_read_discount, 0.5);
        assert_eq!(openai.cache_write_multiplier, 0.0);

        let anthropic = registry.get("anthropic_5min").unwrap();
        assert_eq!(anthropic.cache_mode, CacheMode::Explicit);
        assert_eq!(anthropic.ttl_seconds, Some(300));
    }

    #[test]
    fn test_cache_is_cold() {
        let cfg = ProviderCacheConfig {
            cache_mode: CacheMode::Explicit,
            cache_read_discount: 0.1,
            cache_write_multiplier: 1.25,
            ttl_seconds: Some(300),
            requires_markers: true,
            notes: None,
        };
        assert!(!cfg.cache_is_cold(200)); // 200s < 300s TTL
        assert!(cfg.cache_is_cold(400));  // 400s > 300s TTL

        let auto_cfg = ProviderCacheConfig {
            cache_mode: CacheMode::Automatic,
            cache_read_discount: 0.5,
            cache_write_multiplier: 0.0,
            ttl_seconds: None,
            requires_markers: false,
            notes: None,
        };
        assert!(!auto_cfg.cache_is_cold(99999)); // automatic never cold from our perspective
    }

    #[test]
    fn test_from_model_entry() {
        let entry = serde_json::json!({
            "model": "anthropic/claude-sonnet-5",
            "cache_mode": "explicit",
            "cache_read_discount": 0.1,
            "cache_write_multiplier": 1.25,
            "cache_ttl_seconds": 300,
            "cache_requires_markers": true
        });

        let cfg = ProviderCacheConfig::from_model_entry(&entry);
        assert_eq!(cfg.cache_mode, CacheMode::Explicit);
        assert_eq!(cfg.cache_read_discount, 0.1);
        assert_eq!(cfg.cache_write_multiplier, 1.25);
        assert_eq!(cfg.ttl_seconds, Some(300));
        assert!(cfg.requires_markers);

        // Missing fields → defaults
        let bare = serde_json::json!({"model": "unknown/model"});
        let cfg = ProviderCacheConfig::from_model_entry(&bare);
        assert_eq!(cfg.cache_mode, CacheMode::Automatic);
        assert_eq!(cfg.cache_read_discount, 0.5);
        assert_eq!(cfg.cache_write_multiplier, 0.0);
        assert_eq!(cfg.ttl_seconds, None);
        assert!(!cfg.requires_markers);
    }
}
