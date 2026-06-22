# rustc 1.94 = toolchain разработки/CI (bevy_ecs 0.18.1 требует ≥ 1.89).
# Держать синхронно с .github/workflows/ci.yml.
FROM rust:1.94-bookworm AS builder

WORKDIR /app
# Только Rust-сервер — без Unity `client/`, без DLL и прочего мусора из корня репозитория.
COPY Cargo.toml Cargo.lock ./

# Предварительная сборка зависимостей (deps layer)
RUN mkdir -p server && echo "fn main() {}" > server/main.rs && \
    cargo build --release && \
    rm -rf server

COPY server ./server
COPY config.json cells.json buildings.json ./
# Каталог миграций нужен на этапе компиляции: `sqlx::migrate!("./migrations")`
# (db/mod.rs) — compile-time макрос, встраивает миграции в бинарь. Без копии
# сборка падает: "error canonicalizing migration directory /app/./migrations".
COPY migrations ./migrations

# Сборка проекта (touch для инвалидации fingerprints после заглушки)
RUN find server -name '*.rs' -exec touch {} + && \
    cargo build --release && \
    cp target/release/openmines-server /usr/local/bin/openmines-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# WORKDIR ОБЯЗАН быть /app, а НЕ /data: configs читаются из рабочего каталога,
# а /data — смонтированный volume (мир/БД). При WORKDIR=/data volume затеняет
# запечённые config.json/cells.json/buildings.json. M3R_DATA_DIR=/data в compose.
WORKDIR /app

COPY --from=builder /usr/local/bin/openmines-server /usr/local/bin/openmines-server
COPY --from=builder /app/config.json ./config.json
COPY --from=builder /app/cells.json ./cells.json
COPY --from=builder /app/buildings.json ./buildings.json

RUN chmod +x /usr/local/bin/openmines-server

EXPOSE 8090 8091

ENTRYPOINT ["/usr/local/bin/openmines-server"]
