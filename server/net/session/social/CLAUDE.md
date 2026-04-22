<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/social

## Purpose

Чат, команды, кланы, здания и паки.

## Key Files

| File | Description |
|------|-------------|
| `buildings.rs` | Развёртывание/удаление строений, проверки прав |
| `chat.rs` | Локальный чат (HB bubble), канальный чат (FED/DNO/CLAN), `Chin` init, channel switch |
| `clans.rs` | CRUD кланов, заявки, join/leave, админские действия |
| `commands.rs` | Слэш-команды (`/give`, `/money`, `/tp`, `/heal`, `/clan`, `/pack`, `/admin`), `send_ok`, `is_admin_command` |
| `misc.rs` | Auto-dig toggle/set, Whoi, программатор TY (PROG/PDEL/pRST/PREN), настройки TY |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- `commands.rs` экспортирует `send_ok`, `send_admin_help`, `is_admin_command` — используются из dispatch и ui.
- `chat.rs` зависит от `commands.rs` (handle_chat_command для слэш-команд в чате).
- Смерть/урон перенесены в `play/death.rs`, гео в `play/geo.rs`.
- Новые чат-команды добавлять в `commands.rs`.

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
