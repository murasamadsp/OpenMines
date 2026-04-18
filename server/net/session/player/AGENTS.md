<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/player

## Purpose

Жизненный цикл игрока после авторизации: init/save/disconnect.

## Key Files

| File | Description |
|------|-------------|
| `init.rs` | Вход в мир (`init_player`) и удаление сессии (`on_disconnect`) |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Сохранять пакетный порядок в `init_player`.
- При disconnect — persist state до cleanup.

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
