# OpenMines

Нативный Rust-сервер OpenMines и legacy-клиент.

Короткое описание проекта и запуск.

## Быстрый старт

### 1. Подготовка

1. Установить Rust (1.88+) и Git.
2. Проверить и при необходимости отредактировать `configs/config.json` под
   нужные порты/логи/сеть. Сервер не создаёт этот файл автоматически: отсутствие
   или неполная структура конфигурации — ошибка старта.
3. Состояние сервера (SQLite и файлы мира `*_v2.map`, `*_road_v2.map`,
   `*_durability.map`, `*_world.journal`) по умолчанию пишется в
   каталог из `data_dir` относительно текущей рабочей директории. При первом
   запуске после обновления файлы из старой раскладки (лежали в корне рядом с
   `config.json`) переносятся в `data_dir` автоматически. Переопределение:
   переменная окружения `M3R_DATA_DIR`.

### 2. Запуск сервера локально

```bash
cargo build --release
./scripts/dev-run.sh
```

По умолчанию сервер слушает `0.0.0.0:8090`.

Подробнее о режимах запуска, отладке и использовании встроенной консоли читайте в **[docs/DEBUG.md](file:///Users/murasama/Projects/games/OpenMines/docs/DEBUG.md)**.

### 2.1. Локальная dev-сессия без VPS

Быстрая проверка локального wire-контура без Unity и без VPS:

```bash
./scripts/dev-smoke.sh
```

Smoke поднимает сервер во временной директории с явным минимальным
`configs/config.json`, сбрасывает внешние `M3R_*` override-переменные для
детерминизма и проверяет TCP-последовательность `ST AU PI` + auth-failure
`cf BI HB GU`.

Обычный цикл разработки должен идти против локального сервера и отдельного
каталога состояния:

```bash
./scripts/dev-server.sh
```

Скрипт запускает `openmines-server` на `127.0.0.1:8090`/`0.0.0.0:8090` с
явным локальным конфигом в `.local/openmines-dev/configs/config.json` и
состоянием в `.local/openmines-dev/data`. При запуске `M3R_PORT`,
`M3R_DATA_DIR`, `M3R_USE_CTRL_C`, `M3R_ABORT_ON_PANIC`, `M3R_LOG` и `RUST_LOG`
сбрасываются, чтобы локальный цикл не зависел от внешнего shell-окружения. Удаление `.local/openmines-dev` полностью
сбрасывает локальную сессию.

Unity Editor:

1. Открыть проект `client/`.
2. Выбрать `OpenMines -> Environment -> Local`.
3. Нажать Play.

Клиентский endpoint не задаётся переменными окружения и не угадывается в runtime.
Единственный источник — `Assets/Resources/OpenMines/EnvironmentCatalog.asset`.
Если активный профиль отсутствует, битый или запрещён для текущего типа запуска,
клиент останавливает подключение с явной ошибкой.

Headless production build требует явного профиля:

```bash
./scripts/build-client.sh all production
```

`local`/`dev` профили разрешены для Editor/Development-сценариев, но запрещены
для release build.

Дополнительные опции:

```bash
./scripts/dev-run.sh --regen
# или
./scripts/dev-run.sh --regen-world
M3R_REGEN_WORLD=1 ./scripts/dev-run.sh
```

### 3. Запуск через Docker

```bash
docker build -t openmines-server -f ops/Dockerfile .
docker run --rm -p 8090:8090 -p 8091:8091 \
  -e M3R_ADMIN_TOKEN=local-dev-admin \
  -v openmines_state:/data \
  openmines-server
```

Порт `8090` — игровой протокол, `8091` — admin web server (`/metrics` тоже
там). `M3R_ADMIN_TOKEN` обязателен: без него сервер падает fail-fast при
старте admin web. Конфиги (`config.json`, `cells.json`, `buildings.json`)
запекаются в образ в рабочий каталог `/app` и читаются оттуда; том `/data`
(`M3R_DATA_DIR`) хранит только состояние — базу (`openmines.db`) и слои мира в
`/data/data/`. Поэтому `WORKDIR` образа — `/app`, а не `/data` (иначе том
затенил бы запечённые конфиги).

### 4. Деплой (CI/CD)

Деплой автоматизирован через GitHub Actions:

1. Push в `main` → CI прогоняет `cargo fmt`, строгий `clippy` и тесты.
2. При зелёном CI собирается Docker-образ (`ops/Dockerfile`) и публикуется в GitHub Container Registry (GHCR).
3. Образ выкатывается на сервер: бэкап тома состояния → `docker compose pull` → `up -d` (без `down`, том мира сохраняется).

Параметры окружения (хост, пути, ключи) хранятся в секретах репозитория и не присутствуют в исходниках.

Локально «с нуля»: остановить сервер и удалить каталог состояния, например
`rm -rf .local/openmines-dev` для локальной dev-сессии.
