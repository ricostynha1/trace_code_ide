# TraceLean IDE - Development Build Container
# Multi-stage: builds both Rust backend and React frontend

FROM rust:1.82-bookworm AS builder

# Install system deps for Tauri (Linux)
RUN apt-get update && apt-get install -y \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libdbus-1-dev \
    libssl-dev \
    pkg-config \
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

# Copy Rust manifest for dep caching
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock* src-tauri/
COPY src-tauri/build.rs src-tauri/
# Create dummy src to cache deps
RUN mkdir -p src-tauri/src && \
    echo 'fn main() {}' > src-tauri/src/main.rs && \
    echo 'pub fn run() {}' > src-tauri/src/lib.rs && \
    cd src-tauri && cargo fetch

# Copy full source
COPY . .

# Build frontend
RUN npm run build

# Build Rust backend
RUN cd src-tauri && cargo build --release

# --- Runtime stage ---
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    librsvg2-2 \
    libsoup-3.0-0 \
    libjavascriptcoregtk-4.1-0 \
    libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/src-tauri/target/release/tracelean /usr/local/bin/tracelean

ENTRYPOINT ["tracelean"]
