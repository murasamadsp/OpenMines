# Архитектура проекта

## Общая модель

Сервер строится по модульной модели:

- `openmines-protocol` — wire-контракт legacy Unity-клиента: бинарный фрейм, packet builders/decoders, HB sub-packets, golden-тесты.
- `openmines-shared` — общие серверные библиотеки: обязательные конфиги, SQLite, мир, логирование, метрики, время.
- `openmines-server` — игровой сервер: ECS-геймплей, TCP-сессии, HTTP-админка, фоновые задачи.
- `openmines-loadtest` — нагрузочный клиент поверх общего protocol/db API.
- `openmines-proxy` — TCP-прокси поверх общего protocol decoder.

## Поток запуска

1. **Загрузка конфигов**: Читаются обязательные `configs/config.json`, `configs/cells.json` и `configs/buildings.json`.
2. **Инициализация логов**: Запускается `tracing`.
3. **Разрешение путей**: Директория состояния берётся из непустого `M3R_DATA_DIR` или из `data_dir` в `configs/config.json`; пустое значение — ошибка старта.
4. **Миграции**: Сервер переносит старые файлы `mines3.db` в `openmines.db`.
5. **Подготовка мира**: Инициализация `World`. Если передан флаг `--regen`, удаляются файлы `.map` и старые постройки из БД.
6. **Подготовка БД**: Выполнение SQL-миграций. Функция `bootstrap_grant_admin` выдаёт права администратора по списку из `M3R_GRANT_ADMIN`.
7. **Запуск сервисов**:
   - `net::run`: Основной TCP-сервер игрового протокола.
   - `game::run_game_loop_thread`: Отдельный OS-поток игрового тика (см. ниже).
   - `net::web::run_web_server`: HTTP admin server на `M3R_ADMIN_PORT`/`--admin-port`.
   - `console::run_repl`: интерактивная server console.

## Игровой цикл (Game Loop Thread)

Игровая логика выполняется в выделенном `std::thread` — **вне** tokio-рантайма. Это гарантирует:
- предсказуемый тайминг тика без влияния tokio-планировщика;
- отсутствие `async` overhead в горячем пути;
- возможность использовать `bevy_ecs` без Send/Sync оговорок.

### Канал команд (`PlayerCommand`)

Сетевые сессии (tokio-задачи) передают события в game-поток через `tokio::sync::mpsc::unbounded_channel`:

```
TCP session (tokio task)
    │  PlayerCommand::Connect { .. }
    │  PlayerCommand::Ty { pid, data }
    │  PlayerCommand::Disconnect { pid }
    ▼
commands_tx (Arc<Sender>) — non-blocking send
    │
commands_rx (внутри game thread)
    ▼
run_game_tick_sync()  — читает все накопленные команды за 1 тик
```

Типы команд (`contracts.rs`):
- `Connect` — новое соединение, передаёт полные данные строки игрока из БД.
- `Disconnect` — игрок отключился.
- `Ty { pid, data }` — TY-пакет от клиента (движение, копание, строительство, чат и т.д.).

### Sync/Async мост

Game-поток синхронный, но ему нужно писать в БД (async SQLite). Для этого в `GameState` хранится `tokio::runtime::Handle`:

```rust
state.tokio_handle.spawn(async move {
    db.save_player(pid, snapshot).await
});
```

Это позволяет делегировать async-операции в tokio threadpool без блокировки game-потока.

### Конфигурируемые параметры тика

Тикрейт и поведение при панике задаются в `config.json`:

```json
"gameplay": {
  "schedules": {
    "game_loop_tick_rate_ms": 10,
    "game_loop_panic_backoff_ms": 200,
    "schedule_warn_threshold_ms": 50
  }
}
```

- `game_loop_tick_rate_ms` — целевая длительность одного тика в миллисекундах. Если тик выполнен быстрее — поток засыпает на остаток. Должен быть > 0, иначе старт завершается ошибкой.
- `game_loop_panic_backoff_ms` — задержка перед следующим тиком после паники (ECS может быть в неконсистентном состоянии). Должен быть > 0.
- `schedule_warn_threshold_ms` — порог warning для одного ECS schedule. Должен быть >= `game_loop_tick_rate_ms`.

### Диагностика тиков (`target=tickprof`, `target=scheduler`)

Каждые N тиков в `debug` пишется сводка по максимальным значениям за окно:

```
[tickprof] ticks=100 over_budget=3 max_total=12.3ms
  dispatch=1.1ms schedule=9.8ms side=1.0ms actions=47
  side: broadcasts=0.4ms pack_resends=0.2ms ...
```

`tickprof` выводит over-budget только при выходе за бюджет
(`dt_total > game_loop_tick_rate_ms`), не чаще раза в 500ms.

Обычная 5-секундная `tickprof` summary не должна шуметь в prod при `info`.
Для расследования performance включать явно: `M3R_LOG=tickprof=debug`.

`scheduler` warning и detailed `SLOW hazards system` выводятся, когда ECS
schedule выполнялся дольше `schedule_warn_threshold_ms`. Это отсекает шум от
обычных 5-20ms physics/hazards проходов при 10ms tick budget, но оставляет
видимыми реальные стопоры вроде 80ms schedule. Для каждого превышения лог пишет
фактическую длительность и configured threshold.

Game loop watchdog работает из отдельного OS-потока и не ждёт завершения тика.
Если tick-loop не обновляет heartbeat дольше `max(200 * game_loop_tick_rate_ms, 2s)`,
он пишет `GAME TICK WATCHDOG` с последней стадией (`dispatch`, `schedule_run`,
`flush_queues`, `side_*`), именем schedule, номером тика, числом игроков и
количеством pending DB tasks. Это нужно именно для зависаний без последующего
`5s summary`: обычный slow-log срабатывает только после возврата управления.
Если стадия оканчивается на `_ecs_lock_wait`, тик стоит не внутри работы этой
секции, а на ожидании ECS write-lock; в таком случае смотреть надо backtrace
`PARKING_LOT DEADLOCK DETECTED`.

Отдельно включён `parking_lot` deadlock detector. Он каждые 10s проверяет
циклические deadlock-и `parking_lot::{Mutex,RwLock}` и при обнаружении пишет
`PARKING_LOT DEADLOCK DETECTED` с thread id и backtrace каждого участника. Это
не заменяет watchdog: detector ловит только lock-циклы, watchdog ловит любой
стопор game-tick'а.

## Управление данными (State)

- `openmines.db` — единая SQLite БД для персистентных данных.
- `*.map` — бинарные файлы-дампы чанков (кэш мира).
- Все данные сохраняются в директорию состояния, что упрощает бэкап и миграцию.

## Взаимодействие с клиентом

Сетевой протокол реализован в `crates/openmines-protocol`. Каждый подключённый клиент получает `Session`, которая обрабатывает входящие пакеты и синхронизирует состояние (игроки, инвентарь, мир).

## Admin Control Plane

Админ-возможности имеют три поверхности: in-game slash-команды, интерактивная
server console и web admin. Источник правды по списку команд — единый registry
`crates/openmines-server/src/admin/mod.rs`.

- in-game `/admin` строит help из registry;
- console `help` строит help из registry;
- web отдаёт тот же registry через `GET /api/admin/commands`.

Следующий архитектурный шаг: вынести исполнение команд из
`net/session/social/commands.rs` и `console.rs` в общий admin command service.
Web должен быть GUI над тем же service, а не четвёртой реализацией правил.

---

## Внешние интерфейсы и интеграция

### Сетевые интерфейсы

#### 1. Игровой протокол (TCP)
Единая точка входа для игровых клиентов:
- `0.0.0.0:<port>` (`port` из `configs/config.json`).
- Бинарный протокол описан в [PROTOCOL.md](file:///Users/murasama/Projects/games/OpenMines/docs/PROTOCOL.md).

### Управляющие файлы

- `configs/config.json` — обязательный runtime-конфиг (логирование, порт, размеры мира). Валидируется на старте: `port` не может быть `0`, размеры мира должны быть больше `0`, строковые пути/имена не могут быть пустыми.
- `configs/cells.json` — обязательное описание типов клеток мира.
- `configs/buildings.json` — обязательная конфигурация построек. Валидируется на старте: список не может быть пустым, `code` обязан быть ровно одним ASCII-байтом и уникальным, параметры стоимости/заряда/HP не могут быть отрицательными или противоречивыми, footprint клеток не может быть пустым или иметь дубли координат.
- `openmines.db` — SQLite-база данных. При старте сервер автоматически мигрирует старый файл `mines3.db` в `openmines.db`, если он существует. Записи `buildings.data` читаются строго: `NULL`, пустой, битый или неполный JSON — ошибка загрузки состояния.
- `players.inventory` и `players.skills` в SQLite читаются строго: битый JSON — ошибка загрузки игрока. Для `players.skills` допускается только явная миграция старого map-формата в `SkillSlots`.
- Ошибки SQLite не маскируются под пустые списки или дефолтные значения в критичных пользовательских состояниях: `chat_color`, запись chat-сообщений, private-chat target lookup, список программ, клановые меню/заявки/приглашения/участники должны возвращать явную ошибку.
- `*.map` — бинарные дампы чанков мира в директории состояния.

### Переменные окружения (M3R_*)

- `M3R_DATA_DIR` — явный непустой путь к директории состояния (БД, карты). Если не задан, используется `data_dir` из `configs/config.json`.
- `M3R_REGEN_WORLD=1` — полная регенерация мира при старте (удаляет `.map` и постройки из БД). Допустимые bool-значения: `1,true,yes,on,0,false,no,off`; иное значение — ошибка старта.
- `M3R_USE_CTRL_C` — включает/выключает обработку Ctrl+C в shutdown-сигналах. Допустимые bool-значения: `1,true,yes,on,0,false,no,off`; иное значение — ошибка старта.
- `M3R_ABORT_ON_PANIC` — завершать процесс с кодом `101` после panic hook. Допустимые bool-значения: `1,true,yes,on,0,false,no,off`; иное значение — ошибка старта.
- `M3R_GRANT_ADMIN=name1,name2` — автоматически выдать права администратора указанным игрокам.
- `M3R_ADMIN_PORT` — порт HTTP admin server; default `8091`. Не должен совпадать с игровым TCP-портом.
- `M3R_ADMIN_TOKEN` — обязательный токен HTTP admin server. Без него обычный старт сервера падает fail-fast; `--doctor` только валидирует состояние и не требует токен.

### Аргументы запуска

- `--regen` / `--regen-world` — аналогично `M3R_REGEN_WORLD`.
- `--admin-port` — аналогично `M3R_ADMIN_PORT`.
- `--admin-token` — аналогично `M3R_ADMIN_TOKEN`.
