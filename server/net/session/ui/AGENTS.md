<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/ui

## Purpose

Клиентские UI-события: кнопки GUI и инвентарь.

## Key Files

| File | Description |
|------|-------------|
| `gui_buttons.rs` | Разбор и обработка кнопок интерфейса |
| `heal_inventory.rs` | Лечение и инвентарь (`INVN`, `INUS`, `INCL`) |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Проверять, что GUI не ломает auth-сценарии.
- UI-экшены должны быть идемпотентными (повторный клик безопасен).

### Testing Requirements

- Прогонять open/close UI-инвентаря.
- Проверять порядок пакетов `@L/@B` и `GU/Gu`.

### Common Patterns

- Сначала auth/context, потом бизнес-логика.
- Валидацию кнопок делать через префиксные разборщики.

## Dependencies

### Internal

- `server/net/session`
- `server/net/session/social`
- `server/net/session/play`

### External

- `serde_json`
- `tracing`

<!-- MANUAL: -->
