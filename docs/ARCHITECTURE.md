# Архитектура проекта

Этот файл описывает фактическую topology текущего кода. Текущая готовность,
runtime evidence и следующий срез находятся в
[`SERVER_MIGRATION_STATUS.md`](../SERVER_MIGRATION_STATUS.md); целевая модель - в
[`SIMULATION_KERNEL_PLAN.md`](../SIMULATION_KERNEL_PLAN.md).

## Общая модель

Сервер строится по модульной модели:

- `openmines-protocol` — wire-контракт legacy Unity-клиента: бинарный фрейм, packet builders/decoders, HB sub-packets, golden-тесты.
- `openmines-core` — лёгкие доменные value objects: player/world ids, позиции, роли, ранги и direction helpers; без tokio/sqlx/bevy.
- `openmines-config` — обязательные fail-fast runtime-конфиги и их validation.
- `openmines-world` — mmap-слои мира, cells, durability, journal/checkpoint и worldgen.
- `openmines-storage` — SQLite/sqlx, row-типы и миграции.
- `openmines-runtime` — runtime-инструменты: logging, metrics, env parsing, wall-clock helpers.
- `openmines-server` — игровой сервер: ECS-геймплей, TCP-сессии, HTTP-админка, фоновые задачи.
- `openmines-loadtest` — нагрузочный клиент поверх общего protocol/storage API.
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
   - `tasks::simulation::spawn_game_tick_loop`: отдельный event-driven OS-поток
     authoritative simulation (см. ниже).
   - `net::web::run_web_server`: HTTP admin server на `M3R_ADMIN_PORT`/`--admin-port`.
   - `console::run_repl`: интерактивная server console.

## Event-driven simulation owner

Игровая логика выполняется в выделенном `std::thread` вне tokio-рантайма. Это
отделяет authoritative apply от Tokio executor и позволяет использовать обычный
синхронный Bevy ECS path. OS всё равно может вытеснить поток; dedicated thread не
является real-time гарантией.

### Текущие runtime owners

```text
TCP sessions -> QueuedGameCommand -> SimulationRuntime
      ^                                  |       |
      |                           GameEvent    SaveCommand
      |                                  |       |
      +---------------- PresentationRuntime  PersistenceRuntime
```

- `SessionHub` владеет `SessionId`, player/session mapping и bounded outbox каждого
  соединения.
- `SimulationRuntime` владеет command receiver, schedule clock, bounded
  `DueActionQueue`, pending backlogs и event-driven owner wait. Owner паркуется,
  пока нет runnable work или ближайшего deadline; command, persistence progress
  и shutdown будят его через lost-wake-safe `SimulationWaker`. Boom, Protector и
  Raz используют единый stable deadline/admission/apply/effects contract.
  Authoritative ECS пока находится в `Arc<GameState>` под `RwLock`; это
  переходное состояние, а не целевая граница.
- `PresentationRuntime` принимает bounded `GameEvent` и доставляет immutable
  packet/view data. Initial chunk map/BotSpot snapshot уже строится там после
  owner-side visibility commit; building overlay временно читает ECS только как
  read-only snapshot по одному чанку до следующего per-chunk read-model slice.
- `PersistenceRuntime` принимает bounded `SaveCommand`, batch-ит совместимые
  записи, делает retry и публикует typed completion.

TY frame разбирается в session adapter до enqueue. `GameCommand` содержит typed
variants (`Move`, `Dig`, `Build`, `Gui`, `InventoryUse`, `ProgramAction` и
другие), а не сырой `Ty` payload. `QueuedGameCommand` несёт общий authenticated
envelope (`player_id`, `session_id`, `GameCommand`) для всех client actions;
внутренний `PlayerCommand` не повторяет identity.

Command ingress разделён на bounded классы lifecycle, gameplay и internal.
Каждый имеет отдельные capacity и item budget на active cycle; gameplay при full
отклоняется до mutation с видимым legacy-safe ответом, lifecycle и internal
await bounded admission. Метрики экспортируют depth, oldest age, residence и
rejection по классу. Pending durable head одного класса не блокирует runnable
work другого, FIFO внутри класса сохраняется.

Graceful shutdown переходит в `Quiescing`: внешний ingress закрывается, уже
буферизированные команды и accepted `DueAction` дренятся до реальных deadline,
а Protector/Raz building-removal follow-up остаётся в owner-local delete FIFO до
persistence admission. После этого simulation закрывает последний
`PersistenceHandle`, применяет все completion и только затем допускает final
flush. Crash durability это не заменяет.

### Переходные обходы

Не все gameplay flows уже используют owners выше. В коде ещё остаются:

- direct DB tasks для части GUI/auction операций; создание программы идёт через
  `SaveCommand::ProgramCreate` и typed completion;
- `channel_chat` всё ещё использует transitional session/async path и в ручном
  trace дал `201ms` CPU-bound dispatch;
- periodic dirty flush использует owner-local deduplicated entity registries и
  не сканирует players/buildings;
- `PlayerInit` chunk snapshot, wire encode и delivery принадлежат presentation
  owner; simulation `Connect` делает только entity/index apply и фиксирует
  visible-chunk list под session guard;
- programmator исполняется через entity-aware due heap, не periodic player scan;
- standing-cell hazards используют deduplicated due deadlines: safe idle player
  не запускает ECS schedule, непустая клетка повторно планирует только себя;
- granular physics запускается только по pending/active frontier: position
  transition seed-ит region, cell transition будит локальную область;
- alive cells используют exact active registry: player window scan выполняется
  только на position transition, а пустой batch останавливает schedule;
- periodic `bots_render` использует immutable player/BotSpot spatial cache:
  короткая active-player сверка отделена от visibility walk и `HB/X` encode,
  поэтому renderer не берёт ECS lock;
- scheduler берёт ECS write-lock на один runnable schedule и отдельный короткий
  tail, чтобы preemption одного job не блокировал всю schedule phase;
- movement делает chunk snapshot только при crossing chunk boundary; обычный
  intra-chunk `Xmov` не трогает `PlayerView`/chunk sync;
- внешние ECS writers из admin/web/session/shutdown;
- legacy handlers, которые мутируют state и отправляют wire в одном вызове.

Новый код не должен копировать эти paths. Исполняемый порядок их удаления
находится только в `SERVER_MIGRATION_STATUS.md`; целевые gates - в
`SIMULATION_KERNEL_PLAN.md`, единая форма feature-кода - в
`SERVER_CONSISTENCY_PLAN.md`.

### Параметры active cycle и schedules

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

- `game_loop_tick_rate_ms` — budget одного active simulation cycle. Он ограничивает
  command/due carry-over и остаётся порогом `OVER-BUDGET`, но не задаёт частоту
  пустых тиков: idle owner ждёт событие или deadline. Должен быть > 0, иначе
  старт завершается ошибкой.
- `game_loop_panic_backoff_ms` — legacy-совместимое валидируемое значение. Текущий
  simulation owner после panic завершает процесс с кодом `101`, потому что
  продолжение на потенциально повреждённом ECS запрещено; backoff не применяется.
- `schedule_warn_threshold_ms` — порог warning для одного ECS schedule. Должен быть >= `game_loop_tick_rate_ms`.

### Диагностика тиков (`target=tickprof`, `target=scheduler`)

Раз в 5 секунд, при следующем active sample, в `debug` пишется сводка по
максимальным значениям за временное окно:

```
[tickprof] ticks=100 over_budget=3 max_total=12.3ms
  dispatch=1.1ms schedule=9.8ms side=1.0ms actions=47
  side: broadcasts=0.4ms pack_resends=0.2ms ...
```

`tickprof` выводит over-budget только при выходе за бюджет
(`dt_total > game_loop_tick_rate_ms`), не чаще раза в 500ms. Запись содержит
`thread_cpu`, вычисленный `off_cpu` и `execution_class`: `cpu_bound`, `mixed` или
`preempted`. Чистая host preemption остаётся throttled `DEBUG`, а CPU-bound и
mixed stalls остаются `WARN`.

Обычная 5-секундная `tickprof` summary не должна шуметь в prod при `info`.
Для расследования performance включать явно: `M3R_LOG=tickprof=debug`.

`scheduler` warning выводится, когда весь ECS schedule выполнялся дольше
`schedule_warn_threshold_ms`. Detailed `SLOW hazards system` использует более
чувствительный порог
`min(schedule_warn_threshold_ms, game_loop_tick_rate_ms)`, сейчас `10ms`, и
показывает внутренние фазы hazards. Поэтому hazards `5ms` не логируется, а
`10-20ms` уже может дать system detail без `50ms` scheduler warning.

Game loop watchdog работает из отдельного OS-потока и не ждёт завершения cycle.
Если active owner не обновляет heartbeat дольше
`max(200 * game_loop_tick_rate_ms, 2s)`,
он пишет `GAME TICK WATCHDOG` с последней стадией (`dispatch`, `schedule_run`,
`flush_queues`, `side_*`), именем schedule, номером тика, числом игроков и
количеством pending DB tasks. Это нужно именно для зависаний без последующего
`5s summary`: обычный slow-log срабатывает только после возврата управления.
Indefinite idle park не является зависанием; timed park становится ошибкой
watchdog только после пропущенного deadline и того же tolerance.
Стадия `idle` означает, что owner не вернулся из timed park к deadline; без
отдельного deadlock detector это evidence off-CPU stall, а не доказательство
циклической блокировки.
Если стадия оканчивается на `_ecs_lock_wait`, тик стоит не внутри работы этой
секции, а на ожидании ECS write-lock. Это может быть обычный contention или
preemption владельца lock. Deadlock backtrace релевантен только если detector
отдельно напечатал `PARKING_LOT DEADLOCK DETECTED`.

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

Будущий долг admin control plane: вынести исполнение команд из
`net/session/social/commands.rs` и `console.rs` в общий admin command service.
Web должен быть GUI над тем же service, а не четвёртой реализацией правил.
Когда делать этот срез, определяет только `SERVER_MIGRATION_STATUS.md`.

---

## Внешние интерфейсы и интеграция

### Сетевые интерфейсы

#### 1. Игровой протокол (TCP)
Единая точка входа для игровых клиентов:
- `0.0.0.0:<port>` (`port` из `configs/config.json`).
- Бинарный протокол описан в [PROTOCOL.md](PROTOCOL.md).

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
