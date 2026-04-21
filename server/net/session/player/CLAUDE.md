<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/player

## Purpose

Жизненный цикл игрока после авторизации: init/save/disconnect.

## Key Files

| File | Description |
|------|-------------|
| `init.rs` | Вход в мир (`init_player`): ECS spawn, MaxHealth recalc, `send_initial_sync` (порядок 1:1 с C# `Player.Init`), reconnect cleanup, `on_disconnect` |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Порядок пакетов в `send_initial_sync` — 1:1 с C# `Player.Init()`. Не менять без сверки.
- `init_player` спавнит ECS entity с 12+ компонентами, включая `ProgrammatorState` и `PlayerCooldowns` (c190_stacks).
- При disconnect — persist state до cleanup.
- Reconnect: старая сессия чистится (ECS despawn, chunk_players, broadcast hb_bot_del).

### Testing Requirements

- Проверять reconnect без дублей сессий.
- Проверять сохранение денег/HP/инвентаря на `on_disconnect`.

### Common Patterns

- Формировать `ActivePlayer` полностью до начала игрового цикла.
- Убирать игрока из пространственного индекса до освобождения `active_players`.

## Dependencies

### Internal

- `server/net/session`
- `server/db`

### External

- `tracing`

<!-- MANUAL: -->
