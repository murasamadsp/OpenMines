# ECS / архитектура сервера — переделка на масштабируемую модель

## Зачем (диагноз текущего состояния — ИСПРАВЛЕН)

ВАЖНО: первоначальный диагноз был НЕВЕРЕН. Канон «I/O→очередь→единый sim-таск»
**уже существует**:
- conn-таск только декодит и пушит TY в очередь: `connection.rs:171`
  `state.incoming_actions.push(...)` (`IncomingActionsQueue`).
- единый tick-таск (`spawn_game_tick_loop`, lifecycle.rs:214) дренит очередь и
  вызывает `dispatch_ty_packet` (lifecycle.rs:244-249), затем `schedule.run`.
- ВСЕ TY-хендлеры (movement/dig/build/inventory/chat/gui/...) выполняются В ЭТОМ
  tick-таске. 68 `query_player`/84 `modify_player` в основном исполняются здесь,
  а НЕ конкурентно из conn-тасков.

Что реально остаётся проблемой:
- **Доступ к ecs ВНЕ tick-таска**: login/init + disconnect в `player/init.rs`
  (~11 ecs-сайтов: `ecs.write()` spawn + `send_initial_sync` reads + on_disconnect)
  и `outbound/chat_sync.rs` (из login). Это конкурирует с tick'ом за `RwLock` →
  источник фриза C-4 («readers блокировали writer-тик»). connection.rs/auth — 0.
- **Очереди-костыли** (Death/Broadcast/Prog/CellConv/PackResend): следствие того,
  что системы под `schedule.run` (`ecs.write`) не могут ре-входить в `ecs.write`.
  Это разумный обход ограничения bevy_ecs, не «кривость».
- `RwLock<EcsWorld>` нужен ровно из-за п.1 (conn-таск login/disconnect vs tick).

Вывод: ядро здоровое. Узкое место узкое — убрать ecs-доступ из conn-тасков
(login/init/disconnect → в tick-таск), тогда `RwLock` почти без контеншна.

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

## Фазы (ИСПРАВЛЕНЫ под реальную архитектуру)

P1 «command-bus» из первой версии плана ОТМЕНЕНА: `IncomingActionsQueue` уже
делает ровно это для TY-действий. Не дублируем.

- **P2 (теперь первая реальная фаза) — убрать ecs-доступ из conn-тасков.**
  login/init + `send_initial_sync` + on_disconnect (`player/init.rs`,
  `outbound/chat_sync.rs`) сейчас лочат `ecs` в conn-таске, конкурируя с tick'ом
  (C-4 фриз). Цель: 0 `ecs.*lock()` вне tick-таска → `RwLock` почти без контеншна.

  БЛОКЕР (важно): и login, и disconnect ПЕРЕПЛЕТАЮТ async-DB с ecs-операциями:
  - `send_initial_sync`: awaitит `send_chat_login_per_reference` и
    `db.get_program` МЕЖДУ ecs-чтениями и отправкой Init-пакетов (порядок 1:1 C#).
  - `on_disconnect`: `db.save_player().await` рядом с ecs-read/despawn.
  Наивный «перенос в tick» заморозит perf-critical 10ms-цикл на DB-await. НЕЛЬЗЯ.

  ДИЗАЙН (декомпозиция DB ↔ ecs):
  1. lifecycle-очередь `LifeCmd::{Connect{row, programs, chat_hist, tx},
     Disconnect{pid}}` на `GameState`, дренится в tick-ЛОКАЛЬНО (до/после action-
     drain), как `incoming_actions`. Connect ДОЛЖЕН примениться раньше TY этого pid.
  2. conn-таск (login): загружает ВСЮ async-DB (row + programs + chat history)
     ДО enqueue — pre-fetch. cf/Gu шлёт сам (как сейчас). Затем enqueue Connect.
  3. tick: spawn entity + set компонентов + `send_initial_sync_pure` (БЕЗ await,
     строит Init-пакеты из ecs-снапшота + pre-fetched данных, шлёт в tx). Порядок
     пакетов 1:1 сохранить дословно.
  4. conn-таск (disconnect): enqueue Disconnect → tick: extract row (ecs read) +
     despawn + broadcast hb_bot_del; извлечённый row → отдельный async-таск
     `db.save_player` (НЕ в tick).
  ВЕРИФИКАЦИЯ: обязателен коннект живым клиентом (порядок Init/auth — критичен).
  Делать сфокусированно, НЕ в хвосте марафон-сессии (урок отката P1).

  ТРАССИРОВКА ЗАВИСИМОСТЕЙ (готова — следующий заход быстрый):
  - `send_initial_sync` (init.rs:193) — единственный реальный async-DB на логине:
    `send_chat_login_per_reference` → `chat_access`/`load_db_history` (история чата
    из БД). Это ПРЕД-ЗАГРУЗИТЬ в conn-таске и передать в Connect.
  - Блок программы (#p/@P, init.rs:292-305) на свежем логине МЁРТВ:
    `ProgrammatorState::new()` при spawn (init.rs:116) → `selected_id = None`, из БД
    не грузится. `db.get_program` не вызывается. Префетч не нужен.
  - После префетча `send_initial_sync` становится ПОЛНОСТЬЮ sync (ecs-reads + sends,
    без await) → переносится в tick дословно (порядок пакетов байт-в-байт).
  - RECONNECT-ГОНКА решается entity-guard: `LifeCmd::Disconnect{pid, entity}`; в tick
    despawn ТОЛЬКО если `active_players[pid].ecs_entity == entity` (иначе уже
    переподключился — skip). Поэтому connect/disconnect МОЖНО двигать раздельно.
  - `on_disconnect`: `db.save_player().await` → в tick извлечь row (ecs read) +
    despawn + broadcast, а `db.save_player` отдать в `tokio::spawn` (НЕ в tick).
  - ВАЖНО: disconnect-в-одиночку почти не даёт выигрыша (login всё ещё держит ecs
    в conn-таске → RwLock остаётся контендженным). Ценность только когда ОБА
    (connect+disconnect) вне conn-таска. Делать вместе.
- **P3 — Tickrate-бакеты + interest-replication.** Мультирейтный планировщик
  (сейчас всё на 10ms) + дельта-репликация по interest-set (`chunk_players`).
- **P4 — Опциональный пространственный шардинг.** Несколько sim-тасков по зонам.

Доп. (по желанию, не блокер): свести `spawn_game_tick_loop` + будущую
lifecycle-очередь в единый sim-таск как единственного писателя `ecs`.

## Инварианты (нельзя ломать)

- **Wire-формат неизменен** — golden-byte тесты гейтят. Клиент legacy.
- **Формат хранения** (mmap `.mapb` + sqlite) неизменен — roundtrip-тесты.
- Каждая фаза оставляет сервер запускаемым; проверка — реальный запуск + тесты.
- Тестов мало по рантайму/конкуренции → большие фазы проверять запуском, не только `cargo test`.

## Статус
- [x] P1 (command-bus) — ОТМЕНЕНА И ОТКАЧЕНА. Дублировала `IncomingActionsQueue`;
  рероут TADG в отдельный consumer был регрессией (вынес из единого tick-таска).
  Урок: читать существующую архитектуру (incoming_actions + tick loop) ДО кода.
- [x] P2 — ecs-доступ login/disconnect вынесен из conn-тасков в game-tick.
  РЕАЛИЗОВАНО: `LifeCmd::{Connect,Disconnect}` + `life_queue: Mutex<Vec<LifeCmd>>`
  на `GameState`; дренится в `spawn_game_tick_loop` ДО `incoming_actions`, ВНЕ
  `ecs.write()`-блока. `init_player`/`on_disconnect` стали sync-энквью; вся
  ecs-работа в `connect_in_tick`/`disconnect_in_tick` (tick-таск = единственный
  писатель `ecs`). Reconnect-гонка закрыта session-token guard'ом
  (`ActivePlayer.session_token`, монотонный `next_session_token()`): отложенный
  Disconnect сносит entity только если токен в `active_players` всё ещё его.
  УПРОЩЕНИЕ против дизайна: префетч чата НЕ понадобился — на логине
  `current_chat`=="FED", `chat_access` резолвит FED из in-memory `chat_channels`
  (без БД), а блок программы мёртв (`selected_id`=None) → `send_initial_sync`
  стала полностью sync без потери порядка пакетов. `db.save_player` на
  disconnect отдан в `tokio::spawn` (не блокирует 10ms tick).
  ВЕРИФИКАЦИЯ: compile + clippy strict + 146 тестов + boot-smoke зелёные.
  ОСТАЁТСЯ: живой клиент (порядок Init/auth — единственная настоящая проверка).
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
