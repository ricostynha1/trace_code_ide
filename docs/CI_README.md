# CI

`tracelean/Dockerfile.ci` is the CI check. There's no separate test runner
script — the image build itself gates on `cargo check`, `cargo test`, and
`npx tsc --noEmit`, each as its own `RUN` step. If any step fails, the
`docker build` exits non-zero (red). If every step exits 0, the build
succeeds (green). GitHub Actions builds the exact same Dockerfile, so a
local pass and a CI pass are the same claim.

## Run it locally

```sh
cd tracelean
docker build -f Dockerfile.ci -t tracelean-ci .
```

Exit code 0 → green. Non-zero → read the build log for the first failing
`RUN` step; Docker prints the failing command and its output.

There is no meaningful `docker run` step afterward — the tests already ran
during the build, and `CMD ["true"]` is just a placeholder so the image has
something to execute. The build log *is* the test report.

## What runs on every push

`.github/workflows/ci.yml` builds `tracelean/Dockerfile.ci` on every push
and pull request via `docker build` — no other steps. Same image, same gates,
same log format as the local command above.

## Scope

- `cargo test --workspace` is safe to run unattended: the only test that
  hits a live AI provider (`tests/integration_bedrock.rs`) is gated behind
  the `live_bedrock` Cargo feature, off by default, so it does not build or
  run here.
- Sandbox tests (`core/src/ai/shell_sandbox.rs`) that need `bwrap`
  overlay support self-skip when unavailable
  (`overlay_end_to_end_when_available`) — Docker containers often can't
  nest user namespaces, and neither `bwrap` nor `strace` are installed in
  the CI image. This is intentional: the test still compiles and runs, it
  just no-ops instead of failing, so it isn't asserting anything about
  sandboxing behavior in this environment. It exists to catch regressions
  on machines (including the user's own) where bwrap *is* available.
- `clippy`/`fmt --check` are deliberately not included yet — the codebase
  doesn't have `rustfmt.toml`/`clippy.toml` baselines landed, so adding
  either now would just surface pre-existing drift unrelated to a given
  change. Add them once those baselines exist.

## Base image note

`Dockerfile.ci` uses `rust:1.88-trixie`, not `-bookworm`. The prebuilt
onnxruntime binary pulled in transitively by `fastembed` (used for local
embeddings/semantic search) references `__isoc23_*` glibc symbol versions
that only exist on glibc >= 2.38. Debian bookworm ships glibc 2.36, so
linking `cargo test` failed there with `undefined reference to
`__isoc23_strtoull``; trixie (glibc 2.40) resolves it.
