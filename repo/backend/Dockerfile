FROM rust:1.88-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.toml
RUN mkdir -p src \
 && echo "fn main() {}" > src/main.rs \
 && echo "" > src/lib.rs \
 && cargo build --release \
 && rm -rf src \
           target/release/deps/*fieldops_backend* \
           target/release/fieldops_backend* \
           target/release/.fingerprint/fieldops_backend-*

COPY src ./src
COPY config ./config
COPY logging ./logging
COPY migrations ./migrations
COPY tests ./tests
COPY data ./data

RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/fieldops_backend /usr/local/bin/fieldops_backend
COPY --from=builder /app/migrations /app/migrations
EXPOSE 8080
CMD ["/usr/local/bin/fieldops_backend"]
