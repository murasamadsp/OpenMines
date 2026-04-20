<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/world

## Purpose

Мир: размеры карты, клетки и стартовая инициализация.

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | API мира и фабрики |
| `cells.rs` | Загрузка и парсинг `cells.json` |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Менять формат мира только с `config.json`.
- Проверять метрики чанков и координат.

### Testing Requirements

- Проверить размеры `world` и `chunks`.

### Common Patterns

- World state держать через явный конструктор.

## Dependencies

### Internal

- `server/AGENTS.md`
- `server/config.rs`

### External

- `serde`, `serde_json`

<!-- MANUAL: -->
