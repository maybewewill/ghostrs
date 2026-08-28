# =============================================================================
# Stage 1: Build binary using official Rust image
# =============================================================================
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/spectre

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary for spectre
RUN cargo build --release --bin spectre

# =============================================================================
# Stage 2: Minimal runtime image
# =============================================================================
FROM debian:bookworm-slim

# Install runtime dependencies (CA certificates for secure connections)
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Create necessary runtime directories
RUN mkdir -p /app/maps /app/war3 /app/replays

# Copy compiled binary from builder stage
COPY --from=builder /usr/src/spectre/target/release/spectre /app/spectre

# Copy default config and map resources
COPY spectre.toml /app/spectre.toml
COPY maps/ /app/maps/
COPY war3/ /app/war3/

# Default Environment Variables
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

# Expose ports:
# 6114: GPS / GProxy Reconnect
# 6115: DotaTV Spectator Relay
# 40000-40150: Multi-Lobby Game Ports
EXPOSE 6114/tcp
EXPOSE 6115/tcp
EXPOSE 40000-40150/tcp
EXPOSE 6112/udp

ENTRYPOINT ["/app/spectre"]
