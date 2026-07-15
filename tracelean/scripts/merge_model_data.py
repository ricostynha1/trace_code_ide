#!/usr/bin/env python3
"""Merge model_pricing.json and model_rankings.json into a unified models.json.

Matching: split both IDs into tokens (on '-' and '.'), then check if one's
tokens are a subsequence of the other's. Like a glob: claude*opus*4*5 matches
claude-opus-4-5-20251101. Longest token overlap wins.

Usage:
    python scripts/merge_model_data.py
"""

import json
import re
from pathlib import Path

DATA_DIR = Path(__file__).resolve().parent.parent / "data"


def extract_slug(pricing_key: str) -> str:
    """Extract core model slug from a litellm pricing key."""
    slug = pricing_key.lower().strip()

    # Take last path segment
    parts = slug.split("/")
    slug = parts[-1]

    # Strip purely-alpha provider dot-prefix: 'anthropic.claude-xxx' -> 'claude-xxx'
    slug = re.sub(r"^[a-z]+\.", "", slug)

    # Strip version suffixes
    slug = re.sub(r"-v\d+:\d+$", "", slug)       # -v1:0
    slug = re.sub(r"@\d+$", "", slug)             # @20251001
    slug = re.sub(r"-\d{8}$", "", slug)           # -20240620

    return slug


def tokenize(s: str) -> list[str]:
    """Split model ID into tokens on '-' and '.'."""
    return [t for t in re.split(r"[-.]", s.lower()) if t]


def tokens_are_subsequence(needle_tokens: list[str], haystack_tokens: list[str]) -> bool:
    """Check if needle tokens all appear in haystack tokens in order."""
    it = iter(haystack_tokens)
    return all(t in it for t in needle_tokens)


def find_best_match(slug: str, ranking_entries: list[tuple], ranking_index: dict) -> dict | None:
    """Find best ranking match for a slug using token subsequence.

    ranking_entries: list of (model_id, tokens) pre-computed.

    Bidirectional:
    - ranking tokens ⊆ slug tokens (slug has extra version/date tokens)
    - slug tokens ⊆ ranking tokens (slug stripped of date that ranking kept)

    Score: shared_tokens / max(len_slug_tokens, len_rid_tokens)
    Higher = tighter. Only accept if extra tokens are pure-digit (dates/versions).
    """
    if not slug or len(slug) < 3:
        return None

    slug_tokens = tokenize(slug)
    if not slug_tokens:
        return None

    # Exact match (fast path)
    if slug in ranking_index:
        return ranking_index[slug]

    best_rid = None
    best_score = 0.0

    for rid, rid_tokens in ranking_entries:
        if not rid_tokens:
            continue

        shared = 0
        extra_tokens = []

        # Direction 1: rid tokens are subsequence of slug tokens
        if len(rid_tokens) <= len(slug_tokens):
            if tokens_are_subsequence(rid_tokens, slug_tokens):
                shared = len(rid_tokens)
                # Extra tokens in slug that aren't in rid
                extra_tokens = [t for t in slug_tokens if t not in rid_tokens]

        # Direction 2: slug tokens are subsequence of rid tokens
        if len(slug_tokens) <= len(rid_tokens):
            if tokens_are_subsequence(slug_tokens, rid_tokens):
                s = len(slug_tokens)
                if s > shared:
                    shared = s
                    extra_tokens = [t for t in rid_tokens if t not in slug_tokens]

        if shared < 2:
            continue

        # Extra non-date tokens = bad (means different model variant)
        # Date tokens: purely numeric, >= 4 digits, or version-like (v1, v2)
        non_date_extras = [t for t in extra_tokens
                          if not (t.isdigit() and len(t) >= 4)
                          and not re.match(r"^v\d+$", t)]

        # If extra tokens include non-date words, skip (different variant)
        if non_date_extras:
            continue

        # Score: shared / total tokens (tighter = better)
        total = max(len(slug_tokens), len(rid_tokens))
        score = shared / total

        if score > best_score:
            best_rid = rid
            best_score = score

    if best_rid:
        return ranking_index[best_rid]
    return None


def resolve_provider_cache(model_key: str, provider_cache: dict) -> dict | None:
    """Resolve provider cache config for a model key.

    Matches based on provider keywords in the model key.
    Returns None for providers not in our cache config (they get null fields in output).
    """
    lower = model_key.lower()

    if "anthropic" in lower or "claude" in lower:
        return provider_cache.get("anthropic_5min")
    if "openai" in lower or "gpt" in lower or "/o1" in lower or "/o3" in lower:
        return provider_cache.get("openai")
    if "deepseek" in lower:
        return provider_cache.get("deepseek")
    if "minimax" in lower:
        return provider_cache.get("minimax_passive")
    if "qwen" in lower or "alibaba" in lower:
        return provider_cache.get("qwen")
    # Providers without explicit cache support — use OpenAI-like automatic defaults
    if "mistral" in lower:
        return provider_cache.get("openai")  # automatic prefix cache like OpenAI
    if "meta" in lower or "llama" in lower:
        return provider_cache.get("openai")
    if "xai" in lower or "grok" in lower:
        return provider_cache.get("openai")

    return None


def merge() -> list[dict]:
    """Merge pricing and rankings into unified model list."""
    pricing_path = DATA_DIR / "model_pricing.json"
    rankings_path = DATA_DIR / "model_rankings.json"
    provider_cache_path = DATA_DIR / "provider_cache.json"

    with open(pricing_path) as f:
        pricing = json.load(f)
    with open(rankings_path) as f:
        rankings = json.load(f)

    # Load provider cache config if available
    provider_cache = {}
    if provider_cache_path.exists():
        with open(provider_cache_path) as f:
            provider_cache = json.load(f).get("providers", {})

    ranking_index = {r["model_id"].lower(): r for r in rankings}
    # Pre-tokenize ranking IDs, sort by token count descending (most specific first)
    ranking_entries = [(rid, tokenize(rid)) for rid in ranking_index]
    ranking_entries.sort(key=lambda x: -len(x[1]))

    matched_count = 0
    results = []

    for entry in pricing:
        pricing_key = entry["model"]
        slug = extract_slug(pricing_key)

        match = find_best_match(slug, ranking_entries, ranking_index)

        # Resolve provider cache config for this model
        cache_config = resolve_provider_cache(pricing_key, provider_cache)

        merged_entry = {
            **entry,
            "slug": slug,
            "coding_index": match["coding_index"] if match else None,
            "coding_rank": match["rank"] if match else None,
            "ranking_model_id": match["model_id"] if match else None,
            # Provider cache fields (from data/provider_cache.json)
            "cache_mode": cache_config.get("cache_mode") if cache_config else None,
            "cache_read_discount": cache_config.get("cache_read_discount") if cache_config else None,
            "cache_write_multiplier": cache_config.get("cache_write_multiplier") if cache_config else None,
            "cache_ttl_seconds": cache_config.get("ttl_seconds") if cache_config else None,
            "cache_requires_markers": cache_config.get("requires_markers") if cache_config else None,
        }
        results.append(merged_entry)

        if match:
            matched_count += 1

    # Stats
    unique_matched_rankings = set(
        r["ranking_model_id"] for r in results if r["ranking_model_id"]
    )
    print(f"Pricing entries: {len(pricing)}")
    print(f"Ranking entries: {len(rankings)}")
    print(f"Matched: {matched_count}/{len(pricing)} pricing entries")
    print(f"Unique rankings matched: {len(unique_matched_rankings)}/{len(rankings)}")

    # Show unmatched rankings
    unmatched_rankings = [
        r for r in rankings if r["model_id"].lower() not in
        {rid.lower() for rid in unique_matched_rankings}
    ]
    if unmatched_rankings:
        print(f"\nUnmatched rankings ({len(unmatched_rankings)}):")
        for r in unmatched_rankings[:15]:
            print(f"  #{r['rank']:3d} {r['model_id']:40s} ({r['model_name']})")
        if len(unmatched_rankings) > 15:
            print(f"  ... and {len(unmatched_rankings) - 15} more")

    return results


def main():
    results = merge()

    output_path = DATA_DIR / "models.json"
    with open(output_path, "w") as f:
        json.dump(results, f, indent=2)
    print(f"\nWrote {output_path} ({len(results)} entries)")


if __name__ == "__main__":
    main()
