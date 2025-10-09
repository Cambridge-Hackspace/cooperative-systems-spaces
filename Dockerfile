# ==================== FRONTEND BUILD STAGE ====================
FROM node:20-alpine AS frontend-builder

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
FROM rust:1.90.0 AS backend-builder

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .

# Build server in release mode
RUN cargo build --release

# ==================== RUNTIME STAGE ====================
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates  \
    vi \
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