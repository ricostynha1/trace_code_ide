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
# A differential suite every clause of which already has an agreed run against
# inputs that hash as they do now is skipped: running it again could only say
# the same (REQ-STALE.agreed_not_rerun; `tracelean-trace . --drt-due` lists
# which). Suites claiming no clause always run. `--again` runs every one.
#
#   tools/differential-all.sh              # every suite something changed under
#   tools/differential-all.sh 4            # four binaries at a time
#   tools/differential-all.sh --again      # every suite
#
# Exits non-zero, naming the suite, if any of them fails.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

again=""
if [ "${1:-}" = "--again" ]; then again=1; shift; fi

# Every core but one, so the machine stays usable (TRACELEAN_JOBS, or the first
# argument, overrides).
jobs="${1:-${TRACELEAN_JOBS:-$(( $(nproc) > 1 ? $(nproc) - 1 : 1 ))}}"

# Measured: the Rust runner is built instrumented and each op's bound entry is
# held to every line run or waived, which a Rust-bound op needs for L3
# (docs/04-coverage.md). TRACELEAN_DRT_LINES= (empty) to skip it.
export TRACELEAN_DRT_LINES="${TRACELEAN_DRT_LINES-1}"

echo "building the test binaries"
cargo test --workspace --no-run --quiet || exit 1

# The suites whose every clause is current, by source path.
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
