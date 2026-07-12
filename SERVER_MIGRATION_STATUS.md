# OpenMines Server Migration Status

Обновлено: 2026-07-12.

Это короткая карта фактического состояния. Подробные решения и доказательства
находятся в `SIMULATION_KERNEL_PLAN.md`; правила формы кода - в
`SERVER_CONSISTENCY_PLAN.md`.

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

## Карта этапов

Проценты по строкам не складываются в общий процент. Они показывают зрелость
конкретного архитектурного слоя.

| Этап | Готовность | Фактическое состояние |
| --- | ---: | --- |
| 0. Evidence и guards | 75% | release traces, CPU/off-CPU classification, strict clippy, architecture guard; одинаковый benchmark обязателен не для каждого среза |
| 1. Session/output owner | 65% | `SessionId`, bounded outbox, `SessionHub`, presentation owner есть; common authenticated envelope не завершён |
| 2. Command/effects boundary | 35% | connect/disconnect, move, teleport-open, delayed consumables и building delete перенесены; GUI/economy/chat/clan/admin ещё имеют bypass |
| 3. Persistence owner | 45% | bounded writer, batching, retry, completion и graceful drain есть; program/chat/GUI/auction bypass и crash journal остаются |
| 4. Admission/isolation | 35% | event-driven wait и bounded due queue готовы; основной ingress всё ещё unbounded, workload classes не разделены |
| 5. Owned simulation | 15% | runtime владеет clocks/receivers/backlogs, но ECS и indexes остаются в `Arc<GameState>` под глобальным `RwLock` |
| 6. Active/due work | 20% | granular frontier, crafting due queue и consumable due queue есть; actors/guns/hazards/dirty snapshots ещё частично scan-all |
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

### P1: основной ingress остаётся unbounded

При долгом persistence stall durable command удерживает FIFO head, а новые
movement/chat/connect продолжают накапливаться. Это bounded-latency и OOM risk.

Нужен не `Vec`-лимит и не silent drop, а typed ingress classes:

- lifecycle - гарантированный bounded admission;
- gameplay - bounded queue с явной overload policy;
- internal completion/follow-up - отдельный недропаемый путь;
- depth, oldest age, rejected/coalesced и starvation metrics.

### P1: accepted DueAction теряется при graceful shutdown

Inventory уже списан, но `finish_shutdown` сейчас может уничтожить будущий
Boom/Protector/Raz до его deadline. Финальный player snapshot тогда сохраняет
списанный item, а действие не выполняется.

Правильный gate:

- simulation переходит в `Quiescing` и закрывает внешний ingress;
- buffered commands и due actions исполняются по реальным deadlines;
- Protector/Raz building removals идут во внутренний simulation-owned FIFO, а
  не обратно во внешний command channel;
- persistence completion barrier завершается до final player/building/world
  flush.

Это закрывает graceful shutdown. Crash durability отдельно требует persistent
action intent/idempotency и replay; `Instant` сериализовать нельзя.

### P1: global mutable `GameState`

ECS находится под общим `RwLock`, а session/admin/web/background paths всё ещё
могут читать или мутировать authoritative state. Поэтому preemption owner-а
превращается в latency других подсистем, а invariants нельзя доказать типами.

### P2: connect остаётся широким synchronous flow

Свежий trace одного connect:

- total tick: `43.35ms`;
- thread CPU: `6.84ms`;
- off-CPU: `36.51ms`;
- connect dispatch: `38.07ms`;
- initial presentation build: `10.69ms`;
- schedule после connect: `4.30ms`.

Это не `43ms` вычислений, но пользовательская latency реальна. Целевой flow:

```text
auth hydrate outside owner
    -> bounded lifecycle admission
    -> short entity/index apply
    -> immutable PlayerInitView
    -> encode/send in presentation owner
```

### P2: periodic dirty scan

- players: scan всех player entities каждые `10s`;
- buildings: scan всех building entities каждые `45s`;
- saturation откладывает повтор до следующего interval.

Building registry делается первым: lifecycle зданий уже централизован, а
production dirty transitions ограничены. Player registry требует сначала
incarnation-safe reconnect, иначе старая dirty entity может исчезнуть без save.

### P2: один idle player всё ещё запускает periodic systems

Hazards, guns, programmator, physics/alive и bots render пока не все выражены
через explicit due/active registries. Поэтому `0 players` уже дешёвый, а
`1 idle player` - ещё нет.

### P2: presentation/read paths

`bots_render.snapshot`, admin map и часть initial presentation читают общий
ECS/world. На hotspot это даёт global snapshots и lock hold, а не работу по
изменившимся chunks.

## Следующий обязательный порядок

1. Закрыть graceful drain accepted DueAction через `Quiescing` и internal
   building-delete backlog.
2. Заменить unbounded ingress на bounded typed workload classes с overload и
   starvation policy.
3. Ввести `DirtyBuildings`, удалить building scan-all и исправить missing dirty
   marks в hourly damage/gun charge.
4. Исправить active reconnect; затем ввести incarnation-aware `DirtyPlayers`.
5. Сузить connect до entity/index apply и вынести `PlayerInitView` encode/send.
6. Перевести programmator, guns, hazards, alive/granular на due/active registries.
7. Удалить external ECS writers и физически передать Bevy `World` simulation
   owner-у по значению.
8. Построить immutable per-chunk read model и interest subscriptions.
9. Только затем вводить spatial workers и проверять deterministic digest на
   `1/2/4` workers.

## Видимые milestones

| Milestone | Пользовательский результат | Статус |
| --- | --- | --- |
| M1. Zero-player idle | почти нулевой CPU, нет 100 Hz ticks и idle warnings | готов |
| M2. Saturation safety | DB stall не вызывает OOM, starvation или item loss | следующий |
| M3. Zero scan-all idle | огромный clean world не влияет на maintenance cost | не готов |
| M4. Thin connect | connect storm не блокирует gameplay, init не сидит в owner | не готов |
| M5. Cheap idle actor | один sleeping player/robot почти ничего не стоит | не готов |
| M6. Owned ECS | нет external writers и global ECS lock wait | не готов |
| M7. Interest read model | fanout/render зависит от changed/visible chunks | не готов |
| M8. Spatial multicore | одинаковый digest и доказуемый speedup на 1/2/4 workers | не начат |

## Конечный критерий

Миграция закончена, когда одновременно выполняются свойства:

- CPU равен `O(ready work)`, а не размеру мира или числу sleeping actors;
- память равна `O(loaded + active + bounded queues)`;
- authoritative apply детерминирован для ordered input, explicit time и RNG seed;
- разрешённая недетерминированность ограничена ingress/admission/presentation;
- mutation имеет один путь `typed input -> admission -> apply -> effects`;
- durable accepted work не теряется при saturation или graceful shutdown;
- ECS и spatial indexes имеют одного owner-а;
- multicore достигается spatial ownership без общего gameplay lock;
- wire клиента остаётся неизменным;
- senior может восстановить ownership model из этого документа и нескольких
  module facades, не читая весь сервер.
