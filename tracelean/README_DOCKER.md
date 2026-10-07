# TraceLean — running and testing in containers

TraceLean's test suite reaches outside Rust more than most: it elaborates Lean, it spawns
language servers, and it runs a Python differential adapter against a compiled Lean model.
Each of those is a toolchain someone has to install at a version that has to match, which
is why `make test` in a container is the recommended way to check this repository on a
machine that is not the one it was written on.

Run everything below from the `tracelean/` directory.

---

## Quick reference

| Command | What it does |
|---|---|
| `make shell` | Interactive shell with every toolchain installed |
| `make check` | `cargo check --workspace --all-targets` |
| `make test` | Rust tests, TypeScript type-check, vitest |
| `make e2e` | Builds the example's Lean model runner and runs all six differential bindings against it |
| `make vitest` | Frontend tests only |
| `make cover` | Line coverage and test results, written as JSON in `/tmp` for TraceLean to import (currently 59% Rust, 95% Python) |
| `make sandbox-test` | The bubblewrap sandbox tests, which need privileges `make ci` cannot grant |
| `make ci` | Everything, as one image build — the CI gate |
| `make build` | Release image → `tracelean:latest` |
| `make clean` | Remove containers, volumes, `target/`, `node_modules/`, `dist/` |

---

## The images

| File | Service | Purpose |
|---|---|---|
| `Dockerfile.dev` | `dev`, `check`, `test`, `e2e` | Development and testing. Source is bind-mounted, so host edits apply immediately. |
| `Dockerfile` | `app` | Two-stage release build → a slim runtime image with the GUI binary. |
| `Dockerfile.ci` | — | The CI gate. The image *building* is the check: any failing step fails the build. It is a throwaway — the point is the exit code, not the ~29 GB image, so `docker image rm tracelean-ci` after a run is fine. |

### What is installed, and why it is checked

`Dockerfile.dev` ends with a layer that runs `--version` on every tool. A container that
builds green and then cannot elaborate Lean is the same lie as a dashboard reporting a
check nobody ran, so the check is part of the build rather than a note in this file.

| Tool | Version | Needed for |
|---|---|---|
| Rust | pinned by `rust-toolchain.toml` (1.95.0) | the workspace |
| rust-analyzer | same toolchain as the compiler | the Rust language server TraceLean spawns |
| Node | 22 | the frontend build and vitest |
| Python | 3 | the example's implementation, driven by TraceLean's differential-testing runner |
| Lean + Lake | 4.12.0 via elan | the Lean model runner, `lake serve` for the infoview |
| pyright | 1.1.403 | the Python language server |
| cargo-llvm-cov | 0.6.16 | line coverage of the Rust workspace (`make cover`) |
| coverage.py | Debian's `python3-coverage` | line coverage of the example's Python |

### Three different things called coverage

Worth separating, because the panel and `make cover` answer different questions:

| Name | Asks | Where |
|---|---|---|
| Requirement coverage | What fraction of clauses carry evidence above L1? | `trace/map.rs`, the traceability panel |
| Input coverage | Did a differential run exercise every input constructor and see an output variant? | `drt::Coverage`, the floor a run must clear to earn L3 |
| Line coverage | Was this code executed by the tests at all? | `make cover` |

`make cover` leaves three reports in the container's `/tmp`: `llvm-cov.json`,
`coverage-py.json` and `test-results.json`. TraceLean imports them — the graph
then shows coverage on the code nodes it was measured for, and a pass count on
the implementation each test is annotated against. It measures nothing itself:
adding a language means adding an importer, not teaching the graph about another
tool.

Spec strength is a fourth thing, and the one the others cannot see: a proof can
hold of many different functions, so `trace/strength.rs` asks whether the proved
properties *determine* the model. See `docs/spec_strength.md`.

The first two are built in and gate the evidence levels. Line coverage is a tool
the container carries; nothing in TraceLean consumes it yet. The obvious use is
checking a `@tests` claim — a test annotated as exercising a clause whose
implementation never executes is a claim the line coverage can falsify — and
that is not built.

The Lean toolchain is pinned to what `example/formal/lean-toolchain` names and installed at
image-build time, so a test run does not begin with a several-hundred-megabyte download. A
project with a different `lean-toolchain` still works: elan fetches what that file asks for.

`Dockerfile.dev` accepts `--build-arg UID=$(id -u)` if the fixed UID 10001 does not suit
your host's bind-mount permissions.

### What `make ci` cannot check

`docker build` runs under the default seccomp profile, which forbids
unprivileged user namespaces. `bwrap --version` still succeeds there, so the
check is whether bubblewrap can actually *create* a namespace
(`sandbox::session::bwrap_available`); inside the CI build it cannot, and the
sandbox tests skip with that reason printed rather than failing. Run them for
real with `make sandbox-test`, which uses the `sandbox` compose service and its
`seccomp:unconfined` option.

The same distinction matters at runtime: TraceLean will not offer a sandboxed
session in an environment where bubblewrap cannot start one.

### What `make ci` builds its context from

The repository root, not `tracelean/`. Two checks need what lives above it: the
keymap test regenerates the root `README.md`, and the end-to-end differential
run works on `example/`. With the narrower context those tests silently skipped
— they are written to return when the project is absent — so the image reported
green on checks it had never run.

---

## The release image and the GUI

```bash
make build

# Allow the container to reach your X server, then run it:
xhost +local:docker
docker run --rm \
  -e DISPLAY=$DISPLAY \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  -v $(pwd)/../example:/project \
  tracelean:latest
```

Software GL is baked in (`LIBGL_ALWAYS_SOFTWARE`, `llvmpipe`, and WebKit's DMABUF renderer
disabled), because there is no GPU in the container and WebKit fails opaquely without one.
Pass `--device /dev/dri` to use the host GPU instead.

For Wayland, replace `-e DISPLAY` with `-e WAYLAND_DISPLAY=$WAYLAND_DISPLAY` and mount the
Wayland socket.

---

## Running on the host, without Docker

Install the same things the image installs:

```bash
# Rust — rust-toolchain.toml pins the version and components
rustup show

# Node 22 and the frontend deps
npm install

# Lean, pinned to the example project's toolchain
curl -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh \
  | sh -s -- -y --default-toolchain none
elan toolchain install leanprover/lean4:v4.12.0

# The Python language server
npm install -g pyright

# System libraries for Tauri
# Debian/Ubuntu:
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libdbus-1-dev \
  libssl-dev pkg-config build-essential curl wget file python3
# Fedora:
sudo dnf install webkit2gtk4.1-devel gtk3-devel librsvg2-devel \
  libsoup3-devel javascriptcoregtk4.1-devel dbus-devel \
  openssl-devel pkg-config gcc python3
```

Then:

```bash
cargo test --workspace     # 661 tests
npx tsc --noEmit
npm test                   # vitest
npm run tauri dev          # the GUI, with hot reload
```

A release build needs no `tauri-cli`: `tauri-build` reads `gui_backend/tauri.conf.json` and
embeds `dist/` at compile time, so

```bash
npm run build
cargo build --release -p tracelean   # → target/release/tracelean
```

produces the same binary `cargo tauri build --no-bundle` would. Install `tauri-cli` only
when you actually want an AppImage or `.deb`.

Missing a toolchain is a first-class, explained state rather than a failure: without Lean,
the differential tests report "no model runner" and the infoview says the server is not
running, instead of quietly passing.

---

## Test layout

Tests live in a separate `tests/` crate rather than inline `#[cfg(test)]` modules:

```
tests/
├── unit.rs
└── unit/
    ├── test_trace.rs      annotations, anchors, evidence, the project graph
    ├── test_drt.rs        differential testing, schema inference, binding
    ├── test_judge.rs
    ├── test_lsp.rs
    ├── test_myth.rs
    └── ...
```

Six tests are `#[ignore]`d because they write into `example/`, need a Lean toolchain, or
exist only to print what something looks like: `build_example_runner`, `run_example_drt`,
`materialize_example_bindings`, `probe_example_status`, `probe_project_graph`,
`probe_agent_graph_tool`. `make e2e` and `make ci` run the first two.

---

## Troubleshooting

| Problem | Fix |
|---|---|
| pkg-config errors | You are building outside Docker without the system libraries. Use `make check`. |
| Slow first build | Normal — `cargo fetch` plus a full compile. The `cargo-registry` and `target-cache` volumes make later builds fast. |
| GUI will not open | `xhost +local:docker`, and check `echo $DISPLAY`. |
| GL/Mesa errors | Software GL is baked in; try `--device /dev/dri` to use the host GPU. |
| Permission errors on bind-mounted files | Rebuild with `--build-arg UID=$(id -u)`. |
| `Permission denied` creating `/app/target` or the cargo registry | A named volume left over from an older image, created root-owned. `docker volume rm tracelean_target-cache tracelean_cargo-registry tracelean_cargo-git`, then run again. Docker seeds a named volume from whatever is at its mount point in the image, ownership included, so the image creates those directories owned by the non-root user. |
| `EACCES: permission denied, rmdir 'dist/assets'` on `npm run build` | A root-owned `dist/` left by an older container image that ran as root. `sudo rm -rf dist` once; the current `Dockerfile.dev` runs as an unprivileged user, so it will not come back. |
| `lean: command not found` inside the container | The image pins Lean 4.12.0; rebuild with `docker compose build --no-cache dev`. |
