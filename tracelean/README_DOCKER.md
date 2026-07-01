# TraceLean IDE — Docker Usage

All dependencies (Rust, Node.js, system libs) are bundled in containers. No local toolchain needed.

---

## Architecture

`docker-compose.yml` defines two services. You interact with **services**, not Dockerfiles directly.

| Service | Dockerfile | Purpose |
|---------|------------|---------|
| `dev` | `Dockerfile.dev` | Dev environment: check, test, shell. Source mounted live. |
| `build` | `Dockerfile` | Multi-stage release → `tracelean:latest` image with GUI binary. |

---

## Quick Reference

| Command | What it does |
|---------|-------------|
| `make check` | Build dev image + `cargo check --workspace` |
| `make test` | Run all workspace tests |
| `make test-unit` | Run unit tests only (`tracelean-tests` package) |
| `make shell` | Interactive bash inside dev container |
| `make build` | Build release image → `tracelean:latest` |
| `make clean` | Remove containers, volumes, local artifacts |

---

## Dev Container (service `dev`)

Full development environment. Source is volume-mounted — edits on host reflect immediately.

### What's included
- Rust 1.88 + cargo
- Node.js 22 + npm
- All Tauri Linux system deps (webkit2gtk, gtk3, dbus, etc.)
- `tauri-cli`
- `elan` (Lean 4 toolchain manager)

### Build / rebuild

```bash
docker compose build dev            # build image
docker compose build --no-cache dev # clean rebuild
```

Builds automatically on first `make check`/`make shell`/`make test`.

### Use it

```bash
make shell   # interactive bash
make check   # cargo check --workspace
make test    # cargo test --workspace
```

Inside the shell:

```bash
cargo check          # fast incremental
cargo test           # run tests
npm run build        # build frontend
cargo build --release --bin tracelean   # build backend binary only
```

> `cargo tauri build` also bundles an AppImage/`.deb`. It needs `xdg-utils` (now in the image — rebuild with `docker compose build --no-cache dev` if you hit `xdg-open not found`). For dev you rarely need the bundle; `cargo build` produces the binary at `target/release/tracelean`.

### Volume mounts
- `.:/app` — source code (live)
- `cargo-cache` — cargo registry (persists)
- `cargo-git` — git deps (persists)
- `target-cache` — compiled artifacts (persists, fast rebuilds)

---

## Release Image & Launching the GUI (service `build`)

Produces a minimal runtime image with the compiled binary and Mesa software-GL drivers.

### Build it

```bash
make build
# or: docker compose build build
```

### Launch the GUI

```bash
# 1. Allow container to reach your X server
xhost +local:docker

# 2. Run TraceLean (opens GUI window on your desktop)
docker run --rm \
  -e DISPLAY=$DISPLAY \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  -v $(pwd)/example:/project \
  tracelean:latest
```

That's it. The image bakes in software-GL fallback env vars — no extra flags needed.

### Wayland

Replace `-e DISPLAY` with `-e WAYLAND_DISPLAY=$WAYLAND_DISPLAY` and mount the Wayland socket.

### Headless CI

Don't run the GUI. Use the dev service:

```bash
docker compose run --rm dev bash -c "npm install && cargo test --workspace"
```

---

## Common Workflows

### First time
```bash
make check   # pulls/builds image, installs deps, checks code
```

### See the AI Chat panel
The AI Chat button appears in the top menu bar. If you don't see it after pulling new code:
```bash
# Dev mode (rebuilds frontend automatically):
cargo tauri dev

# Or via Docker — rebuild from scratch:
docker compose build --no-cache dev
make shell
# inside:
npm install
npm run build
cargo build --release --bin tracelean
```

The panel has 4 tabs: **Chat**, **Settings** (provider/model config), **Log** (full prompt/response history), **Stats** (token usage & cost).

Configure a provider in Settings before chatting. The **Mock** provider is always available — it shows the exact prompt and lets you paste a response manually (useful for debugging prompts without spending tokens).

### Iterate on Rust
```bash
make shell
# inside:
cargo check   # fast incremental
cargo test
```

### Reset everything
```bash
make clean
# Removes containers, volumes (cargo cache, target), node_modules, dist
```

---

## Test Convention

Tests live in a separate `tests/` crate, not inline `#[cfg(test)]` modules:

```
tests/
├── unit.rs
└── unit/
    ├── test_commands.rs
    ├── test_state.rs
    ├── test_undo_tree.rs
    └── test_persistence.rs
```

Files named `test_{source_file}.rs`. Integration tests go in `tests/integration/` named by requirement ID.

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| pkg-config errors | You're running outside Docker. Use `make check`. |
| Slow first build | Normal — initial `cargo fetch` + compile. Cached after. |
| GUI won't open | Run `xhost +local:docker` first. Check `echo $DISPLAY`. |
| GL/Mesa errors | Image has software-GL baked in. Try `--device /dev/dri` for host GPU. |
| "connection refused" on DISPLAY | Ensure X server is running and `/tmp/.X11-unix` is mounted. |
| `cargo tauri build`: `xdg-open not found` | Rebuild dev image: `docker compose build --no-cache dev`. |

---

## Running Locally (without Docker)

If you have the toolchain installed on your host, you can skip Docker entirely.

### Prerequisites

- Rust 1.88+ (`rustup update stable`)
- Node.js 22+ and npm
- Tauri CLI:
  ```bash
  cargo install tauri-cli --version "^2"
  ```
- System libs (Debian/Ubuntu/Fedora):
  ```bash
  # Debian/Ubuntu:
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
    libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libdbus-1-dev \
    libssl-dev pkg-config build-essential curl wget file

  # Fedora:
  sudo dnf install webkit2gtk4.1-devel gtk3-devel librsvg2-devel \
    libsoup3-devel javascriptcoregtk4.1-devel dbus-devel \
    openssl-devel pkg-config gcc
  ```
- (Optional) Lean 4 via [elan](https://github.com/leanprover/elan) for spec type-checking

### Build & run

```bash
cd tracelean
npm install
cargo tauri dev     # hot-reload frontend + Rust backend
```

This opens the GUI window. The **AI Chat** button is in the top menu bar.

### Release build

```bash
npm run build
cargo tauri build --no-bundle   # binary at src-tauri/target/release/tracelean
```

### Run tests

```bash
cargo test --workspace
```
