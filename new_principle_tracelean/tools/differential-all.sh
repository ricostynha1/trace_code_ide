#!/usr/bin/env bash
# Run every differential suite, using the whole machine.
#
# `cargo test -- --ignored` runs the same tests and takes two and a half times
# as long, because cargo runs test *binaries* one at a time. Inside a binary
# libtest uses threads, so the parallelism a full run gets is bounded by the
# tests in one suite — a mean of 2.9 here, measured at 487% of sixteen cores.
#
# There is nothing to fix in the project for that: it is how cargo runs targets.
# So this builds the binaries with cargo and then runs them concurrently, which
# is the same work in the same processes with the same arguments. Measured on a
# sixteen-core machine, warm: 3m23 through cargo, 1m26 through this.
#
# Both shared runners are generated and built by the first suite that wants one
# and reused by the rest, so a cold run pays for that once. See
# docs/08-ideas.md §1.
#
#   tools/differential-all.sh              # every suite
#   tools/differential-all.sh 4            # four binaries at a time
#
# Exits non-zero, naming the suite, if any of them fails.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

# More than eight is slower here, not faster: the total CPU time is unchanged
# and the contention is not. A number that fits the machine rather than one that
# saturates it.
jobs="${1:-8}"

# Measured: the Rust runner is built instrumented and each op's bound entry is
# held to every line run or waived, which a Rust-bound op needs for L3
# (docs/04-coverage.md). TRACELEAN_DRT_LINES= (empty) to skip it.
export TRACELEAN_DRT_LINES="${TRACELEAN_DRT_LINES-1}"

echo "building the test binaries"
cargo test --workspace --no-run --quiet || exit 1

binaries="$(
  cargo test --workspace --no-run --message-format=json 2>/dev/null |
    python3 -c '
import sys, json
found = set()
for line in sys.stdin:
    try:
        message = json.loads(line)
    except ValueError:
        continue
    if (message.get("reason") == "compiler-artifact"
            and message.get("executable")
            and message.get("profile", {}).get("test")):
        found.add(message["executable"])
print("\n".join(sorted(found)))
'
)"

count="$(printf '%s\n' "$binaries" | grep -c .)"
echo "running $count suites, $jobs at a time"

failures="$(
  printf '%s\n' "$binaries" |
    xargs -P "$jobs" -I{} sh -c '{} --ignored --test-threads 4 >/dev/null 2>&1 || echo {}'
)"

if [ -n "$failures" ]; then
  echo
  echo "these suites failed; run one of them through cargo to see why:"
  printf '%s\n' "$failures" | sed 's|.*/||; s|-[0-9a-f]*$||' | sed 's/^/  /'
  exit 1
fi

echo "all $count suites passed"
