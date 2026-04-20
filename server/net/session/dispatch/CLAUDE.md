<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/dispatch

## Purpose

Маршрутизация входящих `TY`-событий в обработчики.

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | `handle_ty` и маршруты событий |
| `ty.rs` | (внутренний файл) разбор payload и вызов `handle_x*` |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Не трогать `TY`-маппинг без проверки клиента.
- Новые `TY` — через единый путь и явный `not implemented`.

### Testing Requirements

- Прогонять вход/движение/копание/chat через `TY`.
- Проверять обработку неизвестных событий.

### Common Patterns

- Логи с `TY`, `x`, `y`, `pid` для диагностики.
- Короткие хелперы, без тяжелой логики в матчине.

## Dependencies

### Internal

- `server/net/session`
- `server/net/session/play`
- `server/net/session/ui`
- `server/net/session/social`

### External

- `tracing`

<!-- MANUAL: -->
