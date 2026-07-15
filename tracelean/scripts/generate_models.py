#!/usr/bin/env python3
"""Generate data/models.json — single entry point for all model data.

Orchestrates:
1. Fetch model pricing from LiteLLM + coding rankings from llm-stats.com
2. Merge pricing + rankings into unified entries
3. Enrich with provider cache config from data/provider_cache.json

Usage:
    python scripts/generate_models.py

Requires:
    - LLM_STATS_API_KEY env var (get from https://llm-stats.com/developer)
    - pip install -r scripts/requirements.txt

Output:
    data/models.json — unified model catalog with pricing, rank, and cache config
"""

import subprocess
import sys
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent


def run_step(description: str, script: str):
    """Run a sub-script, abort on failure."""
    print(f"\n{'─' * 60}")
    print(f"  {description}")
    print(f"{'─' * 60}\n")

    result = subprocess.run(
        [sys.executable, str(SCRIPTS_DIR / script)],
        cwd=str(SCRIPTS_DIR.parent),
    )

    if result.returncode != 0:
        print(f"\n✗ FAILED: {script} (exit code {result.returncode})")
        sys.exit(result.returncode)

    print(f"\n✓ Done: {description}")


def main():
    print("═" * 60)
    print("  generate_models.py — build data/models.json")
    print("═" * 60)

    # Step 1: Fetch raw data from external APIs
    run_step(
        "Fetching model pricing (LiteLLM) + coding rankings (llm-stats.com)",
        "fetch_model_metadata.py",
    )

    # Step 2: Merge + enrich with provider cache info → models.json
    run_step(
        "Merging pricing + rankings + provider cache → models.json",
        "merge_model_data.py",
    )

    print(f"\n{'═' * 60}")
    print("  ✓ data/models.json generated successfully")
    print(f"{'═' * 60}\n")


if __name__ == "__main__":
    main()
