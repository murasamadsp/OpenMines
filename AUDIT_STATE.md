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
- Веб-админка уже умеет менять роль online/offline игрока через
  `POST /api/players/:id/role`; frontend select есть в `server/admin/app.js`.

## Не считать закрытым

- Единый владелец клетки не готов: тип клетки, durability, здания, SQLite и кэши
  всё ещё живут в разных местах.
- Полный единый владелец клетки не завершён; `WorldCell` уже объединяет
  type/durability для части live-path, но ещё не включает pack/DB/index state в один
  authoritative boundary.
- Однопоточный 10ms tick остаётся архитектурным потолком. Не трогать без метрик
  нагрузки или конкретного hot path.
- Tickprof `side` hot path не закрыт: нужен живой лог с per-section timings.
- Программатор не считать “готовым” без ручного wire/GUI сценария по клиенту и
  референсу.

## Следующий правильный порядок

1. Спроектировать фасад `WorldCell { type, durability, pack }` как единый read/write
   boundary, не переписывая весь мир одним махом.
2. По tickprof сначала собрать лог, потом оптимизировать конкретную секцию.
3. Любую намеренную девиацию от C#/JS reference сразу записывать в
   `docs/DEVIATIONS.md`.
