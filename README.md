# OpenMines

Нативный Rust-сервер OpenMines и legacy-клиент.

Короткое описание проекта и запуск.

## Быстрый старт

### 1. Подготовка

1. Установить Rust (1.88+) и Git.
2. Скопировать шаблон конфигурации:

```bash
cp config.example.json config.json
```

3. Отредактировать `config.json` под локальные порты/логи/сеть.
4. Состояние сервера (SQLite и файлы мира `.mapb`) по умолчанию пишется в каталог `data/` относительно текущей рабочей директории (`data_dir` в конфиге). При первом запуске после обновления файлы из старой раскладки (лежали в корне рядом с `config.json`) переносятся в `data/` автоматически. Переопределение: переменная окружения `M3R_DATA_DIR`.

### 2. Запуск сервера локально

```bash
cargo build --release
cargo run --release
```

По умолчанию сервер слушает `0.0.0.0:8090`.

Дополнительные опции:

```bash
cargo run --release -- --regen
# или
cargo run --release -- --regen-world
M3R_REGEN_WORLD=1 cargo run --release
```

### 3. Запуск через Docker

```bash
docker build -t openmines-server -f ops/Dockerfile .
docker run --rm -p 8090:8090 -p 8091:8091 -v openmines_state:/data openmines-server
```

Порт `8090` — игровой протокол, `8091` — метрики Prometheus. Конфиги (`config.json`, `cells.json`, `buildings.json`) запекаются в образ в рабочий каталог `/app` и читаются оттуда; том `/data` (`M3R_DATA_DIR`) хранит только состояние — базу (`openmines.db`) и слои мира в `/data/data/`. Поэтому `WORKDIR` образа — `/app`, а не `/data` (иначе том затенил бы запечённые конфиги).

### 4. Деплой (CI/CD)

Деплой автоматизирован через GitHub Actions:

1. Push в `main` → CI прогоняет `cargo fmt`, строгий `clippy` и тесты.
2. При зелёном CI собирается Docker-образ (`ops/Dockerfile`) и публикуется в GitHub Container Registry (GHCR).
3. Образ выкатывается на сервер: бэкап тома состояния → `docker compose pull` → `up -d` (без `down`, том мира сохраняется).

Параметры окружения (хост, пути, ключи) хранятся в секретах репозитория и не присутствуют в исходниках.

Локально «с нуля»: остановить сервер и удалить каталог состояния, например `rm -rf data/` в корне репозитория (или свой `data_dir` / `M3R_DATA_DIR`).
