# OpenMines Server Architecture Plan

## Статус

Этот документ является рабочим планом миграции сервера. Он заменяет прежний
RFC, в котором были смешаны диагноз, benchmark-журнал, V1/V2 и детали отдельных
оптимизаций.

Текущий commit, проценты, runtime evidence и следующий исполняемый срез находятся
только в `SERVER_MIGRATION_STATUS.md`. Этот файл задаёт целевую модель и gates;
исторический журнал ниже не должен переопределять checkpoint.

Форма feature-модулей, error/effect contracts и capability guards вынесены в
`SERVER_CONSISTENCY_PLAN.md`. Эти планы имеют общие gates, но не смешивают
runtime ownership с механической организацией файлов.

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

## Выученные уроки

1. Wall time без `thread_cpu/off_cpu` не доказывает дорогой алгоритм. Сначала
   классифицируется stall, затем меняется code path.
2. Даже host preemption становится серверной проблемой, если происходит под
   глобальным write-lock: вытесненный owner блокирует независимую работу.
3. Immutable extract -> unlock -> IO является каноническим persistence pattern.
   World flush подтвердил это реальным устранением read contention.
4. Structural split файла снижает навигационную цену, но не меняет ownership.
   Slice завершён только когда старый mutation/send/DB/spawn path удалён.
5. Локальный Rayon под общим `RwLock` не является multicore architecture.
   Параллельный read-only analysis полезен, но authoritative apply требует
   spatial ownership.
6. Bounded означает не только размер контейнера: нужны admission policy, depth,
   oldest age, saturation behavior и deterministic test.
7. Один активный игрок не должен будить все shards или платить за global barrier.
   Multicore включается по active shards, а idle world остаётся почти бесплатным.

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
11. Каждый migrated flow имеет одну форму: typed input -> bounded admission ->
    authoritative apply -> typed effects. IO/presentation adapters находятся за
    этой границей, а не образуют исключения внутри apply.
12. Authoritative apply детерминирован для ordered input, явного clock value и
    explicit RNG seed. OS scheduling, network arrival и presentation могут быть
    недетерминированы только за typed policy boundary и не меняют state result.
13. Gameplay delay выражается deadline, а не `sleep`. Реакция на нагрузку
    выражается budgets, admission и backpressure, а не скрытым adaptive tick.
14. Crate/file boundary вводится по владельцу state или capability, а не по
    размеру файла. Reverse dependency запрещается compile/architecture guard.
15. Simulation kernel является внутренним ядром OpenMines, не универсальным
    framework: generic abstraction появляется только после двух доказанных
    одинаковых contracts.
16. Time-scale invariance: одинаковые snapshot, logical-time inputs/deadlines и
    RNG seed дают одинаковый authoritative digest независимо от скорости
    wall-clock replay. Presentation timing может отличаться, state result - нет.

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

### История завершённых срезов до checkpoint 2026-07-12

Этот раздел сохраняет evidence и последовательность миграции. Формулировки
`следующий` внутри исторических записей относятся к моменту измерения. Текущий
порядок всегда брать из `SERVER_MIGRATION_STATUS.md`.

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

Чистые host-preemption misses (`thread_cpu <= 25% budget`, `off_cpu >= 4x CPU`)
больше не публикуются как server `WARN`: они остаются throttled `DEBUG` и в
cadence metrics. CPU-bound и mixed stalls по-прежнему являются `WARN`. Реальные
примеры `0.174ms CPU + 17.680ms off-CPU` и `5.151ms CPU + 13.594ms off-CPU`
зафиксированы classification test как `preempted` и `mixed` соответственно.

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

Это доказало отсутствие потерь и устойчивый throughput на том сценарии, но не
завершило цель большой MMORPG. На том checkpoint fixed-rate `10ms` admission ещё
ограничивал latency; позднее fixed idle loop был удалён event-driven срезом.

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

Building delete теперь резервирует `SaveKind::BuildingDelete` до mutation, а
optional Box записывается атомарно в той же SQLite transaction. Старый
`ApplyRemovedBuilding`/`SaveKind::Box` dual path удалён.

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

Death box drop перенесён на тот же boundary:

- gameplay, AoE, console и ECS schedules публикуют death intent в один
  deduplicating FIFO owner;
- lifecycle резервирует `SaveKind::Box` до любых death/respawn mutation;
- после admission death batch одним authoritative apply меняет player/world,
  возвращает box upsert и только затем публикует wire effects;
- при saturation позиция, корзина и world остаются неизменными, intent pending;
- death больше не пишет в `box_persist_q`; saturation и exactly-once box save
  покрыты deterministic test.

Box persistence slice завершён полностью:

- manual/programmatic/auto-dig публикуют typed pickup intent и получают ordered
  basket/bubble/cell/bot effects только после admission;
- building removal и damage destruction проходят через typed `RemovePack` ->
  bounded persistence -> `BuildingDeleted` completion;
- `BoxPersistQueue`, `box_persist_q`, per-tick Tokio DB spawn и shutdown direct
  drain удалены;
- все box upsert/delete теперь проходят только через bounded persistence owner.

Первый program persistence flow также завершён:

- `PROG` формирует typed `ProgramSaveRequest` внутри simulation и больше не
  запускает отдельную Tokio task с прямыми SQLx-вызовами;
- save source, выбор программы и authoritative readback выполняются одной
  storage transaction у persistence owner;
- admission заранее резервирует bounded slots и для request, и для completion,
  поэтому completion delivery не может заблокировать writer при shutdown;
- transient storage failure ретраится, permanent rejection завершает request
  без бесконечного retry;
- completion содержит исходный `SessionId`: результат старой сессии после
  reconnect не меняет ECS и не отправляет wire новой сессии;
- saturation, transient retry, permanent rejection, atomic transaction,
  отсутствие direct DB mutation и stale-session completion покрыты тестами.

Runtime trace на том checkpoint определил тогдашний Stage 2 GUI slice:

- одна `GuiButton` command заняла `202.3ms wall / 195.4ms thread CPU`, то есть
  это доказанный CPU-bound server stall, а не host preemption;
- текущий string dispatcher объединяет authoritative craft/market/teleport
  mutations и read-only window rendering под одним именем `gui_button`;
- запланированный срез типизировал GUI commands: mutation оставалась в
  simulation, а immutable view/render work уходила в presentation owner;
- granular spike `42.5ms wall` содержал только `2.6ms thread CPU` и `40.9ms
  off-CPU`, поэтому он не является основанием для локального cell/Rayon fix;
  spatial ownership остаётся отдельным этапом после устранения external writers.

Первый structural slice owned simulation runtime завершён:

- прежний `tasks/lifecycle.rs` на `3051` строку разделён на periodic IO lifecycle
  (`66` строк), supervisor/owner `SimulationRuntime` (`~500` строк) и закрытые
  модули command admission, scheduler, effects, dirty snapshots и profiler;
- `SimulationRuntime` единолично владеет command receiver, persistence completion
  receiver, pending command/death/box backlogs, schedule clock и tick counters;
- lifecycle больше не содержит scheduler, profiling, gameplay apply или effect
  flush;
- `tick.rs` теперь только координатор фаз (`~200` строк), без scheduler internals,
  persistence scan implementation и формата profiler-логов; production-функции
  проходят strict `too_many_lines` без нового suppression;
- `scripts/arch-guard.sh` запрещает async task spawn и direct DB access внутри
  simulation owner, чтобы новый boundary нельзя было обойти незаметно;
- это structural ownership move, а не финальное когнитивное упрощение всего
  сервера: старые external writers и публичные `GameState` mutation APIs ещё
  удаляются следующими срезами;
- это пока не полный Stage 4: runtime всё ещё держит `Arc<GameState>`, а полный
  production audit нашёл внешние ECS access paths и direct DB mutations;
- первым обязательным writer-removal slice были delayed consumables с Tokio
  timers; этот путь позднее заменён owner-owned `DueActionQueue`. Auction и
  другие external writers остаются отдельным долгом;
- только после удаления external writers Bevy `World` переносится в runtime по
  значению и `RwLock<EcsWorld>` удаляется физически.

World flush contention закрыт первым snapshot/persist slice:

- runtime trace и timestamps файлов доказали, что `hazards lookup_time=140ms`
  ожидал `cells/road RwLock`, пока periodic flush выполнял файловый IO под
  write-lock;
- `MapStore` теперь ведёт deduplicated explicit dirty slots и отделяет immutable
  versioned batch за `O(dirty)`;
- `open/seek/write/flush` выполняются после освобождения `cells/road` locks;
- partial failure возвращает dirty header/index/slots, а journal checkpoint
  происходит только после успешной записи всех слоёв;
- read-only gameplay больше не ждёт map IO. World mutation пока всё ещё ждёт
  journal mutex: полный generation journal/persistence boundary не завершён.

Первый typed GUI/presentation slice завершён для открытия телепорта:

- `GUI_` несёт исходный `SessionId` и parsed `GuiCommand`; profiler labels
  конечны и не зависят от пользовательского payload;
- stale queued teleport-open после reconnect не мутирует новую сессию;
- teleport view подготавливается как immutable DTO, destinations берутся через
  bounded chunk index вместо scan всех building entities;
- HORB serialization и отправка выполняются presentation owner без доступа к
  ECS/world/DB;
- исправлен подтверждённый server wire bug миникарты: Unity parser требует
  `=R#color`, а прежний builder отправлял `=R%color`, из-за чего Rect prefab
  вообще не создавался;
- это teleport-only vertical slice. Остальные GUI/economy/clan/auction handlers
  ещё используют legacy adapters, а одинаковый release trace до/после пока не
  выполнен, поэтому `gui_button=202ms` нельзя объявлять полностью устранённым.

Temporal semantics будущего на тот момент `DueActionQueue` была подтверждена по
reference и затем реализована:

- Unity `TY.time` является неубывающим planned client timestamp, но C# server
  только декодировал его и не использовал для reorder, latency compensation или
  late-drop; в новом server это telemetry, не authoritative deadline;
- delayed action получает `due_at = authoritative_apply_time + delay` и
  стабильный ключ `(due_at, admission_sequence)`;
- action становится eligible на первом owner tick после deadline; owner исполняет
  её ровно один раз, но bounded per-tick due budget может стабильно перенести
  избыток на следующий tick с измеримой lateness; disconnect её не отменяет;
- bounded admission происходит до показа transient pack и списания item;
- legacy shrinking half-drain queue, global 5us drop gate и thread races не
  являются наблюдаемым контрактом и не переносятся.

Свежий trace `2026-07-11 18:24` уточняет, но не меняет диагноз:

- connect tick занял `22.69ms wall`, но только `5.90ms thread CPU`; `16.79ms`
  были off-CPU, поэтому это mixed stall, а не `22ms` чистой ECS работы;
- внутренний connect profile всё ещё показал `13.58ms wall`, включая
  `8.83ms spawn_ecs`: connect остаётся отдельным bounded flow, а не поводом
  оптимизировать Bevy вслепую;
- `tick.schedule` держал глобальный ECS write-lock `38.11ms wall`. Без локального
  CPU sample это не доказательство дорогой schedule, но сам lock превращает
  preemption одного потока в блокировку внешних readers/writers;
- конечный connect path: hydrate вне simulation -> bounded admission -> короткий
  entity/index apply -> immutable `PlayerInitView` -> encode/send в presentation.

Single-source connect storm `2026-07-11 19:34` отделил network capacity от
simulation capacity:

- запрос `100000` соединений с одного localhost дал `10173` TCP connect,
  `1893` завершённых login и `89827` connect errors; server process не падал;
- локальный ephemeral range был `49152..=65535`, а accept backlog `128`, поэтому
  этот прогон не измеряет предел concurrent sessions сервера;
- при примерно `1241` одновременно видимых игроках `bots_render.snapshot`
  удерживал глобальный ECS read-lock до `63.33ms`, а `hazards` занимал до
  `12.19ms`; это подтверждает presentation/global-scan bottleneck независимо
  от внешнего connect limit;
- loadtest обязан разделять connect errors по OS error, auth timeout, server
  close и outbox overflow. Агрегированное `connect errors` не является
  архитектурным доказательством;
- connect storm является отдельным gate: lifecycle admission не должен
  вытеснять уже принятые gameplay commands или раздувать simulation backlog.

## Карта миграции

Этапы перекрываются, поэтому один общий процент вводит в заблуждение:

| Этап | Текущее состояние | Следующий честный gate |
| --- | --- | --- |
| 0. Evidence/guards | active | одинаковый release baseline для каждого perf slice |
| 1. Session/output owner | существенно начат | common authenticated envelope, без sender capability |
| 2. Command/effects | частично | каждый перенесённый flow имеет один path и zero direct output |
| 3. Persistence owner | частично; building delete закрыт | zero direct gameplay DB writes |
| 4. Admission/isolation | event-driven wait и due queue закрыты; ingress unbounded | bounded queues и connect storm без gameplay starvation |
| 5. Owned simulation | только structural foundation | zero external ECS writers, удалить `RwLock<EcsWorld>` |
| 6. Active/due work | ранние slices | zero scan-all для sleeping/clean actors |
| 7. Interest/read model | pilot | immutable per-chunk snapshots и bounded fanout |
| 8. Spatial multicore | не начат как ownership model | deterministic 1/2/4-worker digest и speedup |

Исполняемый порядок здесь намеренно не дублируется. Текущий единственный
следующий срез, его файлы, acceptance tests и anti-solutions находятся в
`SERVER_MIGRATION_STATUS.md`.

Typed building-delete gate закрыт 2026-07-12:

- `RemovePack` резервирует bounded persistence command и completion capacity до
  любой authoritative mutation;
- admission заменяет `BuildingFlags` на `BuildingDeletePending`, сохраняя точное
  прежнее dirty-состояние; pending building исключён из lookup, damage/effect,
  gun, Raz, AccessGun, teleport view и Resp/death paths;
- SQLite одним transaction проверяет `id + coordinates`, удаляет building,
  очищает Resp bindings и записывает optional Box;
- completion повторно проверяет `id + coordinates + operation id`, поэтому stale
  или ABA result не может удалить replacement;
- runtime apply возвращает полный ordered cell/block/window/box/inventory effect
  result без скрытой отправки из world mutation;
- graceful shutdown прекращает admission, закрывает последний persistence sender,
  применяет все reserved completions до final player/building/world flush;
- permanent persistence failure bounded и размораживает building; legacy
  `ApplyRemovedBuilding`, direct DB delete helpers и dual path удалены и запрещены
  architecture guard;
- saturation zero-mutation, dirty restore, operation ABA, atomic Resp/Box,
  effect order, pending-system freeze и shutdown-before-flush покрыты тестами.
- strict workspace clippy, server `340 passed / 1 ignored`, protocol `120 passed`,
  storage `32 passed`, architecture guard и `scripts/dev-smoke.sh` зелёные.

Crash/restart durability ещё не закрыта: процесс может аварийно завершиться между
SQLite commit и mmap apply. Этот разрыв закрывает запланированный generation
journal/replay; graceful shutdown barrier не выдаётся за crash journal.

Gate первого `DueActionQueue` slice для Boom:

- queue принадлежит `SimulationRuntime` по значению, без `Arc/Mutex`;
- key равен `(due_at, admission_sequence)`, payload не содержит socket/session;
- reservation происходит до cooldown/item/transient-pack mutation;
- saturation оставляет gameplay state неизменным;
- due drain имеет item/time budget, stable carry-over и lateness metric;
- Boom apply использует deterministic RNG и bounded spatial candidates;
- apply возвращает typed cell/health/death/pack/FX effects и не делает
  send/DB/spawn;
- legacy Boom `tokio_handle.spawn + sleep` удаляется в том же slice.

Gate закрыт 2026-07-11:

- queue принадлежит `SimulationRuntime` по значению и использует стабильный
  `(Instant, sequence)` key;
- admission резервирует capacity до cooldown/item/transient-pack mutation, а
  saturation сохраняет state неизменным;
- drain имеет item/time budgets, stable carry-over и lateness metrics;
- deterministic Boom apply возвращает единый typed effects result без
  network/DB/task capabilities;
- command -> due -> schedule effect ordering и active `SessionId` после
  reconnect закреплены end-to-end тестом;
- legacy Boom timer удалён;
- strict server clippy, `331 passed / 1 ignored`, protocol `120 passed` и
  `scripts/dev-smoke.sh` зелёные.

Protector/Raz due-action gate закрыт 2026-07-12:

- общий inventory admission резервирует due capacity до cooldown, item и
  transient-pack mutation для Boom, Protector и Raz;
- saturation оставляет cooldown, inventory, pack registry и wire неизменными;
- payload хранит только authoritative center и trigger player, без socket/session;
- Protector/Raz apply выполняется simulation owner, не делает send/DB/spawn и
  возвращает typed cell/health/death/building-delete/pack/FX effects;
- player и building candidates берутся из chunk indexes; Raz больше не сканирует
  все здания мира и помечает каждое повреждённое здание dirty;
- damage attribution отделена от player-request authorization: AoE delete не
  требует ownership/proximity, ручное удаление по-прежнему требует;
- удаление Gate/IDamagable проходит только через durable `RemovePack` completion;
- legacy Protector/Raz `tokio_handle.spawn + sleep`, direct wire и второй damage
  path удалены; architecture guard запрещает их возврат;
- saturation, exact 2s/5s deadline, mixed FIFO, wire-free apply, damage rights,
  building dirty и ordered end-to-end wire покрыты deterministic tests.

Event-driven owner wait gate закрыт 2026-07-12:

- fixed `sleep`/`next_tick_at` удалены; owner запускает active cycle только для
  runnable command/completion/backlog или due/schedule/maintenance deadline;
- command, persistence capacity/completion, crafting, external death/box,
  schedule change и shutdown публикуют state до lost-wake-safe `unpark`;
- spurious/coalesced wake приводит к повторному plan, а не пустому tick;
- persistence saturation паркует только заблокированный work class и не создаёт
  busy-spin; independent completion/Box work продолжает исполняться;
- indefinite idle не тревожит watchdog, timed park остаётся наблюдаемым после
  deadline+tolerance; blocking wait исключён из tickprof wall/off-CPU;
- fixed `10ms` сохранился как active-cycle budget, а gun/due delays остаются
  точными domain deadlines;
- deterministic tests покрывают earliest deadline, idle schedules без catch-up,
  persistence blocking, wake-before-park, watchdog idle/overdue и Unix deadline;
- release `0 players`: за контрольные `120s` выполнено `15` active cycles вместо
  теоретических `12_000` fixed ticks, process CPU `0.14s`, текущий CPU `0.0%`,
  RSS `5-12 MiB`, idle `OVER-BUDGET`/watchdog warnings отсутствуют;
- один SIGINT полностью остановил owner, persistence и world flush без Enter.

Остаток idle-cost теперь локализован: periodic player dirty flush будит owner раз
в `10s` и сканирует player entities; buildings уже используют `DirtyBuildings`
с работой `O(dirty)`. Следующий performance gate - incarnation-aware player
registry. Дешёвый `1 idle player` после
этого требует due-only hazards/guns/programmator вместо periodic entity scans.

Event-driven wait не закрывает два соседних correctness gate: основной command
ingress остаётся unbounded, а graceful shutdown пока может отбросить accepted
future DueAction после уже сохранённого списания item. Они имеют приоритет над
следующим performance slice и подробно зафиксированы в
`SERVER_MIGRATION_STATUS.md`.

Этап 3 не завершён. Прямые persistence bypass ещё есть у остальных program
операций (open/create/rename/delete/copy), chat/GUI/auction и shutdown
player/building snapshot. Очередь writer пока in-memory: graceful drain доказан,
crash/restart durability требует отдельного intent journal.

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

На persistence checkpoint планировался перенос `bots_render` на immutable chunk
snapshots и отделение command admission от fixed-rate schedules. Event-driven
admission позже выполнен; `bots_render` read model всё ещё не завершён.

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
- каждый migrated flow следует единому typed input/admission/apply/effects
  шаблону, закреплённому architecture guard после удаления legacy path;
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

### Этап 4. Admission и изоляция классов работы

- заменить unbounded simulation inbox на bounded typed ingress;
- разделить lifecycle, gameplay, due work и maintenance на явные классы с
  независимыми item/time budgets;
- admission policy определяет accept, coalesce и reject до authoritative
  mutation; принятая команда не теряется молча;
- auth hydrate и подготовка immutable init data выполняются вне simulation;
- connect apply ограничен entity/index mutation и не кодирует/не отправляет
  initial presentation;
- фиксированный пустой tick заменяется ожиданием ближайшего command, deadline,
  active frontier или maintenance deadline;
- публикуются queue depth, oldest age, admitted, coalesced, rejected и budget
  exhaustion по каждому классу.

Gate:

- connect storm не ухудшает p99 уже авторизованных gameplay commands;
- lifecycle backlog bounded и имеет проверяемую overload policy;
- один игрок не платит за пустые tick/schedule/shard wakeups;
- 10k idle sessions не создают simulation work;
- deterministic tests доказывают отсутствие starvation между классами.

### Этап 5. Owned simulation runtime

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

### Этап 6. Active/due work

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

### Этап 7. Interest management

- immutable per-chunk presentation snapshots;
- observer subscriptions принадлежат SessionHub/read model;
- snapshot encode/cache выполняется вне authoritative apply;
- initial player presentation строится из immutable `PlayerInitView`, а не
  сериализуется под authoritative ECS access;
- hotspot fanout имеет явный gameplay/presentation cap;
- output coalescing не меняет authoritative state.

Gate:

- 10k idle connections имеют bounded memory;
- 1k hotspot actors не создают неконтролируемый `O(P^2)` backlog;
- slow clients изолированы от simulation.

### Этап 8. Spatial multicore

Это нативная многопоточность на фиксированном пуле OS workers, не Tokio tasks на
сущность и не один thread на shard. Один мир делится только технически:

- одинаковые `ShardState` владеют диапазонами chunks;
- один writer на shard;
- active-shard registry планирует только shards с command/due/frontier work;
- один active shard исполняется одним worker без fanout по пулу и без общего
  барьера с dormant shards;
- независимые active shards одного epoch исполняются параллельно;
- cross-shard intents проходят отдельную stable border phase;
- border mailboxes bounded, а merge сортируется по stable source/sequence key;
- migrations имеют generation и deterministic order;
- global services не получают право мутировать shard state.

Фазы worker epoch одинаковы для sequential и parallel режима:

```text
admit local commands
-> apply local due/frontier work
-> export border intents
-> deterministic border merge/apply
-> emit immutable effects
```

Spatial multicore ускоряет распределённую активность. Один hotspot shard не
получает линейный speedup автоматически; его защищают bounded work/fanout caps,
а делить его мельче можно только тем же ownership protocol.

Gate:

- sequential и parallel state/event digests совпадают;
- минимум 1.7x throughput на 2 workers и 3x на 4 workers для shardable case;
- при конфигурации 4 workers один active shard имеет не более 5% overhead против
  one-worker baseline и не будит остальные workers;
- border conflicts и actor migration покрыты tests;
- мир остаётся непрерывным, без gameplay regions/instances.

## Capacity contract

Цифры являются тестовыми gates, а не обещанием одного VPS:

| Scenario | Gate |
| --- | --- |
| Огромный idle world | tick cost не зависит от world area |
| 100k sleeping actors | work зависит только от due/active subset |
| 10k idle sessions | bounded memory и независимый simulation tick |
| Single-source connect storm | классифицированные ошибки; не считается capacity gate |
| Distributed connect storm | bounded admission без gameplay starvation |
| Distributed active actors | p99 command-to-effect < 50ms без растущего backlog |
| 1k hotspot actors | p99 < 50ms при зафиксированном fanout cap |
| Multicore | доказуемый speedup при одинаковом state/event digest |
| Один active shard | не более 5% overhead от configured worker pool |
| Persistence saturation | bounded backlog, видимая admission policy, no loss |
| Due burst | bounded work per tick, stable carry-over, measured lateness |
| Offline fast-forward | digest совпадает для real-time, accelerated и resumed replay |

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
- lifecycle storm не может вытеснить gameplay или создать unbounded backlog;
- idle runtime ждёт событие/deadline, а не выполняет фиксированные пустые ticks;
- real-time, accelerated и resumed replay одной logical timeline дают одинаковый
  authoritative state/event digest;
- multicore достигается spatial ownership без общего gameplay lock;
- capability gates из `SERVER_CONSISTENCY_PLAN.md` равны zero;
- новый gameplay flow добавляется в одном domain module и не требует правок
  lifecycle, TCP loop, DB task и ECS transport component одновременно.
