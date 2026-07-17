# cargo-chef автоматически кешируют зависимости без ручного перечисления крейтов.
# rustc 1.96 = toolchain разработки/CI (bevy_ecs 0.18.1 требует ≥ 1.89).
# Держать синхронно с .github/workflows/ci.yml.
FROM rust:1.96-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Кешируем зависимости (пересборка только при изменении Cargo.toml/Cargo.lock)
RUN cargo chef cook --release --recipe-path recipe.json

# Сборка проекта
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY configs ./configs
# Миграции встроены compile-time макросом из crates/openmines-storage/migrations.
RUN cargo build --release --bin openmines-server --bin openmines-proxy && \
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
