# ECS / архитектура сервера — переделка на масштабируемую модель

## Зачем (диагноз текущего состояния)

Сейчас `bevy_ecs` используется как **глобально-залоченный мешок компонентов**:

- `GameState.ecs: RwLock<EcsWorld>` — один лок на весь мир.
- ~152 места в net-слое (`query_player`×68 + `modify_player`×84) берут read/write
  на ВЕСЬ мир ради доступа к одному игроку из per-connection async-тасков.
- Системы (9 шт) гоняются на тике под `ecs.write()` через `schedule.run()`.
- Чтобы система не словила вложенный `ecs.write()` → дедлок, мутации отложены в
  очереди-костыли: `DeathQueue`, `BroadcastQueue`, `ProgrammatorQueue`.
- Зафиксированные баги от этого: фриз «readers блокировали writer-тик» (C-4),
  «нельзя вызывать handle_death изнутри schedule.run».

Потолок такой схемы — сотни игроков (контеншн глобального лока), и каждый новый
кусок логики тащит boilerplate + риск дедлока.

## Целевая архитектура

Принцип: **симуляция владеет миром единолично; сеть — только I/O; общение через
каналы.** Это убирает глобальный RwLock и очереди-костыли, и открывает шардинг.

```
[TCP conn task]  --decode-->  SessionCommand  --mpsc-->  [SIM LOOP owns World]
   (per player)                                              | applies authoritatively
[TCP conn task]  <--bytes--  outbound mpsc  <--replication-- | (validation здесь)
```

- **Connection task = чистый I/O.** Читает фреймы → декодит в `SessionCommand`
  → пушит в общий inbound-канал. Исходящее: пишет в свой `UnboundedSender<Vec<u8>>`
  (уже есть). Никаких `ecs.*lock()` из conn-тасков.
- **SessionCommand** — enum игровых намерений (`Move{dir}`, `Dig{dir}`,
  `Build{..}`, `UseItem{..}`, `Chat{..}`, `Gui{..}`, `Connect{..}`, `Disconnect`).
  Net переводит wire→Command; симуляция применяет и валидирует (server-authoritative).
- **Sim loop владеет `World`** (без RwLock, обычный `&mut`). Дренит команды,
  гоняет системы.
- **Multi-tickrate:** системы в бакетах по частоте — движение/дигание (по
  команде), физика (sand/alive/acid — свои рейты), почасовые (building damage).
  Моделируется несколькими `Schedule` + run-conditions.
- **Interest-managed replication:** изменения уходят только игрокам, в чьём
  interest-set (соседние чанки) они произошли. `chunk_players` уже есть — оформить
  как явный replication-шаг с дельтами.
- **ECS-движок: оставляем `bevy_ecs`** (решение принято, обосновано ресёрчем).
  Проблема не в либе, а в модели доступа (shared RwLock из conn-тасков). Сообщество
  прямо рекомендует канон: один Bevy-app как authoritative-сервер на fixed tick
  loop + tokio/MPSC декаплинг сети (bevyengine/bevy discussion #21820). 10k+
  сущностей bevy_ecs итерирует за микросекунды — узкое место не движок, а лок и
  сетевой I/O. Менять либу (hecs/sparsey быстрее в микробенчах, но теряем
  параллельный планировщик/change-detection и платим churn'ом) — пустая работа.

### Путь к масштабу (вширь, не в одном процессе)

Когда мир за sim-loop за каналами — следующий шаг шардинг: несколько sim-тасков,
каждый владеет регионом (зоной чанков); межзонные переходы — через команды. 10k+
достигается шардингом + interest management, а не «толще лок».

## Фазы (каждая: компилится + тесты + проверка запуском)

- **P1 — Command-bus + sim-loop скелет (без смены поведения).**
  Ввести `SessionCommand` + общий inbound mpsc + один sim-таск, который дренит и
  применяет команды, ВЫЗЫВАЯ существующие хендлеры (пока ещё через ecs-локи).
  Прогнать через него 1-2 TY-события как пруф. Всё остальное на старом пути.
- **P2 — Перенос владения World в sim.** Sim — единственный писатель. Net-side
  `query_player/modify_player` переводятся хендлер-за-хендлером на команды/снапшоты.
  Убрать `ecs.write()` из conn-тасков. Снять очереди-костыли (Death/Broadcast/Prog).
- **P3 — Tickrate-бакеты + interest-replication.** Формализовать мультирейтный
  планировщик и дельта-репликацию по interest-set.
- **P4 — Опциональный пространственный шардинг.** Несколько sim-тасков по зонам.

## Инварианты (нельзя ломать)

- **Wire-формат неизменен** — golden-byte тесты гейтят. Клиент legacy.
- **Формат хранения** (mmap `.mapb` + sqlite) неизменен — roundtrip-тесты.
- Каждая фаза оставляет сервер запускаемым; проверка — реальный запуск + тесты.
- Тестов мало по рантайму/конкуренции → большие фазы проверять запуском, не только `cargo test`.

## Статус
- [~] P1 — Command-bus + sim-loop скелет. СДЕЛАНО частично: `SessionCommand`
  (`game/command.rs`) + шина `GameState.command_tx/command_rx` +
  `spawn_command_consumer` (`net/lifecycle.rs`); TY `TADG` рероут через шину как
  пруф. Boot-проверка: сервер доходит до `TCP listening`, consumer стартует,
  game-tick здоров (over_budget=0). Осталось в P1: прогнать остальные TY-события
  через шину; проверить TADG round-trip живым клиентом.
- [ ] P2 — Владение World в sim, снятие очередей-костылей
- [ ] P3 — Tickrate-бакеты + interest-replication
- [ ] P4 — Шардинг (опционально)

## Смежные задачи (всплыли по ходу — верифицировать отдельно)
- **Генерация мира медленная/возможно криво портирована** (2048² регенится
  >50с на старте; юзер: «плохо портирована с оригинала»). Свериться с C#
  `World.cs`/генератором (`server/world/generator.rs`, `anl.rs`, `sectors_gen.rs`)
  — и по скорости, и по соответствию 1:1.
- Крейт-косметика (отдельно от ECS): `indexmap` (programmator function_order),
  `thiserror` (db/players.rs 16 unwraps), `pretty_assertions` (dev),
  `building_cells()`→`impl Iterator`. См. аудиты 3 агентов в истории сессии.
