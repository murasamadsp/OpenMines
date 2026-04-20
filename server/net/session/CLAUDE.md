<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session

## Purpose

Сессионный слой: соединения, TY/GUI/chats, auth и сценарии после входа.

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | Точка экспорта сессионных модулей |
| `connection.rs` | Принятие соединения, handshake, циклы чтения/записи |
| `dispatch/mod.rs` | Маршрутизация TY-событий в игровые обработчики |
| `play/movement.rs` | Обработка движения и обновления видимости чанков |
| `play/dig_build.rs` | Логика копания и постройки |
| `social/buildings.rs` | Пакеты, кланы, постройки |
| `auth/login.rs` | Обработка AU и перевод на auth-GUI |

## Subdirectories

| Directory | Purpose |
|----------|---------|
| `auth/` | До- и после-авторизация |
| `dispatch/` | Диспетчеризация входящих `TY` |
| `outbound/` | Лёгкая исходящая отправка U/J-пакетов |
| `play/` | Игровой мир, движение, копание, чанки, паки |
| `player/` | Жизненный цикл игрока в сессии |
| `social/` | Чаты, кланы, здания, прочая социальная логика |
| `ui/` | GUI-кнопки и инвентарь/лечение |

## For AI Agents

### Working In This Directory

- Протокол — только через `protocol`/`prelude`.
- `connection.rs` меняется только с проверенной моделью handshake (ST/AU/PI/PO).
- На ошибках авторизации не расширять побочные эффекты.

### Testing Requirements

- Прогонять базовый connect/auth flow.
- Проверить cleanup `active_players` и сохранение в БД.

### Common Patterns

- GUI/auth переходы через `AuthState`.
- Отправлять только сериализованные пакеты (`send_u_packet`, `send_b_packet`).

## Dependencies

### Internal

- `server/net`
- `server/protocol`
- `server/game`
- `server/db`

### External

- `tokio`
- `tracing`

<!-- MANUAL: -->
