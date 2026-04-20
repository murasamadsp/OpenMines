<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/db

## Purpose

Доступ к SQLite и инициализация БД.

## Key Files

| File | Description |
| - | - |
| `mod.rs` | Основной модуль базы данных |

## Subdirectories

| Directory | Purpose |
| - | - |
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Не вносить миграции без согласованной схемы.
- Проверять корректное закрытие и обработку ошибок.

### Testing Requirements

- Build + прогон с временной БД.

### Common Patterns

- `rusqlite` с явным управлением подключениями.

## Dependencies

### Internal

- `server/AGENTS.md`
- `config.json`

### External

- `rusqlite`, `serde`

<!-- MANUAL: -->
