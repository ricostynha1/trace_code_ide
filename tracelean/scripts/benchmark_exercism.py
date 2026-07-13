#!/usr/bin/env python3
"""Aider Exercism benchmark for telemetry regression testing.

Runs exercism tasks against an LLM via AWS Bedrock (or OpenRouter),
asking it to implement solutions. Then runs pytest to verify.

Does NOT require launching the Rust IDE binary — calls the LLM API directly
with a minimal code-generation agent loop.

Setup: Downloads exercism Python track tasks automatically if not present.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import time
import zipfile
from dataclasses import dataclass, field, asdict
from io import BytesIO
from pathlib import Path
from typing import Optional
from urllib.request import urlopen, Request
from urllib.error import URLError

# --- Config ---
COST_CAP_USD = float(os.environ.get("BENCHMARK_COST_CAP", "1.0"))
OUTPUT_PATH = os.environ.get("BENCHMARK_OUTPUT", "benchmark_results.json")
EXERCISM_DIR = Path(os.environ.get("EXERCISM_DIR", "exercism_tasks"))
LANGUAGE = os.environ.get("EXERCISM_LANGUAGE", "python")
MAX_ITERATIONS = int(os.environ.get("BENCHMARK_MAX_ITERATIONS", "3"))

# Model config — defaults to Qwen Coder Next on Bedrock
PROVIDER = os.environ.get("BENCHMARK_PROVIDER", "bedrock")  # bedrock | openrouter
MODEL_ID = os.environ.get("BENCHMARK_MODEL", "qwen.qwen3-coder-next")
BEDROCK_REGION = os.environ.get("AWS_REGION", os.environ.get("BEDROCK_REGION", "us-east-1"))
BEDROCK_ENDPOINT = os.environ.get("BEDROCK_ENDPOINT", "")  # auto-constructed if empty
OPENROUTER_KEY = os.environ.get("OPENROUTER_API_KEY", "")

# Pricing per 1M tokens (for cost tracking)
PRICING = {
    "qwen.qwen3-coder-next": (0.80, 3.20),
    "qwen.qwen3-coder-480b-a35b-instruct": (0.80, 3.20),
    "qwen.qwen3-coder-30b-a3b-instruct": (0.20, 0.78),
    "anthropic.claude-sonnet-5": (3.00, 15.00),
    "anthropic.claude-haiku-4-5": (0.80, 4.00),
}

# Exercism GitHub repo
EXERCISM_REPO_URL = "https://github.com/exercism/{language}/archive/refs/heads/main.zip"

# Default exercism tasks (Aider benchmark subset — Python track)
DEFAULT_TASKS = [
    "hello-world",
    "two-fer",
    "resistor-color",
    "rna-transcription",
    "reverse-string",
    "gigasecond",
    "space-age",
    "isogram",
    "pangram",
    "bob",
]


# ---------------------------------------------------------------------------
# Exercism setup
# ---------------------------------------------------------------------------

def setup_exercism_tasks(language: str = "python", tasks: list[str] | None = None):
    """Download exercism tasks from GitHub if not already present."""
    tasks = tasks or DEFAULT_TASKS
    missing = [t for t in tasks if not (EXERCISM_DIR / t).exists()]

    if not missing:
        print(f"All {len(tasks)} exercism tasks already present in {EXERCISM_DIR}")
        return

    print(f"Downloading {len(missing)} missing exercism tasks for '{language}' track...")
    url = EXERCISM_REPO_URL.format(language=language)

    try:
        print(f"  Fetching {url} ...")
        resp = urlopen(url, timeout=60)
        zip_data = BytesIO(resp.read())
    except (URLError, OSError) as e:
        print(f"ERROR: Failed to download exercism repo: {e}")
        sys.exit(1)

    EXERCISM_DIR.mkdir(parents=True, exist_ok=True)

    with zipfile.ZipFile(zip_data) as zf:
        prefix = f"{language}-main/exercises/practice/"
        extracted = 0

        for task_name in missing:
            task_prefix = f"{prefix}{task_name}/"
            members = [m for m in zf.namelist() if m.startswith(task_prefix)]

            if not members:
                print(f"  WARNING: Task '{task_name}' not found in exercism/{language} repo")
                continue

            task_dir = EXERCISM_DIR / task_name
            task_dir.mkdir(parents=True, exist_ok=True)

            for member in members:
                rel_path = member[len(task_prefix):]
                if not rel_path or member.endswith("/"):
                    continue
                out_path = task_dir / rel_path
                out_path.parent.mkdir(parents=True, exist_ok=True)
                with zf.open(member) as src, open(out_path, "wb") as dst:
                    dst.write(src.read())

            extracted += 1

    print(f"  Extracted {extracted} tasks to {EXERCISM_DIR}/")


# ---------------------------------------------------------------------------
# Data classes (mirrors Rust structs)
# ---------------------------------------------------------------------------

@dataclass
class TokenUsage:
    input_tokens: int = 0
    output_tokens: int = 0
    thinking_tokens: int = 0
    cached_tokens: int = 0


@dataclass
class CostEstimate:
    total_usd: float = 0.0
    input_cost: float = 0.0
    output_cost: float = 0.0
    cached_savings: float = 0.0


@dataclass
class SessionStats:
    total_requests: int = 0
    total_input_tokens: int = 0
    total_output_tokens: int = 0
    total_thinking_tokens: int = 0
    total_cached_tokens: int = 0
    total_cost_usd: float = 0.0

    def record(self, usage: TokenUsage, cost: CostEstimate):
        self.total_requests += 1
        self.total_input_tokens += usage.input_tokens
        self.total_output_tokens += usage.output_tokens
        self.total_thinking_tokens += usage.thinking_tokens
        self.total_cached_tokens += usage.cached_tokens
        self.total_cost_usd += cost.total_usd


@dataclass
class BenchmarkTask:
    name: str
    passed: bool = False
    tool_calls: int = 0
    model_iterations: int = 0
    execution_time_ms: int = 0
    token_usage: TokenUsage = field(default_factory=TokenUsage)
    cost: CostEstimate = field(default_factory=CostEstimate)
    parsing_failures: int = 0
    error: Optional[str] = None


@dataclass
class BenchmarkRun:
    tasks: list = field(default_factory=list)
    session_stats: SessionStats = field(default_factory=SessionStats)
    total_time_ms: int = 0
    pass_count: int = 0
    fail_count: int = 0
    aborted: bool = False
    abort_reason: Optional[str] = None
    cost_cap_usd: float = COST_CAP_USD
    model_id: str = MODEL_ID
    provider: str = PROVIDER

    def pass_rate(self) -> float:
        total = self.pass_count + self.fail_count
        return self.pass_count / total if total > 0 else 0.0


# ---------------------------------------------------------------------------
# LLM API calls
# ---------------------------------------------------------------------------

def call_bedrock(messages: list[dict], model_id: str) -> tuple[str, TokenUsage]:
    """Call AWS Bedrock /v1/chat/completions (converse API via mantle endpoint)."""
    import urllib.request

    endpoint = BEDROCK_ENDPOINT or f"https://bedrock-runtime.{BEDROCK_REGION}.amazonaws.com"
    url = f"{endpoint}/model/{model_id}/converse"

    # For Bedrock, we use the converse API format
    # But actually many bedrock models now support OpenAI-compatible /v1/chat/completions
    # Let's use the simpler approach via boto3 if available, else raw HTTP

    try:
        import boto3
        client = boto3.client("bedrock-runtime", region_name=BEDROCK_REGION)

        # Convert messages to Bedrock converse format
        bedrock_messages = []
        for msg in messages:
            role = msg["role"]
            if role == "system":
                continue  # handled separately
            bedrock_messages.append({
                "role": role,
                "content": [{"text": msg["content"]}]
            })

        system_msgs = [msg["content"] for msg in messages if msg["role"] == "system"]
        system_prompt = system_msgs[0] if system_msgs else None

        kwargs = {
            "modelId": model_id,
            "messages": bedrock_messages,
            "inferenceConfig": {"maxTokens": 4096, "temperature": 0.2},
        }
        if system_prompt:
            kwargs["system"] = [{"text": system_prompt}]

        response = client.converse(**kwargs)

        # Extract content
        output = response.get("output", {})
        content_blocks = output.get("message", {}).get("content", [])
        text = ""
        for block in content_blocks:
            if "text" in block:
                text += block["text"]

        # Extract usage
        usage_data = response.get("usage", {})
        usage = TokenUsage(
            input_tokens=usage_data.get("inputTokens", 0),
            output_tokens=usage_data.get("outputTokens", 0),
        )

        return text, usage

    except ImportError:
        print("ERROR: boto3 not installed. Install with: pip install boto3")
        print("  Or set BENCHMARK_PROVIDER=openrouter and OPENROUTER_API_KEY")
        sys.exit(1)
    except Exception as e:
        raise RuntimeError(f"Bedrock API error: {e}")


def call_openrouter(messages: list[dict], model_id: str) -> tuple[str, TokenUsage]:
    """Call OpenRouter /v1/chat/completions."""
    import urllib.request

    if not OPENROUTER_KEY:
        print("ERROR: OPENROUTER_API_KEY not set")
        sys.exit(1)

    url = "https://openrouter.ai/api/v1/chat/completions"
    body = json.dumps({
        "model": model_id,
        "messages": messages,
        "max_tokens": 4096,
        "temperature": 0.2,
    }).encode()

    req = Request(url, data=body, headers={
        "Authorization": f"Bearer {OPENROUTER_KEY}",
        "Content-Type": "application/json",
    })

    resp = urlopen(req, timeout=120)
    data = json.loads(resp.read())

    content = data["choices"][0]["message"]["content"]
    usage_data = data.get("usage", {})
    usage = TokenUsage(
        input_tokens=usage_data.get("prompt_tokens", 0),
        output_tokens=usage_data.get("completion_tokens", 0),
    )

    return content, usage


def call_llm(messages: list[dict]) -> tuple[str, TokenUsage]:
    """Route to configured provider."""
    if PROVIDER == "bedrock":
        return call_bedrock(messages, MODEL_ID)
    elif PROVIDER == "openrouter":
        return call_openrouter(messages, MODEL_ID)
    else:
        raise ValueError(f"Unknown provider: {PROVIDER}")


def compute_cost(usage: TokenUsage) -> CostEstimate:
    """Compute cost from usage and model pricing."""
    input_price, output_price = PRICING.get(MODEL_ID, (1.0, 3.0))
    input_cost = (usage.input_tokens / 1_000_000) * input_price
    output_cost = (usage.output_tokens / 1_000_000) * output_price
    return CostEstimate(
        total_usd=input_cost + output_cost,
        input_cost=input_cost,
        output_cost=output_cost,
    )


# ---------------------------------------------------------------------------
# Task execution
# ---------------------------------------------------------------------------

def read_task_files(task_dir: Path) -> dict[str, str]:
    """Read relevant files from an exercism task directory."""
    files = {}
    for f in task_dir.rglob("*"):
        if f.is_file() and f.suffix == ".py" and "__pycache__" not in str(f):
            files[f.name] = f.read_text()
    # Also read .docs/instructions.md if present
    instructions = task_dir / ".docs" / "instructions.md"
    if instructions.exists():
        files["instructions.md"] = instructions.read_text()
    return files


def extract_code(response: str) -> str | None:
    """Extract Python code from LLM response (```python blocks or full response)."""
    # Try to find ```python ... ``` blocks
    pattern = r"```python\s*\n(.*?)```"
    matches = re.findall(pattern, response, re.DOTALL)
    if matches:
        return matches[-1].strip()  # last block is usually the final answer

    # Try ``` blocks without language
    pattern = r"```\s*\n(.*?)```"
    matches = re.findall(pattern, response, re.DOTALL)
    if matches:
        return matches[-1].strip()

    # If response looks like pure code (starts with def/class/import), use it directly
    stripped = response.strip()
    if stripped.startswith(("def ", "class ", "import ", "from ")):
        return stripped

    return None


def find_solution_file(task_dir: Path) -> Path | None:
    """Find the file the student is supposed to edit (not test file)."""
    for f in task_dir.glob("*.py"):
        if f.name.startswith("test_") or f.name == "__init__.py":
            continue
        # Check if it's a stub (has 'pass' or placeholder)
        content = f.read_text()
        if "pass" in content or "raise NotImplementedError" in content or "..." in content:
            return f
    # Fallback: any non-test .py file
    for f in task_dir.glob("*.py"):
        if not f.name.startswith("test_") and f.name != "__init__.py":
            return f
    return None


def run_tests(task_dir: Path) -> tuple[bool, str]:
    """Run pytest on the task directory."""
    try:
        result = subprocess.run(
            [sys.executable, "-m", "pytest", str(task_dir), "-q", "--tb=short"],
            capture_output=True,
            text=True,
            timeout=30,
            cwd=str(task_dir),
        )
        output = result.stdout + result.stderr
        return result.returncode == 0, output
    except subprocess.TimeoutExpired:
        return False, "Timeout (30s)"
    except Exception as e:
        return False, str(e)


def run_task(task_name: str, session_stats: SessionStats) -> BenchmarkTask:
    """Execute a single exercism task: prompt LLM, write solution, run tests."""
    result = BenchmarkTask(name=task_name)
    start = time.time()

    task_dir = EXERCISM_DIR / task_name
    if not task_dir.exists():
        result.error = f"Task directory not found: {task_dir}"
        result.execution_time_ms = int((time.time() - start) * 1000)
        return result

    # Find the solution file and read task context
    solution_file = find_solution_file(task_dir)
    if not solution_file:
        result.error = "Could not find solution file"
        result.execution_time_ms = int((time.time() - start) * 1000)
        return result

    task_files = read_task_files(task_dir)
    original_content = solution_file.read_text()

    # Build prompt
    system = (
        "You are a coding assistant. Implement the solution for this exercism exercise. "
        "Return ONLY the complete implementation code in a ```python code block. "
        "Do not include test code. Do not explain."
    )

    context_parts = []
    if "instructions.md" in task_files:
        context_parts.append(f"## Instructions\n{task_files['instructions.md']}")

    # Include test file for context
    for fname, content in task_files.items():
        if fname.startswith("test_"):
            context_parts.append(f"## Test file ({fname})\n```python\n{content}\n```")

    # Include stub
    context_parts.append(f"## File to implement ({solution_file.name})\n```python\n{original_content}\n```")

    user_msg = "\n\n".join(context_parts)
    user_msg += f"\n\nImplement the complete solution for `{solution_file.name}`."

    messages = [
        {"role": "system", "content": system},
        {"role": "user", "content": user_msg},
    ]

    # Iterative loop: try up to MAX_ITERATIONS times
    for iteration in range(MAX_ITERATIONS):
        result.model_iterations += 1

        try:
            response, usage = call_llm(messages)
            cost = compute_cost(usage)

            # Accumulate usage
            result.token_usage.input_tokens += usage.input_tokens
            result.token_usage.output_tokens += usage.output_tokens
            result.cost.total_usd += cost.total_usd
            result.cost.input_cost += cost.input_cost
            result.cost.output_cost += cost.output_cost

            session_stats.record(usage, cost)

        except Exception as e:
            result.error = f"LLM call failed: {e}"
            break

        # Cost cap check
        if session_stats.total_cost_usd >= COST_CAP_USD:
            result.error = "Cost cap reached mid-task"
            break

        # Extract and write code
        code = extract_code(response)
        if not code:
            result.parsing_failures += 1
            if iteration < MAX_ITERATIONS - 1:
                messages.append({"role": "assistant", "content": response})
                messages.append({"role": "user", "content": "Please provide the implementation in a ```python code block."})
                continue
            result.error = "Failed to extract code from response"
            break

        # Write solution
        solution_file.write_text(code)

        # Run tests
        passed, test_output = run_tests(task_dir)
        if passed:
            result.passed = True
            break
        elif iteration < MAX_ITERATIONS - 1:
            # Feed error back for retry
            messages.append({"role": "assistant", "content": response})
            messages.append({"role": "user", "content": f"Tests failed:\n```\n{test_output[:1000]}\n```\nFix the implementation."})
        else:
            result.error = f"Tests still failing after {MAX_ITERATIONS} attempts"

    # Restore original if failed (don't leave broken code)
    if not result.passed:
        solution_file.write_text(original_content)

    result.execution_time_ms = int((time.time() - start) * 1000)
    return result


# ---------------------------------------------------------------------------
# Benchmark runner
# ---------------------------------------------------------------------------

def run_benchmark(task_names: list[str] | None = None) -> BenchmarkRun:
    """Run full benchmark with cost cap enforcement."""
    tasks = task_names or DEFAULT_TASKS

    # Auto-download exercism tasks if missing
    setup_exercism_tasks(LANGUAGE, tasks)

    run = BenchmarkRun(cost_cap_usd=COST_CAP_USD, model_id=MODEL_ID, provider=PROVIDER)
    run_start = time.time()

    for task_name in tasks:
        # Pre-task cost cap check
        if run.session_stats.total_cost_usd >= COST_CAP_USD:
            run.aborted = True
            run.abort_reason = (
                f"Cost cap ${COST_CAP_USD:.2f} exceeded "
                f"(spent ${run.session_stats.total_cost_usd:.4f})"
            )
            break

        print(f"  Running: {task_name}...", end=" ", flush=True)
        task_result = run_task(task_name, run.session_stats)
        run.tasks.append(task_result)

        if task_result.passed:
            run.pass_count += 1
            print(f"✓ ({task_result.model_iterations} iter, ${task_result.cost.total_usd:.4f})")
        else:
            run.fail_count += 1
            err = task_result.error or "tests failed"
            print(f"✗ ({err[:60]})")

        # Post-task cost cap check
        if run.session_stats.total_cost_usd >= COST_CAP_USD:
            run.aborted = True
            run.abort_reason = (
                f"Cost cap ${COST_CAP_USD:.2f} exceeded after '{task_name}' "
                f"(spent ${run.session_stats.total_cost_usd:.4f})"
            )
            break

    run.total_time_ms = int((time.time() - run_start) * 1000)
    return run


def to_json(run: BenchmarkRun) -> str:
    """Serialize BenchmarkRun to JSON."""
    data = asdict(run)
    data["pass_rate"] = run.pass_rate()
    return json.dumps(data, indent=2)


def main():
    task_names = sys.argv[1:] if len(sys.argv) > 1 else None
    tasks_to_run = task_names or DEFAULT_TASKS

    print(f"Exercism Benchmark — {PROVIDER}:{MODEL_ID}")
    print(f"Cost cap: ${COST_CAP_USD:.2f} | Max iterations/task: {MAX_ITERATIONS}")
    print(f"Tasks: {tasks_to_run}")
    print(f"Exercism dir: {EXERCISM_DIR}")
    print("-" * 60)

    result = run_benchmark(task_names)

    # Print summary
    print(f"\n{'=' * 60}")
    print(f"Model: {PROVIDER}:{MODEL_ID}")
    print(f"Pass: {result.pass_count} | Fail: {result.fail_count} | "
          f"Rate: {result.pass_rate():.1%}")
    print(f"Cost: ${result.session_stats.total_cost_usd:.4f} | "
          f"Tokens: {result.session_stats.total_input_tokens}in/{result.session_stats.total_output_tokens}out | "
          f"Time: {result.total_time_ms}ms")
    if result.aborted:
        print(f"ABORTED: {result.abort_reason}")

    # Write JSON output
    output = to_json(result)
    Path(OUTPUT_PATH).write_text(output)
    print(f"\nResults written to: {OUTPUT_PATH}")

    sys.exit(0 if result.pass_count > 0 and not result.aborted else 1)


if __name__ == "__main__":
    main()
