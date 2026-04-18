# bevy_ecs 0.18.1 требует rustc ≥ 1.89
FROM rust:1.89-bookworm AS builder

WORKDIR /app
# Только Rust-сервер — без Unity `client/`, без DLL и прочего мусора из корня репозитория.
COPY Cargo.toml Cargo.lock ./

# Предварительная сборка зависимостей с кэшированием
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY server ./server
COPY config.json cells.json buildings.json ./

# Сборка проекта с кэшированием
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/openmines-server /usr/local/bin/openmines-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/local/bin/openmines-server /usr/local/bin/openmines-server
COPY --from=builder /app/config.json /app/config.json
COPY --from=builder /app/cells.json /app/cells.json
COPY --from=builder /app/buildings.json /app/buildings.json

RUN chmod +x /usr/local/bin/openmines-server \
 && mkdir -p /data

EXPOSE 8090

# Рабочий каталог /data: config.json + cells.json + buildings.json в корне тома; БД и .mapb — в подкаталоге `data/` (см. config `data_dir`).
CMD ["sh", "-c", "cp -n /app/config.json /data/config.json && cp -n /app/cells.json /data/cells.json && cp -n /app/buildings.json /data/buildings.json && cd /data && mkdir -p data && exec /usr/local/bin/openmines-server"]
