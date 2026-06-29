# TraceLean IDE — Docker Usage

All dependencies (Rust toolchain, Node.js, system libs for Tauri) are bundled in containers. No local toolchain needed.

---

## Quick Reference

| Command | What it does |
|---------|-------------|
| `make check` | Build dev container, install deps, run `cargo check` |
| `make test` | Run all Rust tests in container |
| `make shell` | Interactive bash inside dev container |
| `make build` | Build release binary (multi-stage, produces `tracelean:latest` image) |
| `make clean` | Remove containers, volumes, local build artifacts |

---

## Development Container (`Dockerfile.dev`)

Full development environment. Source mounted as volume — edits on host reflect immediately inside container.

### What's included
- Rust 1.82 (stable) + cargo
- Node.js 22 + npm
- All Tauri Linux system deps (webkit2gtk, gtk3, dbus, etc.)
- `tauri-cli` pre-installed

### Usage

```bash
# Check that everything compiles
make check

# Run tests
make test

# Interactive shell (explore, run arbitrary commands)
make shell

# Inside shell:
cd src-tauri && cargo test
npm run build
cargo tauri build
```

### Volume mounts
- `.:/app` — your source code (live)
- `cargo-cache` — cargo registry (persists across runs)
- `cargo-git` — git deps (persists)
- `target-cache` — compiled artifacts (persists, speeds up rebuilds)

---

## Production Build (`Dockerfile`)

Multi-stage build. Produces a minimal runtime image with just the compiled binary.

### Stages
1. **Builder** — installs all deps, compiles frontend + backend
2. **Runtime** — slim Debian with only runtime libs + the binary

### Usage

```bash
# Build the release image
make build

# Or directly:
docker compose build build

# Run it (note: GUI app, needs display forwarding)
docker run --rm \
  -e DISPLAY=$DISPLAY \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  -v $(pwd)/my-project:/project \
  tracelean:latest
```

### Note on GUI
TraceLean is a desktop app (Tauri/WebKit). Running the production container requires X11/Wayland forwarding. For headless CI, use the dev container with `cargo check` / `cargo test` instead.

---

## docker-compose.yml Services

| Service | Dockerfile | Purpose |
|---------|-----------|---------|
| `dev` | `Dockerfile.dev` | Development: check, test, build |
| `build` | `Dockerfile` | Produce release image |

---

## Common Workflows

### First time setup
```bash
# Just run check — pulls image, installs everything
make check
```

### Iterate on Rust code
```bash
# Shell keeps running, cargo uses cached target
make shell
# inside:
cargo check   # fast incremental
cargo test    # run tests
```

### CI pipeline
```bash
docker compose run --rm dev bash -c "npm install && cd src-tauri && cargo test"
```

### Reset everything
```bash
make clean
# Removes containers, volumes (cargo cache, target), node_modules, dist
```

---

## Test Convention

Tests live in a separate `tests/` directory mirroring source structure:

```
src-tauri/
├── src/
│   ├── commands.rs
│   ├── state.rs
│   ├── undo_tree.rs
│   └── persistence.rs
└── tests/
    └── unit/
        ├── test_commands.rs
        ├── test_state.rs
        ├── test_undo_tree.rs
        └── test_persistence.rs
```

**Not** inline `#[cfg(test)]` modules. Separate files, named `test_{source_file}.rs`.

This mirrors the TraceLean project convention for user projects too:
```
project/
├── src/
│   └── parser/
│       └── lexer.rs
└── tests/
    └── unit/
        └── parser/
            └── test_lexer.rs
```

Integration tests go in `tests/integration/` named by requirement ID.

---

## Troubleshooting

**Build fails on system deps**: The Dockerfile pins all required packages. If you see pkg-config errors, you're probably running outside Docker — use `make check` instead.

**Slow first build**: Initial `cargo fetch` + compile takes a few minutes. Subsequent builds use cached volumes.

**Display issues with production image**: You need X11 forwarding. On Wayland, try `WAYLAND_DISPLAY` passthrough instead.
