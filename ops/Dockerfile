# rustc 1.94 = toolchain разработки/CI (bevy_ecs 0.18.1 требует ≥ 1.89).
# Держать синхронно с .github/workflows/ci.yml.
FROM rust:1.94-bookworm AS builder

WORKDIR /app
# Только Rust-сервер — без Unity `client/`, без DLL и прочего мусора из корня репозитория.
COPY Cargo.toml Cargo.lock ./
COPY crates/openmines-shared/Cargo.toml ./crates/openmines-shared/Cargo.toml
COPY server/Cargo.toml ./server/Cargo.toml
COPY tools/loadtest/Cargo.toml ./tools/loadtest/Cargo.toml
COPY tools/proxy/Cargo.toml ./tools/proxy/Cargo.toml

# Предварительная сборка зависимостей (deps layer). Workspace members должны
# существовать, иначе Cargo падает на резолве путей.
RUN mkdir -p crates/openmines-shared/src server tools/loadtest/src tools/proxy/src && \
    echo "" > crates/openmines-shared/src/lib.rs && \
    echo "fn main() {}" > server/main.rs && \
    echo "fn main() {}" > tools/loadtest/src/main.rs && \
    echo "fn main() {}" > tools/proxy/src/main.rs && \
    cargo build --release --bin openmines-server --bin openmines-proxy && \
    rm -rf crates server tools

COPY crates ./crates
COPY server ./server
COPY tools ./tools
COPY configs ./configs
# Миграции встроены compile-time макросом из crates/openmines-shared/migrations.

# Сборка проекта (touch для инвалидации fingerprints после заглушки).
RUN find crates server tools -name '*.rs' -exec touch {} + && \
    cargo build --release --bin openmines-server --bin openmines-proxy && \
    cp target/release/openmines-server /usr/local/bin/openmines-server && \
    cp target/release/openmines-proxy /usr/local/bin/openmines-proxy

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# WORKDIR ОБЯЗАН быть /app, а НЕ /data: configs читаются из configs/ в рабочем
# каталоге, а /data — смонтированный volume (мир/БД). При WORKDIR=/data volume
# затеняет запечённый configs/. M3R_DATA_DIR=/data в compose.
WORKDIR /app

COPY --from=builder /usr/local/bin/openmines-server /usr/local/bin/openmines-server
COPY --from=builder /usr/local/bin/openmines-proxy /usr/local/bin/openmines-proxy
COPY --from=builder /app/configs ./configs

RUN chmod +x /usr/local/bin/openmines-server /usr/local/bin/openmines-proxy

EXPOSE 8090 8091

ENTRYPOINT ["/usr/local/bin/openmines-server"]
