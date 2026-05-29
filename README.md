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
docker run --rm -p 8090:8090 -v openmines_state:/data openmines-server
```

В образе нужны `config.json`, `cells.json` и `buildings.json`; при первом запуске они копируются в том `/data`. База (`openmines.db`) и слои мира лежат в `/data/data/` (подкаталог по умолчанию `data_dir`). Старые файлы в корне `/data` при старте переносятся в `/data/data/`; при необходимости `mines3.db*` переименовывается в `openmines.db*`.

### 4. Деплой на VPS

Деплой на прод выполняется через GitOps (сервер сам подтягивает изменения из репозитория). Ручные скрипты деплоя из репозитория удалены.

Локально «с нуля»: остановить сервер и удалить каталог состояния, например `rm -rf data/` в корне репозитория (или свой `data_dir` / `M3R_DATA_DIR`).
