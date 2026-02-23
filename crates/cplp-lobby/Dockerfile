# cplp-lobby マルチステージビルド
FROM rust:1.85-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release -p cplp-lobby

# ランタイム
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/cplp-lobby /usr/local/bin/

EXPOSE 3000
CMD ["cplp-lobby"]
