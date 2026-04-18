# edition 2024 + let-chains в коде — нужен rustc ≥ 1.88
FROM rust:1.88-bookworm AS builder

WORKDIR /app
# Только Rust-сервер — без Unity `client/`, без DLL и прочего мусора из корня репозитория.
COPY Cargo.toml Cargo.lock ./
COPY server ./server
COPY config.json cells.json ./
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/openmines-server /usr/local/bin/openmines-server
COPY --from=builder /app/config.json /app/config.json
COPY --from=builder /app/cells.json /app/cells.json

RUN chmod +x /usr/local/bin/openmines-server \
 && mkdir -p /data

EXPOSE 8090 8091

# Рабочий каталог /data: config.json + cells.json в корне тома; БД и .mapb — в подкаталоге `data/` (см. config `data_dir`).
CMD ["sh", "-c", "cp -n /app/config.json /data/config.json && cp -n /app/cells.json /data/cells.json && cd /data && mkdir -p data && exec /usr/local/bin/openmines-server"]
