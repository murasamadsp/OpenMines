<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/game

## Purpose

Состояние игры и игровые сущности.

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | Основной контейнер game state |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Менять `GameState` атомарно.
- Проверять протокол при смене типов сообщений.

### Testing Requirements

- Проверить инициализацию `GameState` из `main.rs`.

### Common Patterns

- Сильная типизация для игровых сущностей и действий.

## Dependencies

### Internal

- `server/world`, `server/db`, `server/net`

### External

- `tokio`, `tracing`

<!-- MANUAL: -->
