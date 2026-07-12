# OpenMines Server Consistency Plan

## Статус

Это companion-план к `SIMULATION_KERNEL_PLAN.md`. Simulation plan меняет
владение состоянием и путь к multicore. Этот документ фиксирует форму кода,
чтобы после миграции сервер не остался набором несовместимых локальных стилей.

Текущую готовность и порядок срезов этот файл не определяет. Единственный
операционный checkpoint - `SERVER_MIGRATION_STATUS.md`.

Цель не в одинаковой раскладке файлов и не в уменьшении строк любой ценой.
Цель: один gameplay flow читается в одном направлении, а запрещённый обход
невозможно случайно добавить обратно.

## Подтверждённая проблема

Сейчас одновременно живут несколько моделей исполнения:

- `CommandEffects` и bounded owner runtimes;
- прямой вызов session-handler с `Outbox`;
- Bevy resource queues и отдельный side-effect flush;
- legacy async tasks и direct DB/wire work в chat/GUI/auction paths;
- async DB-first task с возвратом через `Apply*` player command.

Большой файл неприятен, но сам по себе не является архитектурным дефектом.
Главная entropy возникает, когда один feature может одновременно мутировать
ECS/world, отправлять wire, запускать task и писать в DB.

## Каноническая грамматика

```text
adapter parse
  -> typed input with identity
  -> bounded admission
  -> authoritative apply
  -> typed { events, saves, due }
  -> presentation/persistence adapters
```

Для authenticated client input identity является общей оболочкой, а не полем
случайных variants:

```rust
AuthenticatedCommand {
    session_id,
    player_id,
    action: ClientAction,
}
```

Разные источники не смешиваются в один бесконечный enum:

- `ClientCommand` - уже разобранное действие клиента;
- `DueAction` - принятое delayed действие simulation owner;
- `PersistenceCompletion` - результат durable worker;
- `AdminCommand` - действие control plane.

`apply` не делает `await`, `spawn`, `sleep`, SQL, send/broadcast и raw wire
encoding. Он получает mutable state своего owner и возвращает typed outcome.

## Форма feature-модуля

Обязательна форма API, а не пять файлов на каждую мелкую сущность:

```text
feature/
  mod.rs       private implementation + small facade
  command.rs   optional: typed parsing/validation
  apply.rs     optional: authoritative mutation
  effects.rs   optional: outcomes/domain errors
  view.rs      optional: immutable presentation DTO
  tests.rs     optional: large feature tests
```

Маленький feature может жить в одном файле. Разделение обязательно, когда в
одном модуле смешаны несколько доменов или capability zones. Делить на
`handlers/helpers/utils` запрещено как бессодержательную классификацию; делить
нужно на market, crafting, storage, teleport, programmer и другие features.

Public facade минимален. Внутренние функции по умолчанию private или
`pub(super)`. Domain error локален feature-модулю:

```rust
fn prepare(...) -> Result<Intent, FeatureError>;
fn apply(...) -> Result<Effects, FeatureError>;
```

Только wire adapter решает, какой пакет соответствует `FeatureError`.

## Контракт пользовательского ввода

Любое значение из GUI, slash-команды, console или web проходит один порядок:

```text
parse -> validate explicit domain/range -> typed command
```

- значение выше или ниже диапазона возвращает видимую ошибку с допустимыми
  границами и не создаёт mutation, save, due action или completion;
- clamp/saturation пользовательского значения запрещены: они допустимы только
  для внутренних вычислений и outbound-представления уже валидного state;
- проверка выполняется до authoritative mutation и bounded admission;
- для каждого числового поля тестируются максимум и первое значение выше него.

Полный аудит форм является отдельным consistency-треком и не прерывает
simulation migration; затронутый flow обязан соблюдать контракт сразу.

## Capability zones

| Zone | Может | Не может |
| --- | --- | --- |
| Transport adapter | decode, auth envelope, bounded enqueue | ECS/world/DB mutation |
| Simulation/domain | authoritative state, deterministic apply | Tokio, SQL, Outbox, raw wire |
| Persistence | storage transaction, retry, typed completion | ECS/world/network mutation |
| Presentation | immutable DTO, protocol builders, SessionHub delivery | ECS/world/DB access |
| Admin adapter | parse request, render outcome/read snapshot | direct gameplay mutation |

Нужен один declarative architecture gate по этим zones. Отдельный линтер на
каждую сущность сам станет новым источником entropy.

## Mechanical gates

Gate печатает ratchet counts для всего migration period. После переноса одного
vertical slice его локальный forbidden count обязан стать `0`, а старый путь
удаляется в том же срезе.

| Gate | Definition of zero |
| --- | --- |
| `external_ecs_mutators` | ECS меняет только simulation owner |
| `gameplay_async` | в domain/simulation нет Tokio, spawn и sleep |
| `direct_gameplay_db` | write capability есть только у persistence owner |
| `direct_apply_output` | apply не импортирует Outbox/SessionHub/net send |
| `unbounded_cross_owner_queues` | у каждой очереди есть capacity и policy |
| `raw_authenticated_actions` | все client actions несут common session envelope |
| `scan_all_idle_work` | clean/sleeping actors не обходятся periodic scan |
| `raw_server_events` | event literals находятся только в protocol/adapter |

Для bounded queue обязательны capacity, saturation policy, depth, oldest age,
per-drain work budget и saturation/burst tests. Ограничение памяти без
ограничения drain work не защищает tick. Tick-local `Vec` одного owner не
считается cross-owner queue.

Дополнительные ratchets:

- число mutable capabilities и `Arc<GameState>` references только уменьшается;
- новые `#[allow]` не добавляются;
- architecture truth обновляется в том же commit, что и boundary;
- deterministic state/event digest обязателен до spatial sharding.

## Рабочие треки

### C0. Architecture truth

- поддерживать `docs/ARCHITECTURE.md` как описание текущего кода;
- будущую модель и незакрытые gates держать только в планах;
- не называть transitional adapter завершённым ownership boundary.

### C1. Capability guard

- заменить path-by-path проверки таблицей zones и policies;
- считать bypass по категориям и запрещать рост baseline;
- после миграции feature переключать его policy с ratchet на exact zero.

### C2. Common envelopes и outcomes

- все client actions получают `SessionId` через общий envelope;
- `PlayerCommand` разделяется по источникам input;
- прямые output queues сходятся в один typed effect pipeline;
- error semantics становятся видны из сигнатур.

### C3. GUI по features

- teleport является pilot vertical slice;
- `gui_buttons.rs` разделяется по market/crafting/storage/teleport/programmer;
- router только классифицирует typed command;
- async DB и presentation выносятся до физического распила feature;
- механический распил legacy handlers без boundary migration не считается.

### C4. Admin control plane

```text
AdminRequest -> AdminService -> AdminOutcome
```

Console, slash и web только адаптируют input/output. Registry не считается
единым control plane, пока исполнение остаётся в трёх местах.

### C5. Protocol vocabulary

- server packet builders принадлежат `openmines-protocol`;
- raw `"GU"`, `"@T"`, `"#p"` остаются только внутри protocol/wire adapters;
- golden tests сохраняют wire byte-for-byte.

### C6. Boundary newtypes

`ItemId`, `PackCode`, `CellPos` и другие newtypes вводятся после стабилизации
feature boundary, только если исключают реальный неверный путь. Типы не должны
маскировать всё ещё смешанное ownership.

`programmator.rs` не распиливается механически до due-actor этапа simulation
plan: иначе новая структура почти наверняка будет переделана второй раз.

## Consistency-инварианты ближайших срезов

Фактический порядок берётся только из `SERVER_MIGRATION_STATUS.md`. На текущем
checkpoint Boom/Protector/Raz, typed building completion и event-driven wait уже
перенесены. Consistency-обязанности ближайших simulation slices:

- Graceful DueAction drain использует тот же command/apply/effects путь и
  owner-local FIFO, а не новый shutdown-only handler.
- Bounded ingress получает typed workload classes и явную overload policy, а не
  общий `try_send` с silent drop.
- Dirty registries закрывают старые scan-all capabilities типами и guards.
- External ECS writers удаляются до spatial ownership.
- GUI/chat/admin мигрируются по одному vertical feature с немедленным удалением
  старого path.

Consistency track не задерживает native multithreading декоративным cleanup.
Он закрывает prerequisites: один owner, один effect path, bounded queues и
детерминированный replay. После этого spatial workers не умножат существующие
гонки и стили на число потоков.

## Definition of done

- новый gameplay flow добавляется в одном feature facade;
- изменение не требует одновременно править TCP loop, lifecycle, DB task и ECS
  transport component;
- apply имеет один typed input и один typed outcome;
- old capability path отсутствует, а не помечен deprecated;
- architecture gate доказывает zones, а не только печатает статистику;
- документация отличает текущее состояние от целевой модели.
