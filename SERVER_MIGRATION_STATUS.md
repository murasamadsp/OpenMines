# OpenMines Server Migration Status

Обновлено: 2026-08-10.

Это **единственный актуальный checkpoint и handoff** по миграции сервера.
Подробная целевая модель находится в `docs/SIMULATION_KERNEL_PLAN.md`; целевой план реструктуризации на изолированные крейты — в `docs/TARGET_ARCHITECTURE_PLAN.md`; правила формы
кода - в `docs/SERVER_CONSISTENCY_PLAN.md`; фактическая topology - в
`docs/ARCHITECTURE.md`. Старые приоритеты в `docs/backlog/TODO.md` и `docs/backlog/AUDIT_STATE.md` не
являются планом simulation migration.

## Как продолжить работу

Новый агент сначала выполняет только read-only проверки:

```bash
git status --short --branch
git log -1 --oneline
git diff --check
```

Ожидаемый code checkpoint: Настроен ecs bypass baseline, ecs-bypass-guard.py, test_ecs_bypass_baseline_guard и WebSnapshot для stats/map.

Дальше:

1. Прочитать разделы `Что горит`, `Текущий кодовый срез` и `Запрещённые решения` ниже.
2. Изучить утвержденный [docs/TARGET_ARCHITECTURE_PLAN.md](file:///Users/murasama/Projects/games/OpenMines/docs/TARGET_ARCHITECTURE_PLAN.md) по реструктуризации на изолированные крейты (Nested Crates).
3. Перейти к Этапу 2 плана миграции: постепенному переносу хэндлеров сессий на команды с очисткой baseline-файла `docs/reference/ecs_bypass_baseline.txt`.
4. ~~Следующий конкретный vertical slice — перевести **мутации рынка** GUI (`sell`, `buy`, `sellall`, `getprofit`) на typed command/admission/apply/persistence/effects. Read-only tab switching не смешивать с продажей/покупкой.~~ **Готово.**
5. ~~Перевести покупку слота Up (`buyslot`) на typed mutation/effect/persistence путь с сохранением legacy `GU` wire.~~ **Готово.**
6. ~~Перевести durable Up `delete:{slot}`, `install:{code}#{slot}` и `upgrade` на typed mutation/effect/persistence путь с сохранением порядка `P$/@S/LV/@L/sp/GU`.~~ **Готово.**
7. ~~Перевести building-placement completions (`ApplyPaidBuildingPlaced`, `ApplyInventoryBuildingPlaced`, refund) с прямого Outbox на typed `CommandEffects` с сохранением legacy `P$/Gu/IN/O`.~~ **Готово.**
8. ~~Перевести `DPBX/OpenBox` с gameplay direct-Outbox на typed `SessionBatch`, сохранив `GU` и `open_box` window state.~~ **Готово.**
9. ~~Перевести `InventoryUse` output/errors, включая DB-failure path building placement, на typed effects, сохранив legacy inventory packets, `OK` payload и порядок delayed effects.~~ **Готово.**
10. ~~Перевести local-chat state errors на typed `SessionBatch`, сохранив обычный nearby `HB` side-phase и legacy `OK` payload.~~ **Готово.**
11. ~~Перевести channel-chat state errors на typed `SessionBatch`, сохранив отдельный `ChatFanout` для нормального сообщения и legacy `OK` payload.~~ **Готово.**
12. ~~Перевести programmator editor completion (`Gu/#P/Gu`, `#p/Gu`) на typed `SessionBatch`, сохранив точный legacy packet order и state mutation.~~ **Готово.**
13. ~~Перевести programmator persistence errors для `open/rename/create` на typed `SessionBatch`, сохранив rejected/permanent-failure semantics и legacy `OK` payload.~~ **Готово.**
14. ~~Перевести весь `ProgramSaved` completion на typed `SessionBatch`, сохранив success/error wire sequence (`Gu/@P/BH/#p` или `Gu/@P/BH/OK`) и legacy payload.~~ **Готово.**
15. ~~Перевести `ChatColorCycled` success (`mC`) и permanent DB failure (`OK`) на typed `SessionBatch`, сохранив rejected как тихий результат и legacy payload.~~ **Готово.**
16. ~~Перевести `ChatResynced` success (`mO/mU`) и permanent DB failure (`OK`) на typed `SessionBatch`, сохранив `PlayerUI.current_chat`, rejected/access-denied semantics и legacy payload/order.~~ **Готово.**
17. ~~Перевести `ChatMenuLoaded` success (`mL/mN`) и permanent DB failure (`OK`) на typed `SessionBatch`, сохранив legacy payload/order и rejected semantics.~~ **Готово.**
18. ~~Перевести `ChatPrivateOpened` success (`mO/mU`) и permanent DB failure (`OK`) на typed `SessionBatch`, сохранив `PlayerUI.current_chat`, `TargetNotFound` semantics и legacy payload/order.~~ **Готово.**
19. ~~Перевести malformed `PROG` decode error (`@P/OK`) на typed `SessionBatch`, сохранив legacy packet order/payload и отсутствие `SaveCommand`.~~ **Готово.**
20. ~~Перевести program `pRST` running→stopped output (`Gu/@P/BH`) на typed `SessionBatch`, сохранив reset state transition, stopped pre-open no-op и legacy packet order.~~ **Готово.**
21. ~~Перевести program `PREN` rename prompt (`GU` или malformed `@P`) на typed `SessionBatch`, сохранив `pren:{id}` UI state, HORB payload и legacy semantics.~~ **Готово.**
22. ~~Перевести общий `KernelContext::slash_ok_effect` на typed `SessionBatch`, сохранив session guard и legacy `OK` payload для slash/completion ошибок.~~ **Готово.**
23. ~~Перевести `ClaimBonus` output (`P$/DR/OK`) на typed `SessionBatch`, сохранив legacy packet order/payload, Player save и not-ready/missing-state semantics.~~ **Готово.**
24. ~~Перевести `KnownNoopTy` output (`Help`/`Miso`) на typed `SessionBatch`, сохранив legacy packet payload и тихую семантику остальных известных no-op событий.~~ **Готово.**
25. ~~Перевести manual `Geology`/`Heal` output (`GE`, `@L/@B/@S` или legacy `OK`) из command-path direct sink в typed `SessionBatch`, сохранив programmatic auto-action wrappers и legacy wire.~~ **Готово.**
26. ~~Перевести manual/command-path `Dig` и `Build` output через typed `SessionBatch`, сохранив `OK` state errors, legacy packet payload и прежние world/broadcast effects.~~ **Готово.**
27. ~~Перевести movement follow-up output (`Autodig`/fallback `OpenPack`) с синхронной доставки через outbox на typed `SessionBatch`, сохранив порядок move output → follow-up output и legacy wire.~~ **Готово.**
28. ~~Перевести GUI `pack_remove` admission-overload output (`OK`) с прямого outbox sink на typed `SessionBatch`, сохранив enqueue semantics и legacy payload.~~ **Готово.**
29. ~~Перевести GUI `bld_place` preparation output (`P$` и validation `OK`) на typed `SessionBatch`, сохранив paid-placement admission, async DB task и legacy payload/order.~~ **Готово.**
30. ~~Перевести GUI `open_buildings` HORB output (`GU`) с sync fast-path direct sink на typed `SessionBatch`, сохранив legacy HORB payload.~~ **Готово.**
31. ~~Перевести GUI `createprog` HORB output (`GU`) с sync fast-path direct sink на typed `SessionBatch`, сохранив `createprog` window state и legacy payload.~~ **Готово.**
32. ~~Перевести GUI `clan_create_view`/`clancreate`/`clan_create` HORB output (`GU`) с sync fast-path direct sink на typed `SessionBatch`, сохранив `clan` window state и legacy payload.~~ **Готово.**
33. ~~Перевести GUI market tab switching `sellcrys`/`buycrys` (`GU`) с sync fast-path direct sink на typed `SessionBatch`, сохранив активную вкладку, `market` window state и legacy payload; auction и market mutations не смешивать.~~ **Готово.**
34. ~~Перевести GUI `clan_create_input` legacy `OK` output с sync fast-path direct sink на typed `SessionBatch`, сохранив точный payload.~~ **Готово.**
35. ~~Перевести GUI `craft_recipe` presentation (`GU`) с sync fast-path direct sink на typed `SessionBatch`, сохранив recipe details и `craft_start` action.~~ **Готово.**
36. ~~Перевести оставшийся sync `pack_op:*` output/error path на typed `SessionBatch`, сохранив async `Clans` exception и typed mutation routing (`take_*`/`remove`).~~ **Готово.**
37. ~~Перевести raw GUI `no-window` fallback (`Gu/_`) с sync fast-path direct sink на typed `SessionBatch`, сохранив close semantics; typed `Close` flow не дублировать.~~ **Готово.**
38. ~~Перевести рабочее открытие Clan pack (`pack_op:open:x:y`) с async legacy direct-output на typed `ClanMenu` admission, сохранив доступ и legacy `GU` через completion.~~ **Готово.**
39. ~~Перевести BuildWar skill update (`@S`) после cell conversion с `player_sender` на typed `SessionBatch`, сохранив wire payload и порядок side-phase.~~ **Готово.**
40. ~~Перевести death/respawn self-output (`Gu/@B/P$/@T/@L/@P` плюс chunk refresh) с `player_sender` на typed `SessionBatch`, сохранив порядок legacy self-пакетов и nearby HB.~~ **Готово.**
41. ~~Перевести death-admission state error (`OK`) с `player_sender` на typed `SessionBatch`, сохранив session guard и legacy payload.~~ **Готово.**
42. Следующий срез — выбрать следующий оставшийся session direct-output path и перевести его через тот же typed boundary; не удалять незавершённые feature-заготовки.
43. После каждого среза обновлять этот файл в том же commit. Не создавать новый handoff.

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

Ориентировочно выполнено **55% архитектурной миграции; около 45% осталось**.
Это не процент строк кода и не обещание срока: оценка пересчитана по stage-gates
после закрытия command/effects GUI, chat/clan, auction и programmer slices.
Этапы перекрываются, поэтому число служит только operational estimate. Главные
незакрытые ownership-блоки — удаление `RwLock<EcsWorld>`/внешних ECS writers,
immutable per-chunk interest read model и spatial multicore; последний пока 0%.

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
- исторический `channel_chat`: `201.75ms` command, tick `188.30ms thread CPU`.
  Он снят до commit `506e8ffc`: обычный `Chat` с тех пор использует typed
  `ChatAppend + ChatFanout`, поэтому этот trace нельзя приписывать текущему
  normal-message path без нового воспроизведения;
- `hazards`: полезный lookup занял `17.46us`, но `36.74ms` остались
  unaccounted; одновременно ECS write lock удерживался до `53.86ms`. Этот лог не
  доказывает дорогой hazard lookup, зато снова показывает цену global lock.

Вывод: M1 закрыл только нулевой idle. Active command paths, один idle player и
lock isolation не закрыты. Chat navigation и slash-command fallback теперь
проходят тот же typed command/apply/persistence/effects boundary; обычный
`Chat` переносить повторно запрещено.

### Последний воспроизведённый movement stress

Локальный release run `2026-07-13`, 15 секунд, один interest hotspot:

- `100` клиентов / `2 000 Xmov/s`: `30 001/30 001` effects, `0` unexpected
  disconnect и drain timeout; p50 `3.184ms`, p95 `8.553ms`, p99 `15.370ms`;
- `300` клиентов / `6 000 Xmov/s`: `90 258/90 258` effects, `0` unexpected
  disconnect и drain timeout; p50 `6.478ms`, p95 `16.211ms`, p99 `29.457ms`.

Это сравнимо с прежним профилем `300/6 000`, где p95 был `58ms`, p99 `186ms`.
Причина - presentation coalescing только непрерывных `MovementFanout`: для
каждого player оставляется последнее `HB/X`, а любой иной effect остаётся
ordering barrier. Authoritative movement и chunk-crossing packets не меняются.

### Последний воспроизведённый dig stress

Локальный dev run `2026-07-14`, `300` клиентов, `20s`, один interest
hotspot, `M3R_LOADTEST_ARENA=dig`: каждый запрос копает живую кристальную
клетку, меняет durability/economy и получает `@B`; это не login/movement smoke.

- до batching nearby `HB`: `30 295` sent / `30 259` acked, `36` rejected,
  p50 `51.892ms`, p95 `1 509.596ms`, p99 `3 314.466ms`, p99.9 `6 511.465ms`,
  max `8 439.488ms`;
- после batching: `30 022/30 022` dig acked, `0` rejected/disconnect/error/drain
  timeout; p50 `8.097ms`, p95 `78.471ms`, p99 `213.299ms`, p99.9 `273.906ms`,
  max `274.304ms`.

Причина подтверждена native sample: прямой `handle_dig -> broadcast_hb_at ->
broadcast_to_nearby -> Outbox::send` делал один `try_send` на каждого observer
для каждого действия. Dig теперь кладёт immutable `HB` effects в owner-local
queue; side phase склеивает подряд идущие `HB` subpackets в один frame на
recipient. `Direct`, cell/block update и не-HB nearby packet остаются ordering
barriers. Authoritative state и legacy HB layout не меняются.

Отдельная release-проверка после batching и per-side-phase spatial snapshot:
`1 000` клиентов, `15s`, `5 000` sustained dig/s, `75 059/75 059` acked,
`0` rejected/disconnect/error/drain timeout; p50 `14.275ms`, p95 `22.688ms`,
p99 `27.722ms`, p99.9 `33.557ms`. Это текущий доказанный single-thread
benchmark; debug/dev цифры выше используются только как сравнительный срез
одного и того же кода, а не как release capacity claim.

На том же release fixture проверен второй массовый action path: `1 000`
клиентов, `15s`, `5 000` sustained `Xmov`/s, `75 001/75 001` acked, `0`
rejected/disconnect/error/drain timeout; p50 `15.923ms`, p95 `21.416ms`,
p99 `24.090ms`, p99.9 `26.266ms`. `build` и `mixed` не объявляются
benchmark-ами, пока fixture не сможет поддерживать валидные изменения мира
без скрытого server-side reset: несколько первых успешных placement не являются
устойчивым gameplay workload.

Теперь есть отдельный `build-cycle` fixture: на каждой private cell выполняется
`Xbld/G`, затем двенадцать `Xdig`; это реальная смена `empty -> green block ->
empty` с crystal spend, world journal, durability и cell-map updates. Release
run `1 000` клиентов, `15s`, `75 101/75 101` action acked (`69 101` dig,
`6 000` build), `0` rejected/disconnect/error/drain timeout; p50 `16.970ms`,
p95 `43.876ms`, p99 `233.759ms`, p99.9 `237.889ms`. В первом probe именно
этот workload выявил sync `broadcast_cell_update` в command apply; dig/build
cell updates переведены в queued HB batch. Native sample после фикса не показал
CPU-bound hot stack: simulation большую часть окна parked, поэтому новый
рефакторинг без следующего evidence запрещён.

Для physics добавлен отдельный arena `M3R_LOADTEST_ARENA=granular`: у каждого
fixture-игрока лежит sand над непроходимой опорой, поэтому connect/position
transition создаёт granular frontier. Это устраняет ложный benchmark пустого
мира. Release run `2026-07-14`: `1 000` игроков, `5ms` bounded connect ramp,
`15s`, `5 000 Xmov/s`; `75 000/75 000` effect, `0` rejected/disconnect/drain
timeout, p50 `13.910ms`, p95 `18.973ms`, p99 `21.142ms`, p99.9 `23.456ms`.
Это подтверждает, что repeated `wake_granular_neighborhood` больше не создаёт
physics spike. Отдельный `ramp=0` connect storm теряет pre-auth sessions в
transport/outbox (`__sendto` и mutex по native sample); это другой admission
срез, не доказательство цены granular simulation.

Release reconnect storm `2026-07-14`: `1 000` fixture clients с `ramp=0`,
все `1 000/1 000` прошли auth/init без connect error, timeout или unexpected
disconnect. После полной готовности тот же run держал `5 000 Xmov/s`:
`50 002/50 002` acked, p50 `15.797ms`, p95 `21.411ms`, p99 `24.124ms`,
p99.9 `26.745ms`. Текущий `Connect` path не является подтверждённым bottleneck
одного simulation thread в этом масштабе.

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
| 0. Evidence и guards | 80% | release traces, CPU/off-CPU classification, strict clippy, architecture guard, ecs-bypass-guard.py, test_ecs_bypass_baseline_guard |
| 1. Session/output owner | 80% | `SessionId`, bounded outbox, `SessionHub`, presentation-owned PlayerInit, common authenticated envelope и movement coalescing есть |
| 2. Command/effects boundary | 45% | connect/disconnect, move, teleport-open, delayed consumables, building delete и ProgramCreate перенесены; GUI/economy/chat/clan/admin ещё имеют bypass |
| 3. Persistence owner | 50% | bounded writer, batching, retry, writer drain и `ProgramCreate` completion есть; GUI/auction bypass и crash journal остаются |
| 4. Admission/isolation | 60% | event-driven wait, bounded due queue, typed bounded ingress и thin connect готовы |
| 5. Owned simulation | 20% | runtime владеет clocks/receivers/backlogs, ecs bypass barrier (baseline) введён для сессий; ECS и indexes остаются в `Arc<GameState>` под RwLock |
| 6. Active/due work | 55% | O(1) remove_botspot_runtime внедрён; granular/alive frontier, crafting/consumable/programmator/guns/hazards due queues и dirty registries есть |
| 7. Interest/read model | 25% | WebSnapshot для stats/map полностью убрал ECS-блокировки из веб-API; teleport DTO, bots render, Map/BotSpot snapshots и movement fanout готовы |
| 8. Spatial multicore | 0% | Rayon analysis не является ownership sharding; deterministic 1/2/4-worker model ещё не начат |

## Что реально сделано

### Runtime ownership

- `SessionHub` владеет живыми session mappings и bounded per-session outbox.
- `PresentationRuntime` на выделенном bounded worker доставляет immutable
  effects вне authoritative mutation и drain-ится при shutdown.
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

### ECS Изоляция и Лимитирование (Июль 2026)

- **Ликвидация ECS-блокировок в веб-API**: Для рутов `/stats` и `/api/map` внедрен фоновый сбор снимков `WebSnapshot` в Simulation Thread (раз в секунду). Сетевые Axum-руты теперь читают только иммутабельный `Arc<WebSnapshot>` без блокировок.
- **Внедрение ECS Bypass Barrier**: Создан снимок текущего архитектурного долга `docs/reference/ecs_bypass_baseline.txt`.
- **Автоматический гвард**: Написан скрипт `scripts/ecs-bypass-guard.py` и интегрирован в `scripts/arch-guard.sh`. Любой новый прямой вызов ECS из `net/session` блокируется.
- **Интеграционный тест**: Добавлен тест `test_ecs_bypass_baseline_guard` в `tick/tests.rs` для проверки baseline на уровне `cargo test` с поддержкой автогенерации.
- **Оптимизация Botspots**: Метод `remove_botspot_runtime` переведен с линейного сканирования O(N) чанков на точечное O(1) удаление по координатам чанка.

## Что горит

### P1: global mutable `GameState`

ECS находится под общим `RwLock`, а session/admin/web/background paths всё ещё
могут читать или мутировать authoritative state. Поэтому preemption owner-а
превращается в latency других подсистем, а invariants нельзя доказать типами.
Scheduler уже отпускает write-lock между runnable schedules и перед tail, поэтому
preemption одного schedule не удерживает соседние jobs. Это mitigation, а не
замена owned ECS runtime.

### Закрыто: HB delivery вне simulation owner

Release `build-cycle` `2026-07-14`, `1 000` клиентов, `20s`, `5 000` action/s:
`100 051/100 051` dig/build effects, `0` rejected/disconnect/error/drain timeout,
p50 `11.998ms`, p95 `212.330ms`, p99 `223.309ms`, p99.9 `230.358ms`.

`BroadcastEffect` публикуется одним упорядоченным `WorldEffects` event в bounded
`PresentationRuntime`. Его выделенный thread кодирует HB и будит outbox;
`Direct`, cell/block update и non-HB nearby остаются strict barrier и сбрасывают
HB batch до delivery. Первый вариант оставил consumer на Tokio executor и дал
регрессию (`22 524` effect, p99 `3.1s`); он заменён dedicated thread, чтобы
bulk delivery не вытесняла session tasks. Shutdown закрывает sender и join-ит
worker после final effects.

В simulation tick side broadcasts стали сотнями микросекунд (`108-712us`)
вместо исторических `217ms`; текущие over-budget traces доминируют в dispatch
budget или granular physics, а не в outbox delivery.

Следующий build-cycle release после устранения двойного granular wake expansion:
`100 204/100 204` effects, `0` rejected/disconnect/error/drain timeout, p50
`11.691ms`, p95 `212.021ms`, p99 `221.588ms`, p99.9 `225.815ms`. Очередь
теперь хранит один changed-cell origin, а physics строит тот же итоговый
`x -2..+2`, `y -5..+2` candidate rectangle без промежуточных 15 wake points.
На повторном run slow granular physics warnings не наблюдались; оставшиеся
over-budget ticks - bounded dispatch с `2.4-3.1ms` CPU и `7-9ms` off-CPU.

После этого loadtest получил deterministic staggered gameplay mode; он выявил,
что бесконечное cross-side coalescing `WorldEffects` либо насыщает очередь без
слияния, либо задерживает delivery до первого external barrier. Текущий код
сливает не более `32` соседних streams и затем доставляет их. FIFO/barrier и
limit покрыты unit tests, strict clippy проходит. Новый release runtime claim
для этого bounded режима пока отсутствует: не подменять им цифры выше.

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

Новый trace `2026-07-13 19:56` после active registries: connect dispatch
`8.48ms wall` / `5.90ms CPU`, из них сам command `8.07ms`. Visibility commit
теперь выполняется в исходном entity write: отдельный post-spawn ECS write
удалён, затем owner регистрирует уже зафиксированный chunk index. Локальный
release login-only burst `2026-07-14` на `4x4` fixture: `100/100`, p95
`5.992ms`, p99 `6.082ms`; `300/300`, p95 `6.244ms`, p99 `6.383ms`, без
unexpected disconnect, drain timeout и tickprof warnings. Это fixture gate,
не замена world-sized benchmark; per-chunk building overlay cache остаётся
следующим read-model debt.

### P2: periodic dirty scan

Dirty registries закрыты: periodic player/building flush работают только по
deduplicated entity registry и requeue-ят остаток при saturation. `DirtyPlayers`
проверяет entity generation против текущей player-map, поэтому старая
incarnation после reconnect не может сохранить новую.

Нулевой player ECS больше не проходит periodic player snapshot: disconnect
резервирует и публикует final save до удаления entity, поэтому stale registry
не имеет права брать global ECS write-lock каждые 10 секунд. Release idle
`2026-07-14`, `70s`, `0` player entities: `0` `OVER-BUDGET`, persistence flush,
ECS-lock и watchdog warnings. Building flush остаётся независимым для dirty
строений.

### P2: один idle player всё ещё запускает periodic systems

Programmator, guns, standing-cell hazards, granular physics и alive cells
используют bounded active/due work. Granular/alive region seed-ятся при position
transition, cell transition обновляет локальный frontier; sleeping player не
запускает их schedules. Из заметных periodic read paths остаётся bots render.

**Свежий live-срез 2026-07-13 (нужно повторно измерить после текущей правки):**
на обычной карте `physics` удерживал ECS write lock до `616.84ms` при двух
кандидатах. Внутренний профиль показывал, что время уходит не на apply
(`3.92us`), а на granular scan. Причина: каждый wake point отдельно читал
малые участки mmap-мира. Wake points теперь дедуплицируются и группируются по
chunk, после чего для каждой группы снимается один snapshot. Функциональные
granular-тесты зелёные; нельзя считать latency исправленной до live-повтора
того же сценария.

Исправлен реальный hotspot `alive` при массовом login: несколько seed-окон
радиуса `33x33` раньше сканировались независимо и повторно обходили общие
клетки. `ActiveFrontier` теперь строит точное объединение row-интервалов и
сканирует каждую клетку spatial union ровно один раз. На release `4x4`
login-only burst `1000` клиентов p99 снизился с `218.815ms` до `36.729ms`,
unexpected disconnects -- с `327` до `178`; предупреждения `alive` исчезли.
Оставшиеся `178` -- readiness timeout loadtest, их причина пока не установлена:
перед изменением admission/outbox нужны сохранённые session/presentation логи.

`ProgrammatorDueSchedule` теперь хранит одну актуальную deadline на entity.
Повторный schedule заменяет logical deadline, а stale heap entries удаляются
перед выборкой; один entity не может заполнить due batch из `256` своих старых
шагов. Regression-test планирует один entity `257` раз и получает ровно один
последний step. Это устраняет подтверждённый источник раздувания executor batch;
новый runtime trace ещё нужен, чтобы измерить итоговый p99.

### P2: presentation/read paths

`bots_render` больше не читает ECS во время visibility walk и HB encode:
короткая сверка active-player атрибутов создаёт immutable cache, а BotSpot
cache обновляется на load/spawn/remove. Регрессия удерживает ECS write-lock во
время renderer batch, поэтому возврат к global ECS read запрещён тестом.

Admin map и часть initial presentation всё ещё читают общий ECS/world. На
hotspot это даёт global snapshots, а не работу по изменившимся chunks.

Непрерывный burst обычных movement `HB/X` теперь схлопывается presentation
owner-ом до последнего пакета на player. Первый non-movement effect - strict
barrier; его и последующие events delivery не перескакивает. Это ограничивает
устаревший presentation backlog, но не заменяет per-chunk interest model.

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

1. `crates/server/openmines-server/src/game/logic/due.rs`: добавить
   `DueActionQueue::is_empty()`.
2. `crates/server/openmines-server/src/game/mod.rs`: добавить
   `pub(crate) fn allocate_command_sequence(&self) -> CommandSeq`, использующий
   существующий `command_seq`. И external enqueue, и internal building delete
   получают sequence только через этот API.
3. `crates/server/openmines-server/src/tasks/simulation.rs`: добавить owner-local
   `building_deletes` в `TickPendingWork`; `finish_shutdown(mut self)`
   превращается в quiescing loop.
4. `crates/server/openmines-server/src/tasks/simulation/effects.rs`: складывать
   Protector/Raz removals во внутренний FIFO, не вызывать
   `GameState::enqueue_command`.
5. `crates/server/openmines-server/src/tasks/simulation/commands.rs`: дренить internal
   deletes через существующий `PlayerCommand::RemovePack` apply-path;
   persistence permit резервируется до mutation, saturated head остаётся в FIFO.
6. `crates/server/openmines-server/src/tasks/simulation/tick.rs`: добавить узкий
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

## Завершённый кодовый срез

**Alive active registry закрыт.** `AliveWorkQueue` scan-ит player window только
на position transition, хранит exact set обнаруженных `ALIVE_*` cells и каждые
пять секунд обрабатывает только этот set. Cell update проходит через общий
`GameState::broadcast_cell_update`, поэтому placement/transform не обходит
registry. Пустой filtered batch выключает schedule до следующего seed/wake.

Перекрывающиеся position-transition окна объединяются в `ActiveFrontier` до
чтения мира: стоимость seed зависит от площади их точного spatial union, а не
от `players * 33 * 33`.

Проверка: `active_frontier_matches_exact_union_of_overlapping_windows`, полный
server suite (`381 passed`, `1 ignored`), strict clippy, architecture guard и
`scripts/dev-smoke.sh`.

## Завершённый кодовый срез

**Bots render immutable read model закрыт.** Renderer сверяет только active
player attributes в коротком ECS read section, затем обходит spatial cache и
кодирует `HB/X` без ECS lock. Observer/byte budgets и legacy HB order сохранены;
тест вызывает batch при удерживаемом ECS write-lock.

Проверка: renderer cache/deadlock regression, strict clippy для server,
architecture guard и wire smoke.

## Завершённый кодовый срез

**Connect presentation snapshot закрыт.** `Connect` фиксирует только
`PlayerView`/chunk index под owner-side session guard и публикует immutable
visible-chunk list. Map/BotSpot/HB encoding перенесены в presentation owner,
Player.Init order и повторный session guard сохранены. Initial building overlay
делает отдельный ECS read snapshot на каждый чанк, поэтому preemption не держит
lock на весь 5x5 view. Полный per-chunk cache остаётся следующим read-model debt,
но command dispatch его больше не выполняет.

Начальный `PlayerView` и visible chunk list заполняются в том же entity write,
что и spawn/reconnect; старый второй `initialize_chunk_visibility` write удалён.
Проверка release login-only burst `100`/`300` clients приведена в P2 выше.

**Intra-chunk movement fast path закрыт.** `Xmov` больше не вызывает
`prepare_chunk_changed` и ECS snapshot, пока source/target остаются в одном
чанке. Chunk sync, clears и index update остаются только при реальном crossing.
`tail` для movement `HB/X` берётся из уже открытого write-lock, без второго
ECS read.
На live baseline `100` клиентов / `2,000 Xmov/s` это убирает до `2,000`
лишних snapshot attempts в секунду; baseline сохранил `30,082/30,082` effects
без disconnect или drain timeout.

Проверка: init/reconnect regression, полный server suite (`368 passed`,
`1 ignored`), strict clippy, architecture guard и wire smoke.

## Завершённый кодовый срез

**Movement fanout coalescing закрыт.** Обычный `Xmov` публикует typed
`MovementFanout`; presentation owner берёт последний packet каждого player
только в непрерывном burst. Первый non-movement event сохраняется как barrier,
поэтому GUI, chunk crossing, chat и другой ordering-sensitive output не
пересекаются. Финальная delivery-очередь следует порядку последних updates, не
числовому player ID. Сaturation по-прежнему disconnect-ит известные recipients.

Проверка: два deterministic теста на latest-wins/last-update order и barrier,
полный server suite (`370 passed`, `1 ignored`), strict clippy, architecture
guard, wire smoke и release movement stress `100/2 000`, `300/6 000` без loss.

## Текущая command boundary

**Chat navigation закрыт.** `Chin`/`Cmen`/`Choo`/`Cpri` больше не запускают
session async tasks: global channels отвечают typed `SessionBatch`, а private и
clan channels идут через `ChatResync`/`ChatMenu`/`ChatPrivate` с completion
session guard. Incremental `Chin` передаёт `lastid` до SQL-запроса, поэтому
reconnect не повторяет уже показанную историю. `Cset` по-прежнему использует
`ChatColorCycle`; обычный `Chat` - `ChatAppend + ChatFanout`.

Проверка: targeted chat/movement tests, strict server clippy и wire smoke.

**Slash-command fallback закрыт.** `/moneyall`, `/skill`, `/role` и `/clan`
имеют отдельные `SaveKind`, completion permit и session-guarded completion;
остальные slash-команды возвращают typed `CommandEffects` без outbox delivery
из simulation apply. `LocalChat` и `ChannelChat` классифицируют durable slash
до apply, поэтому saturation не допускает мутацию без persistence reservation.
`spawn_session_async_task` и `legacy_text` удалены из production slash-path;
старые handlers остаются только под `cfg(test)` до переноса их тест-кейсов.

Проверка: targeted command/admission tests, `cargo check -p openmines-server`,
`cargo test -p openmines-server --all-targets local_chat_slash_is_applied_as_a_typed_command`,
strict workspace clippy, `cargo fmt --all` и `git diff --check`. Старый
`commands_social::handle_chat_command` и async clan helpers сохранены для
legacy regression tests, но исключены из production compilation; production
slash fallback теперь сразу возвращает `CommandEffects`.

**Programmer menu/editor (`Pope`) закрыт.** `OpenProgrammer`, `GUI_ prog`, `PROG` без
выбранной программы и `PCOP` резервируют typed durable work до apply.
`ProgramMenu` читает список, `ProgramCopy` копирует owned source; их completion
при актуальной session отдаёт `GU` или ставит следующий persistence request.
Теперь `openprog:{id}` и `rename:{id}:{name}` также резервируют отдельные
`ProgramOpen`/`ProgramRename`: worker повторяет ownership-проверку, выбирает
программу/selected id или переименовывает её, а completion вызывает прежний
editor apply и сохраняет legacy `#P`/`#p`/`Gu` порядок. Старые GUI handlers
оставлены для legacy regression/fallback paths; production
typed-вход не делает прямой DB-вызов из command apply.

Проверка: admission tests для GUI `prog`, `openprog`, `rename`, `PCOP` и
binary `PROG`; persistence completion-capacity tests для `ProgramOpen` и
`ProgramRename`; targeted `program_` suite, `cargo check -p openmines-server
--all-targets --all-features`.

`PDEL` также закрыт через `ProgramDelete`: ownership delete и conditional
selected-program clear выполняются в persistence worker, completion очищает
только runtime state и не отправляет success wire (как C# reference). Reject и
permanent failure дают прежний `OK` только актуальной session. Legacy
`misc::handle_prog_ty` сохранён для regression path.

`PCOP` теперь также проходит typed `ProgramCopy`: command admission только
валидирует положительный id и резервирует durable work, worker повторно проверяет
ownership исходной программы перед копированием, а completion при актуальной
session повторно ставит typed `OpenProgrammer` в ingress для обновлённого меню.
Reject и permanent failure сохраняют прежние `OK`; legacy `misc::handle_prog_ty` оставлен в test
сборке для regression-проверок, но production command path больше не делает
прямой DB-вызов.

Проверка: admission/completion regression, persistence saturation для
`ProgramOpen`/`ProgramRename`/`ProgramDelete`/`ProgramCopy`, targeted `program_`
suite и strict server clippy.

**Settings save (`save:%R%`) закрыт как typed presentation effect.** Парсинг и
authoritative mutation `PlayerSettings` остаются в существующем
`save_settings`/dirty-snapshot пути, но `GUI_` теперь собирает `#S` и повторное
окно настроек в `PacketBatch` и возвращает `SessionBatch`. Production command
path больше не вызывает `net::session::ui::settings::apply` с прямым `Outbox`;
legacy UI helper сохранён для regression tests.

Проверка: typed settings admission/wire regression, strict server clippy,
rustfmt и `git diff --check`.

**My buildings (`Blds`) закрыт.** Запрос резервирует `BuildingMenu` до apply;
persistence worker читает owned buildings, а completion с session guard строит
прежний `Мои здания` GU и обновляет UI state. Старый renderer оставлен только
под `cfg(test)`, production session task удалён.

**Whois (`Whoi`) закрыт.** До worker kernel снимает имена online-игроков из
ECS; только отсутствующие ID читаются из БД. `Whois` резервирует bounded
persistence slot и completion permit до apply, а completion для актуальной
session строит legacy `NL` в исходном порядке, включая повторные и отсутствующие
ID. Старый session async handler удалён.

Проверка: contract admission и completion-wire regression, `cargo check -p
openmines-server`, rustfmt и `git diff --check`.

**GUI read-навигация клана закрыта.** `Clan`, `clan_menu`, `clan_back`,
`clan_view`, `clan_members`, `clan_invite_list`, `clan_invites_view`,
`clan_requests` и открытие кланового pack идут через bounded `ClanMenu` с
completion permit до apply. Для pack kernel повторяет legacy проверку позиции,
owner/clan access; `OpenPack` резервирует `ClanMenu` консервативно до world
lookup, а не создаёт DB work после admission. Worker получает только immutable
online invite-candidate snapshot; GUI completion строит прежний `GU` только
актуальной session.

Проверка: contract admission для всех входов, completion-wire regression,
`cargo test -p openmines-server clan_ --all-features`, `cargo check -p
openmines-server`, rustfmt и `git diff --check`.

**GUI-мутации клана закрыты.** `clan_create` уже входил через typed slash
command; GUI `clan_request`, invite accept/decline, invite send, request
accept/decline, leave, promote и kick теперь создают `ClanCommand` до любой
DB-работы. Worker повторяет capability/rank проверки и завершает mutation
через session-guarded effect; `accept_clan_invite` использует invite edge, а не
request edge. Старые production async branches удалены; оставшиеся legacy
helpers существуют только в тестовой конфигурации.

Проверка: ingress regression для всех mutation button IDs, stale-session join
completion, `cargo test -p openmines-server clan_ --all-features`,
`cargo test -p openmines-storage accepting_invite_uses_invite_edge_and_joins_player --all-features`,
`cargo check -p openmines-server`, rustfmt и `git diff --check`.

### P0, закрытый перед следующим срезом: programmator due requeue

После перевода programmator на due queue один запуск обрабатывал только первый
action без delay: `Label`/условие или переход к следующей функции не ставили
новый deadline. Программа оставалась `running`, но больше не попадала в
schedule. `programmator_system` теперь requeue-ит следующий step во всех
ветках, пока program остаётся running; deadline без delay равен текущему
monotonic time, с delay - точному `now + delay`.

Тест запускает production `programmator` schedule через `ProgrammatorDueBatch`,
проверяет requeue после action без delay и после function transition. Временно
добавленный `PROGDIAG` dump parsed actions удалён: он создавал строки для
каждого action при старте программы и искажал CPU trace.

## Завершённый кодовый срез

**GUI market mutations на typed command pipeline.** `sell:`, `buy:`, `sellall`,
`getprofit` больше не идут через `handle_gui_button_sync_fast_path` и не делают
прямых ECS мутаций из sync fast path. Вместо этого `apply_gui_button_command`
перехватывает market кнопки и вызывает `apply_market_sell/sell_all/buy/get_profit`,
которые:

1. Создают `PacketBatch` вместо отправки через `Outbox`
2. Делают ECS мутации (economy + building moneyinside)
3. Собирают wire пакеты (`@B`, `P$`, `GU`) в `PacketBatch`
4. Возвращают `CommandEffects` с `GameEvent::SessionBatch`

Добавлен `PlayerCommand::MarketGetProfit` для withdraw profit из здания рынка.
`HorbDelivery` trait изменён с `&Outbox` на `&dyn PacketSink` для поддержки
`PacketBatch`. Read-only tab switching (`sellcrys`/`buycrys`) остаётся в sync fast path.

Проверка: 5 market tests, 637 total tests, strict clippy, fmt, wire smoke.

**Auction grid (`auc`) закрыт как первый auction vertical slice.** Кнопка
`auc` больше не запускает legacy `spawn_gui_async_task`: admission резервирует
`AuctionGrid`, persistence worker читает order-count/min-cost snapshot, а
completion с актуальным `SessionId` собирает прежний HORB inventory grid и
передаёт его через `SessionBatch`. Старый `open_auc_grid` оставлен для
legacy regression tests и не является production entry point. Item/order
navigation (`choose`/`openorder`) и auction mutations (`create`/`bet`) в этот
срез намеренно не включены.

Проверка: admission, completion-HORB и persistence completion-capacity tests,
workspace strict clippy, rustfmt и `git diff --check`.

**Auction item orders (`choose:{item}`) закрыт как read-only continuation.**
После клика по item-grid admission резервирует отдельный
`AuctionItemOrders`, persistence worker читает тот же список ордеров с сортировкой
по `cost`, а session-guarded completion собирает прежний `Auc {item}` HORB
список с теми же `openorder:{id}`, `auccreate:{item}` и `auc` actions. Legacy
`open_item_auc` оставлен для старого regression path; `openorder` и все auction
mutations (`create`/`bet`) намеренно не входят в этот срез.

Проверка: admission, persistence completion-capacity и GU/HORB regression tests;
legacy `auc` tests остаются зелёными.

**Auction order detail (`openorder:{id}`) закрыт как read-only continuation.**
Admission резервирует `AuctionOrder`, persistence worker загружает `OrderRow` и
имя buyer одним typed read-model, а session-guarded completion собирает прежнюю
детальную HORB-страницу с таймером, minimal bet, `aucminbet:{id}`,
`aucbet:{id}:%I%`, `choose:{item}` и строкой `Last bet`. `open_order` оставлен
как legacy regression path; фактические `minimalbet`/`bet` mutations ещё не
переводились.

Проверка: admission, persistence completion-capacity и GU/HORB regression tests;
`auc` и `choose` slices остаются зелёными.

**Auction order creation (`aucsetnum:{item}:{cost}:{num}`) закрыт как mutation
continuation.** Admission теперь резервирует `AuctionOrderCreate`, simulation
атомарно проверяет и списывает предмет, а inventory `IN` уходит тем же legacy
порядком до persistence. Worker вызывает прежний `create_order`; success
completion собирает тот же success-HORB, permanent failure восстанавливает
предмет и отправляет `IN` + `OK`. Session guard не позволяет stale completion
отправить wire в новую сессию, но rollback остаётся authoritative. Legacy
`create_order` сохранён для regression path. Страницы `auccreate` и `aucsetcost`
также собираются как typed presentation effects; старые GUI-функции сохранены
для regression paths.

Проверка: regression-тесты на admission/deduction/success-HORB,
permanent-failure refund и typed creation page, `cargo check -p
openmines-server --all-targets`.

**Auction bets (`aucminbet:{id}`/`aucbet:{id}:{amount}`) закрыты как mutation
continuation.** Оба действия проходят через typed `AuctionBet` admission.
Persistence worker читает актуальный order, вычисляет legacy minimum, проверяет
сумму и snapshot денег, выполняет CAS update и в той же durable operation
возвращает деньги предыдущему buyer с rollback order при ошибке refund.
Completion списывает деньги победившего online игрока, синхронизирует `P$`,
возвращает старому online buyer его `P$` и собирает прежний order-detail HORB.
CAS race/reject/not-found переоткрывают detail через уже перенесённый
`AuctionOrder` read path. Невалидная сумма `aucbet` также переоткрывает detail
через typed read; legacy `place_minimal_bet`/`place_bet` оставлены для
regression paths и не являются production entry point.

Проверка: admission, malformed-input, success wire/money и persistence
completion-capacity tests, strict clippy и rustfmt.

**Kernel owner extraction закрыт как structural slice.** `GameState` больше не
хранит пять кластеров реестров и очередей непосредственно в god-object:

- `PlayerRegistry` владеет active/entity/chunk/bots-render player maps;
- `BuildingIndex` владеет origin/chunk building и BotSpot indexes;
- `CommandIngress` владеет bounded ingress counters, sequence, age metrics и
  queued command broadcasts;
- `DueSchedules` владеет crafting/programmator/hazard schedules;
- `WebSnapshotOwner` владеет immutable `Arc<WebSnapshot>` для web routes.

Публичные `GameState` facades сохранены, поэтому wire и call-site API не
изменены. Срез не заявляет Owned ECS: `EcsWorld` всё ещё под `GameState` lock,
а snapshot extraction по-прежнему выполняется коротким owner-side ECS чтением.
Новые owner-модули подключены в `game/kernel/mod.rs`, старые дубли удалены.

Проверка текущего среза: `cargo check -p openmines-server --all-targets
--all-features`, workspace strict clippy с `-D warnings -W clippy::pedantic
-W clippy::nursery`, `cargo fmt --all -- --check`, `git diff --check`.

**Crafting GUI (`craft_start`/`craft_claim`) и clan GUI mutations закрыты через
typed effects.** Crafting собирает session packets, building save и nearby
block update без прямой доставки из legacy handler; clan mutation buttons
(`clan_request`, invite accept/decline/send) проходят существующий
`ClanCommand` completion path. Legacy handlers сохранены для regression tests.
Полный hook подтвердил `668/668` тестов, 2 skipped и legacy wire smoke.
Попытка перенести `open_buildings` в typed `BuildingMenu` откатана: empty-
building HORB потерял legacy Spot/Up routes, что обнаружил smoke; этот путь
остаётся legacy до сохранения полного wire поведения.

**Пустое сохранение `PROG` также переведено на typed persistence.** Если legacy
клиент присылает нулевой ID выбранной программы, command layer резервирует и
возвращает `ProgramMenu`; старый `program_list_after_empty_save` async fallback
удалён. Wire-последовательность не меняется, а regression test подтверждает
отсутствие прямой доставки из command apply.

**`INVN`/`INCL` переведены на typed session effects.** Авторитетная мутация
`PlayerInventory` и точные legacy `IN` packets теперь возвращаются как
`SessionBatch` с исходным `SessionId`; прямой `player_sender` write из command
apply удалён. Порядок `IN full` и `IN full` → `IN choose/close` сохранён;
`INUS` и inventory-backed building placement остаются отдельными slices.

**`Choo` исправлен через `ChatResync`.** Выбор канала больше не меняет
`PlayerUI.current_chat` до завершения persistence и не открывает ошибочно
`ChatMenu`; typed completion теперь выдаёт legacy-порядок `mO → mU` и только
после session-check фиксирует выбранный канал. Обычный `Chat` не затрагивался.

**Снятие средств из pack переведено на typed effects.** Кнопки
`pack_op:take_money` и `pack_op:take_crys` теперь проходят command
admission, возвращают legacy `P$`/`@B` через `SessionBatch` и сохраняют
обнулённое состояние здания через существующий `Building` save. Legacy
обработчики и их regression paths сохранены. Player economy помечается dirty
для существующего player snapshot пути; атомарная транзакция player+building
не входит в этот срез.

**`Sett`/настройки переведены на typed effects.**
Переключатели auto-dig/aggression и открытие окна настроек возвращают
`SessionBatch` с теми же `BD`/`BA`/`GU`, без прямой доставки из command apply.
`open_buildings` намеренно оставлен на legacy renderer: его empty-building
HORB содержит Spot/Up placement routes, которых нет в typed `BuildingMenu`
completion.

**`ADMN` переведён на typed session effect для presentation-only открытия.**
Команда сохраняет session guard и передаёт существующим admin renderers
`PacketBatch`, поэтому `GU`/`OK` wire и current-window semantics не меняются,
но direct socket write из command apply удалён. Admin mutations (`pack_save`,
`resp_save`, upgrade/market actions) остаются отдельными slices.

**`pack_save` переведён на typed mutation/effect.** Legacy-клиент присылает
`pack_save:cost:...#clan:...#` с завершающим `#`; parser теперь принимает этот
фактический RichList wire. Команда валидирует owner/window и оба поля до ECS
мутации, меняет cost/clan под одним write lock, помечает building dirty,
возвращает один refreshed `GU` через `SessionBatch` и один полный
`SaveCommand::Building`. Wire-порядок и `pack:x:y` current-window semantics
сохранены.

**`resp_save` переведён на typed mutation/effect.** Parser теперь принимает
фактические служебные RichList-сегменты legacy-клиента (`:0` и пустые записи)
вместе с `cost`, `clan`, `clanzone`. Команда сохраняет прежние owner/window и
ошибочные wire-ответы, меняет настройки под ECS write lock, помечает здание
dirty, возвращает один refreshed `GU` и один полный `SaveCommand::Building`.

**Durable Up-кнопки переведены на typed Player persistence.** `upgrade`,
`delete`, `install` и `buyslot` сохраняют текущий `SessionBatch` wire, но теперь
до мутации проходят `SaveKind::Player` admission и после успешного handler-а
возвращают полный `SaveCommand::Player`. `skill:<slot>` оставлен
presentation-only: он не создаёт durable save.

**`resp_bind` переведён на typed Player persistence.** Существующие проверки
типа Resp, dirty semantics и повторное открытие GUI сохранены; команда теперь
резервирует `SaveKind::Player` и возвращает полный snapshot с `resp_x/resp_y`
вместе с прежним `GU` через `SessionBatch`.

**`resp_profit` переведён на составной typed persistence.** Снятие денег теперь
меняет Player и Building под одним ECS write lock, помечает оба владельца dirty
и возвращает один `SaveCommand::RespProfit`. Worker сохраняет пару строк одной
SQLite-транзакцией с rollback при ошибке второй записи; раздельные Player и
Building saves для этой операции не допускаются. Legacy Rust wire сохранён
точно: при положительной сумме `P$ → GU`, при нулевой сумме только `GU`; имя
кнопки и payload клиента не менялись.

**`resp_fill` и `gun_fill` переведены на typed charge-fill effect.** Списание
кристаллов и заряд здания теперь выполняются под одним ECS write lock, оба
dirty-наблюдения и полные snapshots возвращаются через один
`SaveCommand::ChargeFill`, а persistence worker пишет Player+Building одной
транзакцией. `@B → GU` сохранён; gun дополнительно возвращает прежний nearby
`HB/O` через `BlockUpdate` effect. Старые handlers оставлены только для
regression tests.

**Выбор слота Up (`skill:{slot}`) переведён на typed session effect.** Подготовка
`up:{json}` и изменение `current_window` больше не идут через legacy handler и
не создают durable save; command layer возвращает тот же `GU` через
`SessionBatch`.

**`buyslot` переведён на typed Player persistence.** Проверки `creds > 1000` и
лимита слотов, изменение `PlayerSkillsComp`, dirty semantics и `GU`-рендер
сохранены; успешная операция возвращает полный `SaveCommand::Player`, а
неуспешная остаётся тихим legacy no-op. Legacy Up handler для этой кнопки
больше не вызывается из production command path.

**`delete`, `install` и `upgrade` переведены на typed Player persistence.**
Командный путь вызывает отдельные typed mutation helpers, возвращает полный
`SaveCommand::Player` только после успешной мутации и сохраняет legacy wire.
Для `upgrade` проверен точный порядок `P$ → @S → LV → @L → sp → GU`; стоимость
рассчитывается безопасным saturating multiply.

**Building placement completions переведены на typed effects.** Completion после
DB insert теперь возвращает `SessionBatch` и `BlockUpdate`; платная установка
сохраняет `Gu`, refund — `P$ → OK`, а inventory placement — `IN` и nearby `O`.
Прямой `Outbox` из completion удалён, stale session отбрасывается до runtime
spawn.

**`DPBX/OpenBox` переведён на typed presentation effect.** Crystal-box HORB
строится тем же payload, но доставляется через `SessionBatch`; `PlayerUI`
получает прежний `open_box`, direct Outbox из gameplay apply удалён.

Следующий архитектурный срез не смешивать с ECS ownership: продолжать перенос
оставшихся session GUI/chat paths через typed command/admission/apply/effects.

## Wire-decoupling: &Outbox → &dyn PacketSink (полная миграция)

Все game logic функции переведены с `&Outbox` на `&dyn PacketSink`:
- **105 функций** на `&dyn PacketSink`
- **1 функция** на `&Outbox` (только `connect_in_tick` в тестах — нужен `Outbox` для `register_test_outbox`)
- `PacketSink` trait: `Send + Sync` (required for `tokio::spawn`)
- `PacketBatch`: `std::sync::Mutex` вместо `RefCell` (для `Sync`)
- Guard `scripts/guards/no-wire-in-lock.sh`: **0 violations** (depth-tracking, comment-aware)
- Все 22 `send_u_packet`/`send_inventory` внутри `modify_player` closures вынесены

Wire-in-lock violations были в:
- `crafter_gui.rs` (start_craft, refund) — вынесены `send_u_packet(&batch, "@B", ...)` и `send_inventory`
- `slash.rs` (admin give_all) — вынесены `send_u_packet` + `send_inventory`, возврат через `CommandEffects`
- `commands_social/mod.rs` (give, money, moneyall) — `send_inventory` через `PacketBatch`
- `heal_inventory.rs` (consume items) — `send_inventory` через `PacketBatch`
- `auction_gui.rs` (create_order, refund) — `send_inventory` через `PacketBatch`
- `market_gui.rs`, `pack_gui.rs`, `buildings.rs`, `dig_build.rs`, `movement.rs`

Проверка: 637 tests, clippy -D warnings, guard 0 violations.

## Видимые milestones

| Milestone | Пользовательский результат | Статус |
| --- | --- | --- |
| M1. Zero-player idle | почти нулевой CPU, нет 100 Hz ticks и idle warnings | готов |
| M2. Saturation safety | DB stall не вызывает OOM, starvation или item loss | готов |
| M3. Zero scan-all idle | огромный clean world не влияет на maintenance cost | не готов |
| M4. Thin connect | connect storm не блокирует gameplay, init не сидит в owner | частично: init вынесен, entity apply/global ECS lock остались |
| M5. Cheap idle actor | один sleeping player/robot почти ничего не стоит | частично: active/due registries закрыли главные periodic scans |
| M6. Owned ECS | нет external writers и global ECS lock wait | не готов |
| M7. Interest read model | fanout/render зависит от changed/visible chunks | частично: bots cache, init map/BotSpot snapshot и movement coalescing; нет per-chunk model |
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
