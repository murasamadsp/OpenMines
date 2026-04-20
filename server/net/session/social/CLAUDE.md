<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/social

## Purpose

Чат, команды, кланы, здания и паки.

## Key Files

| File | Description |
|------|-------------|
| `buildings.rs` | Развёртывание/удаление строений, проверки прав |
| `clans.rs` | CRUD кланов, заявки, join/leave, админские действия |
| `misc.rs` | Чат, каналы, авто-диг, команды (`/give`, `/tp`, `/clan` и др.) |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Проверять кластеры/постройки по правам и позиции.
- Новые чат-команды: валидация + анти-абьюз.

### Testing Requirements

- Проверять, что `clan/buildings` не ломают мир и чаты.
- Проверять игровой `OK`-фидбек на ошибки.

### Common Patterns

- Разделять бизнес-логику и отправку пакетов.
- DB ошибки — с понятным fallback.

## Dependencies

### Internal

- `server/net/session`
- `server/db`
- `server/game`

### External

- `tracing`
- `rand`

<!-- MANUAL: -->
