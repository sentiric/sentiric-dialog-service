# --- STAGE 1: Chef (Planlama) ---
# DÜZELTME: rand_core v0.10+ ve Edition 2024 desteği için Rust 1.85+ zorunludur.
FROM lukemathwalker/cargo-chef:latest-rust-1.85-bookworm AS chef
WORKDIR /app

# --- STAGE 2: Planner ---
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# --- STAGE 3: Builder ---
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

# Protobuf derleyicisini kur
RUN apt-get update && apt-get install -y protobuf-compiler cmake && rm -rf /var/lib/apt/lists/*

# Önce kütüphaneleri derle (Layer Caching)
RUN cargo chef cook --release --recipe-path recipe.json

# Kaynak kodları kopyala ve derle
COPY . .
RUN cargo build --release --bin sentiric-dialog-service

# --- STAGE 4: Runtime (Minimal) ---
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev \
    curl \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1001 appuser
USER appuser
WORKDIR /app

COPY --from=builder /app/target/release/sentiric-dialog-service /app/

ENV DIALOG_SERVICE_LISTEN_ADDRESS=0.0.0.0
ENV DIALOG_SERVICE_GRPC_PORT=12061
ENV DIALOG_SERVICE_HTTP_PORT=12060
ENV RUST_LOG=info

EXPOSE 12060 12061

ENTRYPOINT ["./sentiric-dialog-service"]