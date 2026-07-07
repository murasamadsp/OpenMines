# OpenMines Audit State

Дата актуализации: 2026-07-07.

Цель файла: фиксировать, что уже проверено и закрыто, чтобы следующие проходы не
повторяли аудит и не принимали устаревшие хендоффы за факт.

## Закрыто и проверено

- Game tick panic isolation: supervisor рестартит tick-task, TY panic изолируется
  spawned task, Bevy schedules обёрнуты в `catch_unwind`.
- Реген мира сбрасывает игроков на `gameplay.spawn`; стартовые здания используют ту
  же typed-координату.
- `cells.json` fail-fast валидируется: нет дыр `0..125`, неизвестный id не
  fallback'ится на cell `0`.
- `buildings.json` fail-fast валидируется: обязательные pack-ключи, запрет
  неизвестных ключей, `code` совпадает с `PackType`.
- ECS systems больше не используют `Arc<GameState>` как ресурс: используются
  `WorldResource`, `BoxIndexResource`, `BoxPersistQueue`.
- `gameplay.cooldowns`, `skills`, `spawn`, `programmator`, `schedules` вынесены в
  typed config без silent defaults.
- `WorldProvider` имеет typed cell API: `get_cell_typed` / `set_cell_typed`.
- Live-path gameplay/session cell access переведён на `CellType` API; raw
  `get_cell/set_cell` остаётся только на низкоуровневой map/wire boundary и в тестах.
- Live-path операции, которые меняют и тип клетки, и durability вместе (sand
  move, alive actions, boulder push, geo placement, build placement/upgrade,
  delayed military conversion), пишут через `WorldCell`. Прямой
  `set_durability` остался только в тестах и низкоуровневом `WorldProvider`.
- Box-клетки больше не требуют от callers вручную синхронизировать mmap cell и
  `box_index`: live create/remove paths используют `GameState::put_box_cell` /
  `remove_box_cell`.
- Runtime-индексы зданий (`building_index` + `chunk_buildings`) сведены в методы
  `register_building_entity` / `remove_building_entity` / `move_building_entity`,
  чтобы callers не синхронизировали два кэша вручную.
- `building_index` использует typed key `WorldPos`, как и `box_index`; сырой
  `(i32, i32)` больше не является типом центрального индекса зданий.
- Для origin lookup здания добавлены `GameState::building_entity_at` /
  `has_building_origin`; простые live-path проверки больше не лезут в
  `building_index` напрямую.
- Для read-only запросов к компонентам здания добавлен
  `GameState::query_building_opt`; `play/packs.rs` больше не обращается к
  `building_index` напрямую.
- Для обхода зданий добавлен `GameState::building_entities_snapshot`; прямой
  доступ к `building_index` теперь остаётся внутри `GameState`, а session-код
  использует boundary-методы.
- Активные consumable overlay-паки (boom/protector/razryadka) переведены на
  typed key `WorldPos` и доступны session-коду только через методы `GameState`.
- BotSpot runtime (`botspot_index` + `chunk_botspots`) закрыт за `GameState`:
  создание Spot-bot, регистрация, удаление и HB-снимки больше не читаются и не
  синхронизируются напрямую из session-кода.
- Session-код больше не передаёт сырой `chunk_buildings` в static helpers:
  проверки pack footprint и AccessGun идут через instance boundary `GameState`,
  включая места, где уже удерживается ECS lock.
- Runtime-индексы `building_index`, `chunk_buildings`, `botspot_index`,
  `chunk_botspots`, `box_index`, `box_persist_q` больше не являются публичными
  полями `GameState`; новые внешние обращения теперь ловятся компилятором.
- Static helpers, требующие сырой `chunk_buildings`, приватны внутри
  `GameState`; внешний код может идти только через instance boundary.
- Player spatial index `chunk_players` закрыт за `GameState`: регистрация,
  снятие, полный cleanup stale entries и snapshot игроков в чанке больше не
  выполняются напрямую из session-кода.
- Player sender index `player_tx` закрыт за `GameState`: проверка online,
  получение sender, регистрация и снятие sender больше не идут через публичный
  `DashMap`.
- Mmap-футпринт зданий пишется/очищается через `GameState::place_building_footprint`
  / `clear_building_footprint`; session-модуль построек больше не держит ручной
  цикл `set_cell_typed + broadcast` для footprint.
- ECS-компоненты новых зданий создаются через `spawn_building_from_extra` /
  `BuildingSpawnSpec`; session paths больше не дублируют tuple компонентов здания.
- Runtime commit нового здания (`ECS spawn + runtime индексы + mmap footprint`)
  сведён в `GameState::spawn_building_runtime`; session paths после DB insert
  больше не вызывают эти шаги по отдельности.
- Live creation здания (`DB insert + runtime commit`) сведён в
  `GameState::insert_building_runtime`; session paths оставляют у себя только
  возврат денег/предметов при ошибке БД.
- Runtime removal здания (`runtime индексы + ECS despawn + mmap footprint`) сведён
  в `GameState::remove_building_runtime`; destroy/protector paths больше не
  дублируют ручной cleanup runtime-слоёв.
- Normal destroy здания (`DB delete + runtime cleanup`) сведён в
  `GameState::delete_building_runtime`; дропы и возврат предметов остаются в
  caller'е, потому что зависят от причины сноса.
- Protector gate destroy больше не делает detached DB delete: он await'ит
  `GameState::delete_building_runtime` внутри своей async detonation task.
- Веб-админка уже умеет менять роль online/offline игрока через
  `POST /api/players/:id/role`; frontend select есть в `server/admin/app.js`.

## Не считать закрытым

- Единый владелец клетки не готов: тип клетки, durability, здания, SQLite и кэши
  всё ещё живут в разных местах.
- Полный единый владелец клетки не завершён; `WorldCell` уже объединяет
  type/durability для live-path, box-клетки получили первый boundary, центральные
  индексы box/building используют `WorldPos`, runtime индексы зданий/BotSpot,
  mmap footprint, live creation, destroy и runtime removal зданий сведены в
  helper boundary. Rollback/compensation после runtime failure ещё не включён в
  эту authoritative операцию из-за разных сценариев возврата ресурсов/ошибок.
- Однопоточный 10ms tick остаётся архитектурным потолком. Не трогать без метрик
  нагрузки или конкретного hot path.
- Tickprof `side` hot path не закрыт: нужен живой лог с per-section timings.
- Программатор не считать “готовым” без ручного wire/GUI сценария по клиенту и
  референсу.

## Следующий правильный порядок

1. Дальше расширять boundary к `WorldCell { type, durability, pack }`: следующий
   слой — операции зданий с DB insert/delete и rollback/compensation, не
   переписывая весь мир одним махом.
2. По tickprof сначала собрать лог, потом оптимизировать конкретную секцию.
3. Любую намеренную девиацию от C#/JS reference сразу записывать в
   `docs/DEVIATIONS.md`.
