# OpenMines Server Migration Status

Обновлено: 2026-07-13.

Это **единственный актуальный checkpoint и handoff** по миграции сервера.
Подробная целевая модель находится в `SIMULATION_KERNEL_PLAN.md`; правила формы
кода - в `SERVER_CONSISTENCY_PLAN.md`; фактическая topology - в
`docs/ARCHITECTURE.md`. Старые приоритеты в `TODO.md` и `AUDIT_STATE.md` не
являются планом simulation migration.

## Как продолжить работу

Новый агент сначала выполняет только read-only проверки:

```bash
git status --short --branch
git log -1 --oneline
git diff --check
```

Ожидаемый code checkpoint: `d6570b19` (`Перевести команды и создание программ
на simulation pipeline`) в `main`. Документационный checkpoint может быть
более новым.

Дальше:

1. Прочитать разделы `Что горит`, `Текущий кодовый срез` и `Запрещённые
   решения` ниже.
2. Текущий срез - убрать periodic `PlayerEntities` work для sleeping
   alive/granular actors через explicit active frontier. Не смешивать его с ECS
   ownership, chat optimization или multicore.
3. Перед правкой проверить указанные функции в текущем коде: номера строк могут
   сдвинуться, имена и invariants важнее номера.
4. После среза обновить этот файл в том же commit. Не создавать новый handoff.

## Проверенный checkpoint

На code checkpoint `d6570b19` зелёные:

- pre-commit doctor, architecture guard, dependency policy и security audit;
- strict clippy для всех targets/features;
- `585/585` tests в `cargo-nextest`, `2` skipped;
- отдельный server suite: `364 passed`, `1 ignored` benchmark;
- `scripts/dev-smoke.sh`: auth, gameplay, building/admin, programmator,
  settings и reconnect wire flows;
- rustfmt и `git diff --check`;
- release idle runtime и один SIGINT без дополнительного Enter.

Эти gates не нужно слепо перезапускать перед чтением кода. После изменения
сначала запускаются targeted tests, полный pre-commit - один раз перед commit.

## Состояние на одном экране

Ориентировочно выполнено **35-40% архитектурной миграции**. Это не процент строк
кода и не обещание срока. Главные ownership boundaries уже появились, но самые
тяжёлые этапы - удаление внешних ECS writers, active/due registries, interest
read model и spatial multicore - ещё впереди.

Первый измеримый performance milestone закрыт:

- fixed idle loop `100 Hz` удалён;
- release, `0 players`, контрольные `120s`: `15` active cycles вместо
  теоретических `12 000` fixed ticks;
- process CPU за окно: `0.14s`; текущий CPU: `0.0%`;
- idle `OVER-BUDGET` и watchdog warnings: `0`;
- один SIGINT полностью завершает simulation, persistence и world flush без
  дополнительного Enter.

Это не означает, что сервер уже дешёвый при одном игроке или готов к огромной
нагрузке. Connect, periodic actor systems и global ECS lock ещё видимы в runtime.

### Последние ручные runtime-наблюдения

Логи ниже предоставлены пользователем 2026-07-12 и не воспроизводились агентом:

- connect: `18.31ms wall`, `4.98ms thread CPU`, `13.33ms off-CPU`, command
  `16.00ms`; это реальная latency, но не `16ms` чистых вычислений;
- `channel_chat`: `201.75ms` command, tick `188.30ms thread CPU`; это настоящий
  CPU-bound active-path hotspot, а не host preemption;
- `hazards`: полезный lookup занял `17.46us`, но `36.74ms` остались
  unaccounted; одновременно ECS write lock удерживался до `53.86ms`. Этот лог не
  доказывает дорогой hazard lookup, зато снова показывает цену global lock.

Вывод: M1 закрыл только нулевой idle. Active command paths, один idle player и
lock isolation не закрыты. `channel_chat` должен позже пройти тот же typed
command/apply/persistence/effects boundary, а не получить слепой локальный
микрофикс по одному trace.

## Это движок или нет

Цель - **внутренний simulation kernel OpenMines**, а не универсальный продуктовый
движок. Engine-like primitives нужны игре: ownership, deadlines, admission,
effects, persistence, spatial indexes и deterministic replay.

Отдельный reusable engine сейчас не является целью. Без второго реального
потребителя пришлось бы заранее обобщать transport, persistence, plugins,
scripting, schemas, tooling и versioning. Это увеличит работу и когнитивную цену,
но не докажет полезность abstractions.

Правило extraction:

- game-specific kernel оптимизируется под OpenMines;
- независимые стабильные слои остаются отдельными crates;
- reusable API извлекается только после второго доказанного одинакового
  contract, а не ради абстрактной SOLID-формы.

## Future vector: singleplayer и offline fast-forward

Технически legacy Unity-клиент может запускать Rust-сервер локальным child
process и продолжать использовать тот же TCP wire. Постоянная OS-служба для
singleplayer не нужна: сервер стартует вместе с игрой и штатно завершается после
неё.

Возврат через восемь часов нельзя реализовывать прокруткой миллионов пустых
тиков. Нужны durable wall-clock deadlines/intents и тот же simulation apply-path:

```text
load snapshot
    -> restore durable due intents
    -> advance explicit clock to next event
    -> apply events in stable order with explicit RNG seed
    -> persist final snapshot
```

Это одновременно будущая feature и сильный тест архитектуры. Одинаковые
snapshot, ordered inputs, clock timeline и RNG seed должны давать одинаковый
state/event digest при real-time `1x`, accelerated replay и resume после
остановки. Расхождение обнаружит скрытый wall clock, unordered iteration,
scheduler-dependent mutation, direct IO или RNG вне owner boundary.

Не обещать буквальное `1:1` offline поведение, пока механики не переведены на
event/due model и не появился injected simulation clock. Простые независимые
процессы можно считать аналитически; взаимодействующие акторы требуют
детерминированного event replay с budgets. Это milestone после owned simulation
и durable due intents, не текущий shutdown-срез.

## Карта этапов

Проценты по строкам не складываются в общий процент. Они показывают зрелость
конкретного архитектурного слоя.

| Этап | Готовность | Фактическое состояние |
| --- | ---: | --- |
| 0. Evidence и guards | 75% | release traces, CPU/off-CPU classification, strict clippy, architecture guard; одинаковый benchmark обязателен не для каждого среза |
| 1. Session/output owner | 75% | `SessionId`, bounded outbox, `SessionHub`, presentation-owned PlayerInit и common authenticated envelope есть |
| 2. Command/effects boundary | 40% | connect/disconnect, move, teleport-open, delayed consumables и building delete перенесены; GUI/economy/chat/clan/admin ещё имеют bypass |
| 3. Persistence owner | 50% | bounded writer, batching, retry, writer drain и `ProgramCreate` completion есть; GUI/auction bypass и crash journal остаются |
| 4. Admission/isolation | 55% | event-driven wait, bounded due queue, typed bounded ingress и thin connect готовы |
| 5. Owned simulation | 15% | runtime владеет clocks/receivers/backlogs, но ECS и indexes остаются в `Arc<GameState>` под глобальным `RwLock` |
| 6. Active/due work | 40% | granular frontier, crafting/consumable/programmator/guns due queues и dirty registries есть; actor systems ещё частично scan-all |
| 7. Interest/read model | 10% | teleport DTO и часть immutable presentation готовы; bots render и admin всё ещё читают общий state |
| 8. Spatial multicore | 0% | Rayon analysis не является ownership sharding; deterministic 1/2/4-worker model ещё не начат |

## Что реально сделано

### Runtime ownership

- `SessionHub` владеет живыми session mappings и bounded per-session outbox.
- `PresentationRuntime` доставляет перенесённые immutable effects вне
  authoritative mutation.
- `PersistenceRuntime` является bounded writer для перенесённых durable flows.
- `SimulationRuntime` владеет command receiver, schedule clock, due queue и
  pending backlogs.
- `lifecycle.rs` и `tick.rs` больше не являются единственными god-файлами:
  scheduler, profiler, effects, snapshots, commands, due и wait разделены по
  ответственности.

### Event-driven simulation

- Owner ждёт command, persistence progress/completion или ближайший
  due/schedule/maintenance deadline.
- Spurious wake только повторяет plan и не создаёт пустой tick.
- Wait находится вне tickprof wall/CPU boundary.
- Indefinite idle не считается зависанием; пропущенный timed deadline остаётся
  видим watchdog.
- `10ms` теперь active-cycle budget, а не частота пустого loop.

### Delayed gameplay

- Boom, Protector и Raz используют один bounded `DueActionQueue`.
- Admission резервирует capacity до cooldown/item/pack mutation.
- Apply выполняется simulation owner и возвращает typed effects без
  `tokio::spawn + sleep`, DB и wire.
- Deadline key стабилен: `(due_at, admission_sequence)`.
- Protector/Raz используют spatial candidates; Raz не сканирует все здания.

### Persistence и world

- Disconnect, bonus, program save, box writes и building delete используют
  bounded admission там, где вертикальные срезы завершены.
- Building delete имеет operation ID, ABA guard, completion и atomic DB-side
  Resp/Box cleanup.
- World mmap flush отделяет immutable dirty batch от файлового IO; gameplay не
  держит map lock во время write/flush.
- Persistence completion будит simulation сразу, включая completion внутри
  длинного batch.

### Диагностика

- Tickprof разделяет wall, thread CPU и off-CPU.
- Чистая host preemption не выдаётся за дорогой алгоритм.
- Schedule, lock wait, command, flush и side phases имеют отдельные профили.
- Shutdown, due order, saturation и persistence completion покрыты
  deterministic tests в завершённых срезах.

## Что горит

### P1: global mutable `GameState`

ECS находится под общим `RwLock`, а session/admin/web/background paths всё ещё
могут читать или мутировать authoritative state. Поэтому preemption owner-а
превращается в latency других подсистем, а invariants нельзя доказать типами.

### P2: connect delivery и global lock

Предыдущий подтверждающий trace одного connect (новейший trace указан выше):

- total tick: `43.35ms`;
- thread CPU: `6.84ms`;
- off-CPU: `36.51ms`;
- connect dispatch: `38.07ms`;
- initial presentation build: `10.69ms`;
- schedule после connect: `4.30ms`.

Это не `43ms` вычислений, но пользовательская latency реальна. Auth hydrate
уже вне owner-а; `Connect` теперь выполняет entity/index apply и публикует
immutable `PlayerInit` effect. Chunk snapshot, wire encode и delivery выполняет
presentation owner после повторной session guard. Global ECS lock и off-CPU
latency этим не устранены.

```text
auth hydrate outside owner
    -> bounded lifecycle admission
    -> short entity/index apply
    -> immutable PlayerInitView
    -> encode/send in presentation owner
```

### P2: periodic dirty scan

Dirty registries закрыты: periodic player/building flush работают только по
deduplicated entity registry и requeue-ят остаток при saturation. `DirtyPlayers`
проверяет entity generation против текущей player-map, поэтому старая
incarnation после reconnect не может сохранить новую.

### P2: один idle player всё ещё запускает periodic systems

Programmator, guns, standing-cell hazards и granular physics используют
bounded active/due work. Granular region seed-ится при position transition, а
cell transition будит только локальный frontier; sleeping player не запускает
physics schedule. Alive и bots render пока не все выражены через explicit
registries, поэтому `1 idle player` ещё платит за оставшиеся periodic systems.

### P2: presentation/read paths

`bots_render.snapshot`, admin map и часть initial presentation читают общий
ECS/world. На hotspot это даёт global snapshots и lock hold, а не работу по
изменившимся chunks.

## Завершённый кодовый срез

**Graceful drain accepted DueAction закрыт.** Исходный дефект был:

```text
InventoryUse списывает item и ставит future DueAction
    -> shutdown немедленно выходит из owner loop
    -> finish_shutdown уничтожает DueActionQueue
    -> final snapshot сохраняет уже списанный item
```

Protector/Raz дополнительно возвращали `building_removals` во внешний command
channel. После закрытия ingress такой follow-up терялся.

Реализованный shutdown:

```text
Quiescing
    -> close external command ingress
    -> drain buffered commands
    -> wait for DueAction real deadlines
    -> drain owner-local building-delete FIFO
    -> drain death/box backlogs
    -> drop last PersistenceHandle
    -> apply every persistence completion
    -> final player/building/world flush
```

### Реализованные изменения

1. `crates/openmines-server/src/game/logic/due.rs`: добавить
   `DueActionQueue::is_empty()`.
2. `crates/openmines-server/src/game/mod.rs`: добавить
   `pub(crate) fn allocate_command_sequence(&self) -> CommandSeq`, использующий
   существующий `command_seq`. И external enqueue, и internal building delete
   получают sequence только через этот API.
3. `crates/openmines-server/src/tasks/simulation.rs`: добавить owner-local
   `building_deletes` в `TickPendingWork`; `finish_shutdown(mut self)`
   превращается в quiescing loop.
4. `crates/openmines-server/src/tasks/simulation/effects.rs`: складывать
   Protector/Raz removals во внутренний FIFO, не вызывать
   `GameState::enqueue_command`.
5. `crates/openmines-server/src/tasks/simulation/commands.rs`: дренить internal
   deletes через существующий `PlayerCommand::RemovePack` apply-path;
   persistence permit резервируется до mutation, saturated head остаётся в FIFO.
6. `crates/openmines-server/src/tasks/simulation/tick.rs`: добавить узкий
   quiescing cycle без schedules, bots render и periodic dirty snapshot
   producers.
7. `scripts/arch-guard.sh`: запретить возврат `enqueue_command` из
   `tasks/simulation/effects.rs`.

Не менять порядок shutdown в `tasks/mod.rs` и `main.rs`: simulation сейчас
держит последний `PersistenceHandle`, worker будит owner при progress.

### Проверка

- `tasks::simulation::tick::tests::internal_building_delete_saturation_preserves_head_and_runtime_state`:
  saturated internal FIFO не теряет head и не мутирует ECS до admission;
- `tasks::simulation::shutdown_tests::delete_completion_is_applied_before_final_shutdown_flush`:
  completion применяется до final flush;
- exact-deadline/order tests Boom/Protector/Raz остаются зелёными;
- `CARGO_INCREMENTAL=0 cargo test -p openmines-server tasks::simulation:: -- --nocapture`:
  `33 passed`.

Targeted gate:

```bash
CARGO_INCREMENTAL=0 cargo test -p openmines-server tasks::simulation:: -- --nocapture
CARGO_INCREMENTAL=0 cargo test -p openmines-server game::logic::due:: -- --nocapture
CARGO_INCREMENTAL=0 cargo clippy -p openmines-server --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
scripts/arch-guard.sh
scripts/dev-smoke.sh
```

### Запрещённые решения

- не выполнять future actions через `Instant::MAX`;
- не refund/cancel уже принятый consumable;
- не добавлять shutdown timeout с последующим drop;
- не использовать `tokio::sleep` внутри simulation owner;
- не возвращать internal follow-up во внешний command channel;
- не создавать второй unbounded internal channel или второй apply path;
- не запускать обычные schedules во время quiescing;
- не закрывать persistence до admission всех follow-up;
- не называть graceful drain crash durability;
- не смешивать этот срез с ECS ownership, chat optimization или multicore.

## Завершённый кодовый срез

**M2: bounded typed ingress закрыт.** Вместо одного unbounded `PlayerCommand`
ingress введены независимые bounded каналы lifecycle, gameplay и internal.

- capacity: `1024/8192/1024`; budgets на active cycle: `64/256/64`;
- gameplay full отклоняется до mutation и получает legacy-safe `OK`; lifecycle
  и internal используют awaitable admission, поэтому принятый follow-up не
  теряется;
- depth, oldest age, residence, rejected и budget carry-over метрики разделены
  по классу;
- saturated durable head одного класса не блокирует runnable команду другого;
  FIFO внутри класса сохраняется;
- deterministic tests покрывают reserve lifecycle при full gameplay, round-robin,
  persistence saturation, starvation и исчерпание class budget.

Release runtime gate на одном `8x8` local fixture:

- baseline: `100` gameplay clients, `1000 Xmov/s`, p99 `5.325ms`;
- staged storm: те же `100` gameplay clients (`15000/15000` effects, `0`
  unexpected disconnect) и отдельный burst `300` connect clients;
- gameplay pool under storm: p99 `7.078ms`, p99.9 `8.249ms`; storm pool
  подключился `300/300`, без disconnect и drain timeout.

Первый 35s прогон отброшен: loadtest не поддерживал heartbeat дольше 30s и
получил `Pong timeout`; корректный 15s staged run исключил этот артефакт.

Это закрывает admission safety, но не объясняет и не устраняет CPU-bound
`channel_chat` `201ms` или off-CPU/global-lock stalls. Они остаются evidence для
будущих vertical slices.

## Завершённый кодовый срез

**M3: due queue для пушек закрыт.** Перевели guns с периодического сканирования всех сущностей (`OnlinePlayers`) на explicit `DueGuns` с использованием кэширования кандидатов вокруг активных игроков.

- Добавлена логика `DueGuns` в планировщик, которая срабатывает только при наличии игроков в сети и наступлении времени выстрела пушек.
- Введен метод `fill_gun_candidate_batch`, собирающий кандидатов-пушек в чанках вокруг активных игроков.
- Исправлено отсутствие dirty-меток для пушек: при изменении заряда (charge) пушка помечается в `DirtyBuildings` для сохранения.
- Устранена флапающая ошибка/коллизия базы данных в тестах `schedule_intervals_come_from_config` путем изоляции временных путей SQLite для параллельных тестов.
- Все тесты, clippy, `arch-guard.sh` и `dev-smoke.sh` успешно проходят.

## Завершённый кодовый срез

**Active reconnect и DirtyPlayers закрыт.**
- Исправлено поведение active reconnect: при повторном подключении старая ECS-сущность больше не деспавнится и не пересоздаётся, а переиспользуется.
- В ECS-компонент `PlayerFlags` добавлено поле `incarnation: SessionId`.
- Ресурс `DirtyPlayers` переведён на хранение пар `(Entity, SessionId)`, что позволяет безопасно фильтровать и отбрасывать устаревшие грязные записи предыдущих инкарнаций сессии при сохранении (в методе `snapshot_dirty_player` и таске `flush_dirty_players_once`), исключая ABA гонки и затирание свежих данных.
- Исправлен баг синхронизации ролей при реконнекте: роль игрока теперь корректно обновляется в `PlayerStats` при переиспользовании ECS-сущности на логине (для корректной работы `is_admin_command`).
- Все тесты, включая `dirty_player_registry_drops_stale_entity_after_reconnect`, `stale_disconnect_cannot_remove_or_save_reconnected_incarnation` и `scripts/dev-smoke.sh`, успешно проходят.

## Завершённый кодовый срез

- [x] **M4. Thin connect.** Connect ограничен entity/index apply; immutable
  `PlayerInitView` кодируется и доставляется presentation owner-ом.
- [x] **M5. Chat consistency.** Чат использует `CommandEffects::Saves(ChatAppend)`
  и `ChatFanout`.
- [x] **M6. Command pipeline.** Все session actions проходят через общий
  `QueuedGameCommand { player_id, session_id, command: GameCommand }`; три
  bounded QoS-очереди остаются admission policy M2.
- [x] **M7. Programmator consistency.** `createprog:` выдаёт
  `SaveCommand::ProgramCreate`; persistence completion открывает editor только
  исходной current session.

**Hazards active/due registry закрыт.** `HazardDueSchedule` держит один
ближайший deadline на entity и отбрасывает stale heap entries. Scheduler
запускает hazards только при due batch (`256` entities); system повторно ставит
только живого игрока на непустой клетке. Damage, box pickup и destructible-cell
effects сохранили существующий apply path. C190 reset перенесён к C190 use,
чтобы безопасная idle-позиция не меняла его timeout semantics.

Проверка: registry dedup/deadline test, scheduler test для safe idle player,
полный server suite (`366 passed`, `1 ignored`), strict clippy, architecture
guard и `scripts/dev-smoke.sh`.

## Завершённый кодовый срез

**Granular active frontier закрыт.** `GranularWakeQueue` разделяет region seed
и local cell wake; scheduler запускает physics только при pending/active
frontier, с legacy `physics_ms` cadence. Position transitions seed-ят один
region, cell transition будит локальную область; после опустошения frontier
physics больше не удерживает schedule активным.

Проверка: granular physics fixtures, scheduler test safe idle/active frontier,
полный server suite (`367 passed`, `1 ignored`), strict clippy, architecture
guard и `scripts/dev-smoke.sh`.

## Текущий кодовый срез

**Следующий vertical slice: alive active registry.** Заменить scan player
windows на explicit registry живых клеток, поддерживаемый cell/position
transitions. Не смешивать с ECS ownership или read model; legacy five-second
cadence и deterministic conflict order должны остаться проверяемыми.

## Видимые milestones

| Milestone | Пользовательский результат | Статус |
| --- | --- | --- |
| M1. Zero-player idle | почти нулевой CPU, нет 100 Hz ticks и idle warnings | готов |
| M2. Saturation safety | DB stall не вызывает OOM, starvation или item loss | готов |
| M3. Zero scan-all idle | огромный clean world не влияет на maintenance cost | не готов |
| M4. Thin connect | connect storm не блокирует gameplay, init не сидит в owner | не готов |
| M5. Cheap idle actor | один sleeping player/robot почти ничего не стоит | не готов |
| M6. Owned ECS | нет external writers и global ECS lock wait | не готов |
| M7. Interest read model | fanout/render зависит от changed/visible chunks | не готов |
| M8. Spatial multicore | одинаковый digest и доказуемый speedup на 1/2/4 workers | не начат |
| M9. Time-scale invariance | real-time/accelerated/resumed timeline дают одинаковый digest | future |

## Конечный критерий

Миграция закончена, когда одновременно выполняются свойства:

- CPU равен `O(ready work)`, а не размеру мира или числу sleeping actors;
- память равна `O(loaded + active + bounded queues)`;
- authoritative apply детерминирован для ordered input, explicit time и RNG seed;
- скорость wall-clock replay не меняет authoritative state/event digest;
- разрешённая недетерминированность ограничена ingress/admission/presentation;
- mutation имеет один путь `typed input -> admission -> apply -> effects`;
- durable accepted work не теряется при saturation или graceful shutdown;
- ECS и spatial indexes имеют одного owner-а;
- multicore достигается spatial ownership без общего gameplay lock;
- wire клиента остаётся неизменным;
- senior может восстановить ownership model из этого документа и нескольких
  module facades, не читая весь сервер.
