FROM rust:1-bookworm AS builder

WORKDIR /usr/src/spectre

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --bin spectre

FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

RUN mkdir -p /app/maps /app/war3 /app/replays

COPY --from=builder /usr/src/spectre/target/release/spectre /app/spectre

COPY spectre.toml /app/spectre.toml
COPY maps/ /app/maps/
COPY war3/ /app/war3/

ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

EXPOSE 6114/tcp
EXPOSE 6115/tcp
EXPOSE 40000-40150/tcp
EXPOSE 6112/udp

ENTRYPOINT ["/app/spectre"]
