#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

again=""
if [ "${1:-}" = "--again" ]; then again=1; shift; fi

jobs="${1:-${TRACELEAN_JOBS:-$(( $(nproc) > 1 ? $(nproc) - 1 : 1 ))}}"

export TRACELEAN_DRT_LINES="${TRACELEAN_DRT_LINES-1}"

echo "building the test binaries"
cargo test --workspace --no-run --quiet || exit 1

current=""
if [ -z "$again" ]; then
  current="$(cargo run -q -p tracelean-core --bin tracelean-trace -- . --drt-due 2>/dev/null | awk '$1 == "current" { print $2 }')"
fi

binaries="$(
  cargo test --workspace --no-run --message-format=json 2>/dev/null |
    CURRENT="$current" ROOT="$root" python3 -c '
import sys, json, os
current = set(os.environ["CURRENT"].split())
root = os.environ["ROOT"] + "/"
found, skipped = set(), set()
for line in sys.stdin:
    try:
        message = json.loads(line)
    except ValueError:
        continue
    if (message.get("reason") == "compiler-artifact"
            and message.get("executable")
            and message.get("profile", {}).get("test")):
        source = message.get("target", {}).get("src_path", "").removeprefix(root)
        (skipped if source in current else found).add(message["executable"])
print("\n".join(sorted(found)))
print(len(skipped), file=sys.stderr)
' 2>"$root/target/differential-skipped"
)"

count="$(printf '%s\n' "$binaries" | grep -c .)"
skipped="$(cat "$root/target/differential-skipped" 2>/dev/null || echo 0)"
echo "running $count suites, $jobs at a time; $skipped current, skipped (--again runs them)"

failures="$(
  printf '%s\n' "$binaries" |
    xargs -P "$jobs" -I{} sh -c '{} --include-ignored --test-threads 4 >/dev/null 2>&1 || echo {}'
)"

if [ -n "$failures" ]; then
  echo
  echo "these suites failed; run one of them through cargo to see why:"
  printf '%s\n' "$failures" | sed 's|.*/||; s|-[0-9a-f]*$||' | sed 's/^/  /'
  exit 1
fi

echo "all $count suites passed"
