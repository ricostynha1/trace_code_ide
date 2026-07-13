#!/usr/bin/env python3
"""Fetch model metadata from LiteLLM and coding rankings from llm-stats.com API.

Notes:
- Claude models may report wrong endpoint due to litellm using bedrock-specific
  endpoints vs /v1/chat/completions.
- supports_prompt_caching field comes from litellm's 'supports_prompt_caching' key.
- Coding rankings from llm-stats.com (TrueSkill), ordered by score for stable sort.
- API key: set LLM_STATS_API_KEY env var (get from https://llm-stats.com/developer)
"""

import json
import os
import sys
from pathlib import Path

import requests

# --- Config ---
LITELLM_URL = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"

# llm-stats.com API (https://docs.llm-stats.com/api-reference/introduction)
LLM_STATS_BASE = "https://api.llm-stats.com/stats/v1"
LLM_STATS_API_KEY = os.environ.get("LLM_STATS_API_KEY", "")
# Providers relevant to project
RELEVANT_PROVIDERS = {"anthropic", "openai", "amazon", "nova", "qwen", "mistral", "deepseek", "google", "xai", "meta"}

DATA_DIR = Path(__file__).resolve().parent.parent / "data"


def is_relevant(model_key: str, model_info: dict) -> bool:
    """Check if model belongs to a relevant provider."""
    provider = model_info.get("litellm_provider", "").lower()
    key_lower = model_key.lower()
    for p in RELEVANT_PROVIDERS:
        if p in provider or p in key_lower:
            return True
    return False


def fetch_json(url: str, headers: dict | None = None) -> dict:
    resp = requests.get(url, timeout=30, headers=headers or {})
    resp.raise_for_status()
    return resp.json()


def build_pricing(litellm_data: dict) -> list[dict]:
    """Extract pricing info for relevant models from LiteLLM."""
    results = []
    for model_key, info in litellm_data.items():
        if model_key == "sample_spec":
            continue
        if not isinstance(info, dict):
            continue
        if not is_relevant(model_key, info):
            continue

        results.append({
            "model": model_key,
            "provider": info.get("litellm_provider", ""),
            "pricing": {
                "input_per_1m_tokens": (info.get("input_cost_per_token", 0) or 0) * 1_000_000,
                "output_per_1m_tokens": (info.get("output_cost_per_token", 0) or 0) * 1_000_000,
            },
            "context_window": info.get("max_tokens", info.get("max_input_tokens")),
            "supports_prompt_caching": info.get("supports_prompt_caching", False),
            "supports_tool_calling": info.get("supports_function_calling", False),
            "supported_endpoints": info.get("supported_openai_params", []),
            "release_info": info.get("source", ""),
        })
    return results


def fetch_coding_rankings() -> list[dict]:
    """Fetch coding rankings from llm-stats.com API.

    The website's "Coding Index Score" is a TrueSkill conservative_rating (μ − 3σ)
    computed over all benchmarks in the 'code' category. The /v1/rankings endpoint
    returns only the top 50 (API hard cap). For broader coverage we:
    1. Get the authoritative top-50 from /v1/rankings?category=code
    2. Fetch ALL raw scores from /v1/scores?category=code
    3. Compute TrueSkill ourselves for models beyond the top 50

    This replicates the methodology at:
    https://llm-stats.com/methodology/llm-stats-score
    """
    import trueskill

    if not LLM_STATS_API_KEY:
        print("WARNING: LLM_STATS_API_KEY not set. Set env var to fetch coding rankings.")
        print("  Get key at: https://llm-stats.com/developer")
        return []

    headers = {"Authorization": f"Bearer {LLM_STATS_API_KEY}"}

    # --- Step 1: Get authoritative top 50 from rankings endpoint ---
    top50: dict[str, dict] = {}
    try:
        resp = requests.get(
            f"{LLM_STATS_BASE}/rankings",
            params={"category": "code", "limit": 50},
            headers=headers, timeout=30,
        )
        resp.raise_for_status()
        data = resp.json()
        models = data.get("models", [])
        print(f"  Rankings endpoint (category=code): {len(models)} models")

        for m in models:
            model_id = m.get("model_id", "")
            if not model_id:
                continue
            top50[model_id] = {
                "model_id": model_id,
                "model_name": m.get("model_name", ""),
                "organization": m.get("organization", ""),
                "coding_index": m.get("conservative_rating", 0),
                "score_mu": m.get("score", 0),
                "open_weight": m.get("open_weight", False),
                "benchmarks_evaluated": m.get("benchmarks_evaluated"),
                "source": "rankings_api",
            }
    except Exception as e:
        print(f"  WARNING: Rankings endpoint failed: {e}")

    # --- Step 2: Fetch all raw code scores for TrueSkill computation ---
    all_scores: list[dict] = []
    try:
        cursor = None
        while True:
            params: dict = {"category": "code", "limit": 500}
            if cursor:
                params["cursor"] = cursor

            resp = requests.get(f"{LLM_STATS_BASE}/scores", params=params, headers=headers, timeout=30)
            resp.raise_for_status()
            data = resp.json()

            scores = data.get("scores", [])
            all_scores.extend(scores)
            cursor = data.get("next_cursor")
            if not cursor or not scores:
                break

        print(f"  Scores endpoint (category=code): {len(all_scores)} score rows")
    except Exception as e:
        print(f"  WARNING: Scores endpoint failed: {e}")

    # --- Step 3: Compute TrueSkill for all models ---
    # Group scores by benchmark (each benchmark = one "game")
    from collections import defaultdict
    bench_scores: dict[str, list[tuple[str, float]]] = defaultdict(list)
    model_names: dict[str, str] = {}
    model_orgs: dict[str, str] = {}

    for s in all_scores:
        model_id = s.get("model_id", "")
        bench_id = s.get("benchmark_id", "")
        score_val = s.get("normalized_score") or s.get("score", 0)
        if not model_id or not bench_id:
            continue
        bench_scores[bench_id].append((model_id, float(score_val)))
        model_names[model_id] = s.get("model_name", model_id)
        model_orgs[model_id] = s.get("organization", "")

    # Filter: benchmarks with >= 3 models (per methodology)
    valid_benchmarks = {b: scores for b, scores in bench_scores.items() if len(scores) >= 3}
    print(f"  Valid benchmarks (>=3 models): {len(valid_benchmarks)}/{len(bench_scores)}")

    # Compute cross-benchmark z-score for prior initialization
    model_norm_scores: dict[str, list[float]] = defaultdict(list)
    for bench_id, scores in valid_benchmarks.items():
        values = [v for _, v in scores]
        if len(values) < 2:
            continue
        mean = sum(values) / len(values)
        std = (sum((v - mean) ** 2 for v in values) / len(values)) ** 0.5
        if std == 0:
            continue
        for model_id, val in scores:
            z = (val - mean) / std
            model_norm_scores[model_id].append(z)

    model_g: dict[str, float] = {}
    for model_id, zscores in model_norm_scores.items():
        model_g[model_id] = sum(zscores) / len(zscores)

    # TrueSkill setup matching their parameters
    env = trueskill.TrueSkill(
        mu=25.0,
        sigma=25.0 / 3,
        draw_probability=0.05,
    )

    # Initialize ratings with prior: μ₀ = 25 + 5g
    ratings: dict[str, trueskill.Rating] = {}
    for model_id in model_names:
        g = model_g.get(model_id, 0.0)
        mu_init = 25.0 + 5.0 * g
        ratings[model_id] = env.create_rating(mu=mu_init, sigma=25.0 / 3)

    # Run 3 rating passes (per methodology)
    for _pass in range(3):
        for bench_id, scores in valid_benchmarks.items():
            # Sort by score descending (rank 0 = best)
            sorted_scores = sorted(scores, key=lambda x: -x[1])

            # Build rating groups for TrueSkill (each model is a "team" of 1)
            rating_groups = [(ratings[mid],) for mid, _ in sorted_scores]
            ranks = list(range(len(sorted_scores)))  # 0, 1, 2, ... (lower = better)

            # Rate
            new_ratings = env.rate(rating_groups, ranks=ranks)
            for i, (mid, _) in enumerate(sorted_scores):
                ratings[mid] = new_ratings[i][0]

    # --- Step 4: Build final rankings ---
    # Use API top-50 as authoritative, fill rest from our TrueSkill computation
    all_entries: dict[str, dict] = {}

    # Our computed ratings for all models
    for model_id, rating in ratings.items():
        conservative = rating.mu - 3 * rating.sigma
        all_entries[model_id] = {
            "model_id": model_id,
            "model_name": model_names.get(model_id, model_id),
            "organization": model_orgs.get(model_id, ""),
            "coding_index": round(conservative, 2),
            "score_mu": round(rating.mu, 2),
            "score_sigma": round(rating.sigma, 2),
            "open_weight": False,
            "source": "computed_trueskill",
        }

    # Overlay with authoritative top-50 (their values take precedence)
    for model_id, entry in top50.items():
        all_entries[model_id] = entry

    # Sort by coding_index descending
    entries = list(all_entries.values())
    entries.sort(key=lambda x: (-x.get("coding_index", 0), x.get("model_id", "")))
    for rank, entry in enumerate(entries, 1):
        entry["rank"] = rank

    print(f"  Total coding-ranked models: {len(entries)}")
    return entries


def fetch_model_catalog() -> list[dict]:
    """Fetch full model catalog from llm-stats.com with top_scores.

    Uses /v1/models endpoint with pagination (cursor-based, max 200 per page).
    Includes pricing, context_window, inference capabilities, top_scores.coding.
    """
    if not LLM_STATS_API_KEY:
        return []

    headers = {"Authorization": f"Bearer {LLM_STATS_API_KEY}"}
    all_models = []
    cursor = None

    while True:
        params = {"limit": 200}
        if cursor:
            params["cursor"] = cursor

        try:
            resp = requests.get(f"{LLM_STATS_BASE}/models", params=params, headers=headers, timeout=30)
            resp.raise_for_status()
            data = resp.json()
        except Exception as e:
            print(f"ERROR fetching model catalog: {e}")
            break

        models = data.get("models", [])
        all_models.extend(models)
        cursor = data.get("next_cursor")

        if not cursor or not models:
            break

    print(f"  Fetched {len(all_models)} models from llm-stats.com catalog")
    return all_models


def build_rankings_from_catalog(catalog: list[dict]) -> list[dict]:
    """Extract coding scores from catalog top_scores and rank.

    The catalog top_scores.code field is normalized to 0-1 for most models.
    A handful have raw values (Elo/arena leakage) > 1 — we cap at 1.0 to keep
    the ranking on a consistent scale.
    """
    entries = []
    for model in catalog:
        top_scores = model.get("top_scores", {}) or {}
        # API uses "code" key (not "coding") for coding category scores
        coding_score = top_scores.get("code")
        if coding_score is None:
            continue

        # Normalize: cap at 1.0 (outliers > 1 are raw scores on a different scale)
        coding_score = min(float(coding_score), 1.0)

        org = model.get("organization", {}) or {}
        entries.append({
            "model_id": model.get("id", ""),
            "model_name": model.get("name", ""),
            "organization": org.get("id", "") if isinstance(org, dict) else str(org),
            "score": coding_score,
            "context_window": model.get("context_window"),
            "supports_tools": (model.get("inference", {}) or {}).get("supports_tools", False),
        })

    # Sort by coding score descending, then model_id for stability
    entries.sort(key=lambda x: (-x["score"], x["model_id"]))
    for rank, entry in enumerate(entries, 1):
        entry["rank"] = rank
    return entries


def main():
    DATA_DIR.mkdir(parents=True, exist_ok=True)

    # --- LiteLLM pricing ---
    print("Fetching LiteLLM model prices...")
    litellm_data = fetch_json(LITELLM_URL)
    pricing = build_pricing(litellm_data)
    pricing.sort(key=lambda x: x["model"])

    # --- llm-stats.com coding rankings ---
    # Replicates the Coding Index Score from llm-stats.com/leaderboards/best-ai-for-coding
    # Uses TrueSkill (μ − 3σ) over all code-category benchmarks.
    # Top 50 from API (authoritative), rest computed locally from raw scores.
    print("Fetching coding rankings from llm-stats.com...")
    rankings = fetch_coding_rankings()

    # --- Write outputs ---
    pricing_path = DATA_DIR / "model_pricing.json"
    rankings_path = DATA_DIR / "model_rankings.json"

    with open(pricing_path, "w") as f:
        json.dump(pricing, f, indent=2)
    print(f"Wrote {pricing_path} ({len(pricing)} models)")

    with open(rankings_path, "w") as f:
        json.dump(rankings, f, indent=2)
    print(f"Wrote {rankings_path} ({len(rankings)} models)")

    if not LLM_STATS_API_KEY:
        print("\n⚠️  Set LLM_STATS_API_KEY to fetch coding rankings!")
        print("   Get your free key at: https://llm-stats.com/developer")
        sys.exit(1)


if __name__ == "__main__":
    main()
