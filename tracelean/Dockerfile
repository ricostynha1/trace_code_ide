# TraceLean — release build.
#
# Two stages: compile the frontend and the Tauri binary, then ship the binary on
# a slim runtime with the libraries it dlopens and a Lean toolchain for the
# projects it will open.
#
# There is no `cargo install tauri-cli` here. `tauri-build` reads
# `gui_backend/tauri.conf.json` at compile time and embeds `dist/`, so
# `npm run build && cargo build --release -p tracelean` produces the same binary
# `cargo tauri build --no-bundle` would, without spending a long layer compiling
# the CLI that would only be asked to skip bundling.
FROM rust:1.95-trixie AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libdbus-1-dev \
    libssl-dev \
    pkg-config \
    build-essential \
    ca-certificates \
    curl \
    file \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Node dependencies first, so editing Rust does not reinstall them.
COPY package.json package-lock.json* ./
RUN npm ci || npm install

# Then the Cargo manifests, for the same reason. The workspace is
# core/ + gui_backend/ + tui/ + tests/; `src-tauri/` has not existed since the
# crates were split, and a Dockerfile that still names it fails at COPY.
COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./
COPY core/Cargo.toml core/
COPY gui_backend/Cargo.toml gui_backend/
COPY tui/Cargo.toml tui/
COPY tests/Cargo.toml tests/
# Stub sources so `cargo fetch` can resolve the workspace. Every path here has
# to match the crate's manifest: `tests/` declares `[lib] path = "lib.rs"` and
# four `[[test]]` targets, and `tui` is a lib plus a bin.
RUN mkdir -p core/src gui_backend/src tui/src tests \
    && echo 'pub fn placeholder() {}' > core/src/lib.rs \
    && echo 'pub fn run() {}' > gui_backend/src/lib.rs \
    && echo 'fn main() {}' > gui_backend/src/main.rs \
    && echo 'pub fn run() {}' > tui/src/lib.rs \
    && echo 'fn main() {}' > tui/src/main.rs \
    && touch tests/lib.rs tests/unit.rs tests/integration_bedrock.rs \
             tests/integration_cost_reduction.rs tests/integration_sandbox_session.rs \
    && cargo fetch

COPY . .

RUN npm run build
RUN cargo build --release -p tracelean

# --- Runtime ------------------------------------------------------------
FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    librsvg2-2 \
    libsoup-3.0-0 \
    libjavascriptcoregtk-4.1-0 \
    libdbus-1-3 \
    libgl1-mesa-dri \
    libglx-mesa0 \
    libegl-mesa0 \
    dbus-x11 \
    ca-certificates \
    curl \
    git \
    python3 \
    && rm -rf /var/lib/apt/lists/*

# Software GL: there is no GPU in the container, and WebKit's DMABUF renderer
# fails opaquely without one.
ENV LIBGL_ALWAYS_SOFTWARE=1 \
    GALLIUM_DRIVER=llvmpipe \
    WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    WEBKIT_DISABLE_DMABUF_RENDERER=1

RUN useradd -m -u 10001 tracelean
COPY --from=builder /app/target/release/tracelean /usr/local/bin/tracelean
# Next to the binary: `read_ui_settings_file` looks in the executable's own
# directory first, so this is where keymap.json and the highlight queries have
# to land for the shipped binary to find them.
COPY --from=builder /app/ui_settings /usr/local/bin/ui_settings

USER tracelean
ENV PATH="/home/tracelean/.elan/bin:${PATH}"

# Lean, pinned to the toolchain the example project names. A project with a
# different `lean-toolchain` still works: elan fetches what that file asks for.
RUN curl -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh \
    | sh -s -- -y --default-toolchain leanprover/lean4:v4.12.0

ENTRYPOINT ["tracelean"]
