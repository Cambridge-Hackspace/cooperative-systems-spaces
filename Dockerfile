# ==================== FRONTEND BUILD STAGE ====================
# node:24, and glibc rather than alpine. Two reasons, both of which broke this
# build:
#
#   1. Node 20 is below the floor the dependency tree states. jsdom, undici and
#      the @asamuzakjp packages all declare `node: ^22.x || >=24`, and npm
#      reports every one of them as EBADENGINE on 20. CI and e2e/build.sh both
#      use Node 24; this file was the only place that did not.
#   2. package-lock.json carries glibc binaries only -- there is no
#      @esbuild/*-musl entry anywhere in it -- so an alpine (musl) builder has
#      to resolve a platform package the lockfile does not pin.
#
# bookworm-slim also matches what the runtime stage below already is: a Debian.
FROM node:24-bookworm-slim AS frontend-builder

WORKDIR /app/frontend

# Copy package files
COPY frontend/package*.json ./

# Install dependencies
RUN npm ci

# Copy frontend source
COPY frontend/ ./

# Build frontend
RUN npm run build

# ==================== BACKEND BUILD STAGE ====================
# Matching the toolchain everything else builds with. CI uses stable and
# .reaper.toml pins the 1.97 image; 1.90 here was drift that nothing checked,
# and it would surface only on a push to dev.
FROM rust:1.97 AS backend-builder

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    libpq-dev cmake build-essential libasound2-dev libudev-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .

# Build only the server in release mode (skip edge components in dockerfile)
RUN cargo build --release --package css-server --package css-cli

# ==================== RUNTIME STAGE ====================
FROM debian:trixie-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates  \
    vim \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled server binary (with embedded migrations)
COPY --from=backend-builder /app/target/release/css-server /app/server

# Copy frontend build
COPY --from=frontend-builder /app/frontend/dist /app/frontend/dist

# Copy config samples
COPY config.sample.toml /app/config.sample.toml

# Create a non-root user
RUN useradd -m -u 1000 appuser && \
    chown -R appuser:appuser /app

# Create config directory and set permissions
RUN mkdir -p /app/config && \
    chown -R appuser:appuser /app/config

USER appuser

# Set CONFIG_PATH environment variable to point to mountable directory
ENV CONFIG_PATH=/app/config/config.toml
ENV FRONTEND_PATH=/app/frontend/dist

# Expose port
EXPOSE 8080

# Run the server (migrations will run automatically if auto_migrate is enabled in config)
CMD ["/app/server"]
