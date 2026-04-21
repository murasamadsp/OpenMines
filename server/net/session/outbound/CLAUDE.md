<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/outbound

## Purpose

Исходящая синхронизация чата, инвентаря и базовых состояний.

## Key Files

| File | Description |
|------|-------------|
| `chat_sync.rs` | Инициализация и синхронизация чат-каналов |
| `inventory_sync.rs` | Синхронизация инвентаря и выбранного слота |
| `player_sync.rs` | `send_player_speed` (формула 1:1 с C#), `send_player_health`, `send_player_level`, `send_player_skills`, `send_player_basket` |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Не смешивать бизнес-логику с сериализацией `U`.
- Форматы пакетов держать стабильными (`protocol`).

### Testing Requirements

- Проверять чат и инвентарь после входа/изменений.

### Common Patterns

- Вызывать отправку через `send_u_packet` с готовыми payload-фабриками.

## Dependencies

### Internal

- `server/net/session`

### External

- `tracing`

<!-- MANUAL: -->
