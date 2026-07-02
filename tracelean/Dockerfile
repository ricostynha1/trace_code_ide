# TraceLean IDE - Development Build Container
# Multi-stage: builds both Rust backend and React frontend

FROM rust:1.88-bookworm AS builder

# Install system deps for Tauri (Linux) and tree-sitter-lean4 build (cc)
RUN apt-get update && apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libdbus-1-dev \
    libssl-dev \
    pkg-config \
    build-essential \
    curl \
    wget \
    file \
    && rm -rf /var/lib/apt/lists/*

# Install Node.js 22
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy package files first for layer caching
COPY package.json package-lock.json* ./
RUN npm install

# Install tauri-cli for proper release build with embedded frontend
RUN cargo install tauri-cli --version "^2"

# Copy Rust manifest for dep caching
COPY Cargo.toml Cargo.lock* ./
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock* src-tauri/
COPY tests/Cargo.toml tests/
COPY src-tauri/build.rs src-tauri/
# Create dummy src to cache deps
RUN mkdir -p src-tauri/src && \
    echo 'fn main() {}' > src-tauri/src/main.rs && \
    echo 'pub fn run() {}' > src-tauri/src/lib.rs && \
    mkdir -p tests/unit && touch tests/lib.rs && \
    cargo fetch

# Copy full source
COPY . .

# Build frontend
RUN npm run build

# Build Rust backend with Tauri (embeds frontend, no bundle)
RUN cargo tauri build --no-bundle

# --- Runtime stage ---
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
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
    && rm -rf /var/lib/apt/lists/*

# Force software GL + disable WebKit DMABUF/compositing (no GPU in container)
ENV LIBGL_ALWAYS_SOFTWARE=1 \
    GALLIUM_DRIVER=llvmpipe \
    WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    WEBKIT_DISABLE_DMABUF_RENDERER=1

# Install elan (Lean 4 toolchain manager) for spec type-checking
RUN curl -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh | sh -s -- -y --default-toolchain leanprover/lean4:stable

COPY --from=builder /app/target/release/tracelean /usr/local/bin/tracelean
COPY --from=builder /app/ui_settings /usr/local/bin/ui_settings

# Create non-root user and grant ownership of app/toolchain dirs
RUN useradd -m -u 10001 tracelean \
    && mv /root/.elan /home/tracelean/.elan \
    && chown -R tracelean:tracelean /home/tracelean/.elan \
    && chown tracelean:tracelean /usr/local/bin/tracelean /usr/local/bin/ui_settings

ENV PATH="/home/tracelean/.elan/bin:${PATH}"

USER tracelean
ENTRYPOINT ["tracelean"]
