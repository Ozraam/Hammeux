# syntax=docker/dockerfile:1

# 1. Client web
FROM node:24-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# 2. Serveur Rust (cargo-chef met les dépendances en cache entre deux mises à jour)
FROM rust:1-slim-trixie AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p hameaux-server
COPY . .
RUN cargo build --release -p hameaux-server

# 3. Image finale : le binaire et le client compilé
FROM debian:trixie-slim
RUN useradd --system --uid 10001 --home-dir /data hameaux && mkdir -p /data && chown hameaux /data
COPY --from=builder /app/target/release/hameaux-server /usr/local/bin/hameaux-server
COPY --from=web /web/dist /app/web
ENV HAMEAUX_BIND=0.0.0.0:8080 \
    HAMEAUX_DATA_DIR=/data \
    HAMEAUX_WEB_DIR=/app/web \
    RUST_LOG=info
USER hameaux
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s CMD ["hameaux-server", "healthcheck"]
STOPSIGNAL SIGTERM
CMD ["hameaux-server"]
