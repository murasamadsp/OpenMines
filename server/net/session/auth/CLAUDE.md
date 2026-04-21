<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/auth

## Purpose

Auth и auth-GUI до входа в мир.

## Key Files

| File | Description |
|------|-------------|
| `login.rs` | Обработка AU и маршрутизация в `gui_flow`/`player::init` |
| `gui_flow.rs` | Экран входа/регистрации и шаги ввода ник/пароля |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Синхронизировать auth-формат с клиентским протоколом.
- Сохранять безопасный fallback на ошибки login/регистрации.

### Testing Requirements

- Прогнать `NoAuth` и `Regular`.
- Проверить GUI-fallback на неверный `token`/участие.

### Common Patterns

- Ошибки auth через `OK` + `bot_info`.
- После успеха — сразу `init_player`.

## Dependencies

### Internal

- `server/net/session/player`
- `server/net/session`
- `server/db`
- `server/game`

### External

- `tracing`

<!-- MANUAL: -->
