# OpenMines Server Architecture Plan

## Статус

Этот документ является рабочим планом миграции сервера. Он заменяет прежний
RFC, в котором были смешаны диагноз, benchmark-журнал, V1/V2 и детали отдельных
оптимизаций.

Задача не состоит в подавлении `OVER-BUDGET` warning и не требует переписывать
игру с нуля. Нужно изменить владение состоянием так, чтобы сервер одновременно:

- оставался authoritative MMORPG server для одного непрерывного мира;
- был понятен без знания десятков скрытых путей мутации;
- масштабировался по active/due work, а не по размеру мира;
- использовал несколько ядер через spatial ownership, а не общий `RwLock`;
- сохранял legacy wire без изменения клиента.

## Подтверждённый диагноз

`GameState` сейчас объединяет слишком много владельцев:

- Bevy `World` под глобальным `RwLock`;
- mmap-мир и игровые spatial indexes;
- DB и persistence counters;
- network senders и kick channels;
- command queues;
- schedules и due queues;
- auth/rate-limit/admin state.

Следствия:

- session/UI/admin/background code может мутировать ECS в обход tick;
- gameplay systems могут напрямую отправлять wire и запускать async DB work;
- tick вручную координирует schedules, locks и side-effect queues;
- unbounded queues превращают burst в latency и рост памяти;
- добавление Rayon ускоряет отдельный анализ, но не устраняет сериализацию;
- каждый локальный performance-fix увеличивает число состояний системы.

Проблема находится в ownership/orchestration. Bevy, protocol, world storage и
сама игровая логика не являются причиной полного rewrite.

## Целевая модель

```text
TCP sessions -> SessionHub -> Command -> Simulation
                    ^                       |
                    |------ GameEvent ------|
                                            |
                              SaveCommand -> Persistence

Admin/API <- immutable ReadSnapshot
```

### SessionHub

Единственный владелец живых соединений:

- `SessionId -> bounded outbox`;
- `PlayerId -> current SessionId` после успешной авторизации;
- доставка direct/broadcast/nearby output;
- slow-client policy и graceful disconnect;
- transport metrics.

В ECS и `PlayerCommand` нет socket sender. Reconnect определяется `SessionId`,
а не клонированием channel handle по игровому коду.

### Simulation

Единственный владелец authoritative gameplay state:

- Bevy `World` хранится по значению на simulation thread;
- player/building/bot/spatial indexes имеют того же владельца;
- команды применяются в стабильном порядке;
- gameplay mutation возвращает `GameEvent` и `SaveCommand`;
- network, SQLx, HTTP и Tokio runtime не импортируются в simulation modules.

Bevy остаётся внутренним хранилищем компонентов и scheduler API. Снаружи нет
`ecs.read()`, `ecs.write()`, closure-based `query_player` или `modify_player`.

### Persistence

Один bounded worker:

- принимает `SaveCommand`;
- группирует совместимые записи;
- выполняет retry по явной политике;
- публикует completion command с operation/session generation;
- экспортирует depth, age и failure metrics.

Durable команды не drop. При saturation admission ограничивает источник.

### ReadSnapshot

Admin, metrics и initial presentation читают versioned immutable snapshot.
Они не блокируют simulation и не получают mutable ECS handles.

## Простые правила архитектуры

1. У каждого mutable state ровно один владелец.
2. У gameplay action ровно один путь: command -> mutation -> effects.
3. Tokio обслуживает IO; игровые сущности не являются Tokio tasks.
4. Все production queues bounded и имеют overflow policy.
5. Presentation можно coalesce/drop; accepted durable action терять нельзя.
6. Parallel analysis read-only; authoritative apply принадлежит spatial owner.
7. После переноса вертикального среза старый путь удаляется сразу.
8. Временный dual ownership запрещён.
9. Новый тип или слой вводится только если закрывает существующий bypass.
10. Wire и наблюдаемое клиентом поведение не меняются.

## Что сохраняется

- crates `openmines-protocol`, `openmines-world`, `openmines-storage` и runtime;
- Bevy components, gameplay systems и проверенная доменная логика;
- world dirty tracking, journal и range flush;
- granular intents, parallel read-only analysis и deterministic apply;
- loadtest, wire smoke, golden tests и tick metrics;
- существующий Unity wire contract.

## Что постепенно исчезает

- `GameState` как god object;
- `RwLock<EcsWorld>`;
- `PlayerConnection` с network sender внутри ECS;
- `player_tx` и transport helpers в game state;
- прямые `net/session` и DB вызовы из gameplay;
- external `query_player`/`modify_player` closures;
- global unbounded input/output queues;
- ручная классификация `ScheduleActivity` как публичная orchestration model;
- scan-all для due actors;
- второй scheduler или mutex-backed kernel рядом со старым tick.

## Миграция

Каждый этап является законченным вертикальным срезом. Нельзя начинать spatial
sharding, пока simulation всё ещё доступна внешним writers.

### Текущее состояние, 2026-07-10

Этап 1 выполнен первым вертикальным срезом:

- введён `SessionId`;
- `SessionHub` владеет session/player mapping, reconnect и kick;
- per-session outbox bounded (`2048` packets), overflow отключает slow client;
- TCP flush имеет packet/byte budget и write timeout;
- `PlayerCommand`, ECS `PlayerConnection` и programmator queues больше не
  содержат sender;
- async completion commands адресуются исходным `SessionId`, поэтому результат
  старой сессии не уходит в новую после reconnect;
- session tests: `174 passed`;
- весь `openmines-server`: `279 passed`, `1 ignored` benchmark;
- `scripts/dev-smoke.sh` проходит весь wire flow;
- release `1000 clients`, ramp `3ms`, `5000 Xmov/s`:
  - `1000/1000` login/init;
  - `50001/50001` command effects;
  - `0` unexpected disconnect, `0` drain timeout;
  - p50 `17.651ms`, p95 `23.222ms`, p99 `24.590ms`, max `34.451ms`.

Connect/disconnect vertical slice этапа 2 завершён:

- `apply_player_command` возвращает `CommandEffects`;
- disconnect возвращает `SavePlayer + Fanout` и не запускает DB/network work;
- connect mutation возвращает cleanup/spawn broadcasts как `GameEvent`;
- initial presentation является immutable ordered packet batch;
- `PresentationRuntime` владеет bounded async delivery queue;
- `Fanout` содержит immutable `Vec<SessionId>`, поэтому presentation worker не
  читает ECS или simulation spatial indexes;
- lifecycle только публикует effects и не выполняет network fanout;
- stale session guard не доставляет batch после disconnect/reconnect;
- полный server suite и `scripts/dev-smoke.sh` зелёные.

Предыдущий release `1000 clients`, ramp `3ms`, `5000 Xmov/s`:

- `1000/1000` login/init;
- `50111/50111` command effects;
- `0` unexpected disconnect, `0` drain timeout;
- p50 `17.357ms`, p95 `21.838ms`, p99 `23.734ms`, max `29.325ms`.

Дополнительно завершён первый active-work slice:

- overlapping `alive` windows объединяются в exact scanline frontier;
- работа зависит от площади spatial union, а не `players * 33 * 33`;
- прежний `alive 14.688ms` spike на 1000-player hotspot исчез;
- connect/disconnect fanout больше не исполняется внутри authoritative tick;
- disconnect lock regression `50ms` устранена immutable recipient snapshot.

Wall-clock profiler теперь пишет `thread_cpu`, `off_cpu` и `execution_class`.
Проверенный debug warning `11.318ms wall` содержал только `1.521ms CPU` и
`9.797ms off-CPU`; пустые/random section spikes больше не считаются
алгоритмическим доказательством без CPU time.

Move/chunk visibility slice перенесён на command/effect boundary:

- сетевой `Move` несёт исходный `SessionId`, поэтому stale queued move после
  reconnect не может мутировать entity новой сессии;
- validation/mutation выполняется один раз и возвращает immutable movement и
  chunk fanout snapshots;
- direct movement/chunk output возвращается как ordered `GameEvent` и
  доставляется `PresentationRuntime` вне authoritative dispatch;
- auto-dig/open-pack пока являются явно обозначенным synchronous fallback:
  эти редкие пути сохраняют wire-order до переноса их собственных handlers на
  effects и не создают скрытого второго mutation path.

Проверка после завершения среза, два последовательных release-прогона по 10с:

- `1000/1000` login, `50451/50451` и `50011/50011` command effects;
- `0` unexpected disconnect, `0` drain timeout в обоих прогонах;
- прогон 1: p50 `15.351ms`, p95 `20.777ms`, p99 `22.671ms`,
  p99.9 `27.010ms`;
- прогон 2: p50 `15.009ms`, p95 `20.446ms`, p99 `23.343ms`,
  p99.9 `26.635ms`;
- единичные max `777.518ms` и `121.713ms` не образуют устойчивый server-tail;
  p99.9 стабилен, а tickprof классифицирует wall spikes по
  `thread_cpu/off_cpu`;
- воспроизводимый CPU-bound burst найден на массовом disconnect: dispatch
  использовал явный `10ms` budget и перенёс остаток команд на следующий tick.

Это доказывает отсутствие потерь и устойчивый throughput на данном сценарии,
но не завершает цель большой MMORPG. Fixed-rate `10ms` command admission всё
ещё добавляет ожидание до simulation tick и ограничивает latency снизу.

Этап 3 начат первым bounded persistence slice:

- `PersistenceRuntime` владеет bounded queue (`4096`) и единственным writer;
- writer сохраняет соседние команды одного типа batch-ами до `128`, не меняя
  FIFO между разными типами;
- transient DB errors ретраятся без потери уже принятых durable commands;
- `Disconnect` и daily bonus резервируют durable slot до authoritative mutation;
- periodic player/building snapshots используют atomic dirty handoff: при
  saturation флаг не снимается, после admission новая mutation снова ставит
  `dirty`;
- tick и periodic producers имеют явные task handles; shutdown сначала
  останавливает producers, затем дренирует writer;
- экспортируются depth, high-water, oldest age, batch size и result counters;
- saturation/pending disconnect, slow store, retry, mixed-type FIFO и shutdown
  drain покрыты deterministic tests.

Первый box flow также перенесён: `ApplyRemovedBuilding` резервирует `SaveKind::Box`
до mutation, возвращает typed `BoxWrite`, а writer сохраняет ordered box batch в
одной SQLite transaction. Старый `box_persist_q` в этом flow не используется.

Hazard box pickup также перенесён на intent/admission boundary:

- ECS hazards schedule только публикует typed `BoxPickupIntent` и не меняет
  box/player/world;
- simulation thread держит FIFO backlog с максимум одним intent на игрока;
- `SaveKind::Box` резервируется до authoritative pickup, а при saturation intent
  остаётся pending без mutation;
- после admission pickup повторно валидирует player position и box, затем одной
  операцией меняет world/index/crystals и возвращает typed delete + broadcasts;
- saturation, retry-after-capacity, duplicate coalescing и exactly-once save
  покрыты deterministic tests.

Этап 3 не завершён. Прямые persistence bypass ещё есть у death box flow,
program/chat/GUI/auction и shutdown snapshot. Hazard больше не использует bypass;
следующий box-срез — death drop с admission до `box_put` и очистки корзины. Старый
`box_persist_q` остаётся только у ещё не перенесённых box flows. Очередь writer
пока in-memory: graceful drain доказан, crash/restart durability требует
отдельного intent journal.

Проверка первого persistence milestone, release `1000 clients`, ramp `3ms`,
`5000 Xmov/s`, 10 секунд:

- `1000/1000` login, `50013/50013` command effects;
- `0` unexpected disconnect, `0` drain timeout;
- p50 `17.287ms`, p95 `23.228ms`, p99 `26.034ms`, p99.9 `28.674ms`,
  max `39.589ms`;
- disconnect persistence: `1000 accepted / 1000 persisted`, `11` batches,
  queue depth после drain `0`, high-water `869`, без saturation/retry;
- один Ctrl-C завершил release server без дополнительного Enter после полного
  persistence и world drain.

Latency gate `<50ms` пройден, потерь нет. Это не ускорение относительно лучших
предыдущих прогонов (`p50 ~15ms`, `p99 ~22-23ms`): текущий p50/p99 хуже примерно
на `2-3ms`, поэтому первый persistence slice оценивается как ownership/durability
улучшение без доказанного performance gain.

После закрытия persistence bypass `bots_render` переносится на immutable chunk
snapshots, а command admission отделяется от fixed-rate schedules без второго
simulation owner.

Strict clippy всего server tree зелёный. `apply_player_command` разделён на
typed command families, schedule tail и program-save вынесены в именованные
helpers; новые suppressions не добавлялись.

Интерактивный shutdown проверен на release binary: Ctrl-C завершает процесс без
дополнительного Enter. Stdin reader больше не остаётся навечно в blocking
`read_line`, а проверяет готовность fd и stop-флаг.

### Этап 0. Baseline и архитектурные ограничения

- сохранить текущие targeted/runtime проверки;
- зафиксировать production boundary inventory;
- не подключать неподключённый mutex-backed `game/kernel` draft;
- добавить dependency checks для запрещённых imports по мере появления новой
  simulation boundary.

Gate:

- текущий wire smoke зелёный;
- baseline load scenarios воспроизводимы;
- никакой новой performance primitive не добавляется в `GameState`.

### Этап 1. Session identity и output ownership

- ввести monotonic `SessionId`;
- создать `SessionHub` с bounded per-session outboxes;
- `Connect` несёт `SessionId`, но не sender;
- убрать sender из `PlayerConnection` и затем сам компонент;
- direct/broadcast output направляется через `GameEvent` в `SessionHub`;
- reconnect и kick проверяют current `SessionId`.

Gate:

- в ECS и `PlayerCommand` нет sender types;
- slow-reader не увеличивает simulation queue/tick latency;
- connect/reconnect/disconnect tests и dev smoke зелёные.

### Этап 2. Обязательная command/effect boundary

- `apply_player_command` возвращает `CommandEffects`;
- `CommandEffects` содержит ordered `GameEvent` и `SaveCommand`;
- переносить handlers по одному live vertical flow;
- packet encoding выполняется после simulation mutation;
- completion имеет correlation/generation guard;
- после переноса удаляется соответствующий legacy handler path.

Рекомендуемый порядок flows:

1. move/chunk visibility;
2. dig/build;
3. heal/inventory/boxes;
4. programmator;
5. chat/clans/GUI/economy.

Gate:

- simulation code не импортирует `net/session`, SQLx или socket sender;
- один command нельзя применить двумя путями;
- state/event digest детерминирован для одного command stream;
- protocol golden tests и Unity behavior не изменились.

### Этап 3. Persistence owner

- заменить `tokio::spawn` per gameplay action на bounded writer;
- batch player/building/program/chat/box writes;
- dirty extraction идёт из явных dirty sets;
- backlog age и saturation видимы;
- shutdown дожидается durable drain.

Gate:

- искусственно медленная SQLite не меняет simulation tick p99;
- durable commands не теряются при saturation/restart tests;
- нет DB future, удерживающей gameplay state или ECS handle.

### Этап 4. Owned simulation runtime

- `SimulationRuntime` получает Bevy `World` по значению;
- command receiver, tick clock и gameplay indexes принадлежат runtime;
- `lifecycle.rs` оставляет только start/sleep/watchdog/shutdown;
- session/admin/console используют commands и read snapshots;
- удалить `RwLock<EcsWorld>` и внешние mutation APIs.

Gate:

- production inventory не содержит внешних ECS mutation bypass;
- ECS lock wait отсутствует вместе с самим lock;
- connect/disconnect/completion races покрыты deterministic tests;
- replay даёт одинаковый state/event digest.

### Этап 5. Active/due work

- programmator: due actors only;
- hazards: cell-change activation и damage deadlines;
- guns: due guns + spatial query;
- crafting/building damage: due queues;
- alive/granular: active frontier by chunk;
- bots render: presentation scheduler outside gameplay systems;
- persistence: explicit dirty actors/buildings.

Gate:

- 100k sleeping actors стоят как пустой actor set;
- idle huge-world fixture не создаёт world-area work;
- один due actor не сканирует остальные;
- work линейна active frontier, а не player windows/world area.

### Этап 6. Interest management

- immutable per-chunk presentation snapshots;
- observer subscriptions принадлежат SessionHub/read model;
- snapshot encode/cache выполняется вне authoritative apply;
- hotspot fanout имеет явный gameplay/presentation cap;
- output coalescing не меняет authoritative state.

Gate:

- 10k idle connections имеют bounded memory;
- 1k hotspot actors не создают неконтролируемый `O(P^2)` backlog;
- slow clients изолированы от simulation.

### Этап 7. Spatial multicore

Один мир делится только технически:

- одинаковые `ShardState` владеют диапазонами chunks;
- один writer на shard;
- shards одного tick исполняются параллельно;
- cross-shard intents проходят отдельную stable border phase;
- migrations имеют generation и deterministic order;
- global services не получают право мутировать shard state.

Gate:

- sequential и parallel state/event digests совпадают;
- минимум 1.7x throughput на 2 workers и 3x на 4 workers для shardable case;
- border conflicts и actor migration покрыты tests;
- мир остаётся непрерывным, без gameplay regions/instances.

## Capacity contract

Цифры являются тестовыми gates, а не обещанием одного VPS:

| Scenario | Gate |
| --- | --- |
| Огромный idle world | tick cost не зависит от world area |
| 100k sleeping actors | work зависит только от due/active subset |
| 10k idle sessions | bounded memory и независимый simulation tick |
| Distributed active actors | p99 command-to-effect < 50ms без растущего backlog |
| 1k hotspot actors | p99 < 50ms при зафиксированном fanout cap |
| Multicore | доказуемый speedup при одинаковом state/event digest |
| Persistence saturation | bounded backlog, видимая admission policy, no loss |

Legacy HB использует `u16` coordinates. Без изменения wire адресуемый предел
равен `0..=65535` клеток по каждой оси. Huge world поэтому требует sparse/lazy
chunk storage, а не увеличения dense mmap.

## Проверка каждого среза

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
cargo test --all-targets --all-features
cargo test -p openmines-protocol --all-features
scripts/dev-smoke.sh
```

Изменение gameplay дополнительно проверяется живым Unity-клиентом. Performance
срез требует release before/after на одинаковом world fixture и worker count.

## Definition of done

Сервер считается архитектурно переведённым, когда:

- connection, simulation, persistence и admin read model имеют разных явных
  владельцев;
- authoritative state недоступен внешним mutable code paths;
- command/effect/save являются обязательными, а не декоративными enums;
- стоимость idle мира и sleeping actors не зависит от их полного количества;
- queues bounded, saturation видима и протестирована;
- multicore достигается spatial ownership без общего gameplay lock;
- новый gameplay flow добавляется в одном domain module и не требует правок
  lifecycle, TCP loop, DB task и ECS transport component одновременно.
