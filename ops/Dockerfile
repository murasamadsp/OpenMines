# bevy_ecs 0.18.1 требует rustc ≥ 1.89
FROM rust:1.89-bookworm AS builder

WORKDIR /app
# Только Rust-сервер — без Unity `client/`, без DLL и прочего мусора из корня репозитория.
COPY Cargo.toml Cargo.lock ./

# Предварительная сборка зависимостей (deps layer)
RUN mkdir -p server && echo "fn main() {}" > server/main.rs && \
    cargo build --release && \
    rm -rf server

COPY server ./server
COPY config.json cells.json buildings.json ./

# Сборка проекта (touch для инвалидации fingerprints после заглушки)
RUN find server -name '*.rs' -exec touch {} + && \
    cargo build --release && \
    cp target/release/openmines-server /usr/local/bin/openmines-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /data

COPY --from=builder /usr/local/bin/openmines-server /usr/local/bin/openmines-server
COPY --from=builder /app/config.json ./config.json
COPY --from=builder /app/cells.json ./cells.json
COPY --from=builder /app/buildings.json ./buildings.json

RUN chmod +x /usr/local/bin/openmines-server

EXPOSE 8090

# Запуск сервера напрямую из /data
ENTRYPOINT ["/usr/local/bin/openmines-server"]
