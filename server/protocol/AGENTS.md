<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/protocol

## Purpose

Сетевые протоколы клиента и сервера.

## Key Files

| File | Description |
| - | - |
| `mod.rs` | Экспонирование протокольных типов |
| `packets.rs` | Сериализация/десериализация игровых пакетов |

## Subdirectories

| Directory | Purpose |
| - | - |
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Любые изменения формата пакета — согласовать с клиентом.
- Сохранять обратную совместимость для внешних клиентов.

### Testing Requirements

- Проверить сериализацию в тестовом обмене.

### Common Patterns

- Явные структуры пакетов и serde-based сериализация.

## Dependencies

### Internal

- `server/net`, `server/world`

### External

- `bytes`, `serde`, `serde_json`

<!-- MANUAL: -->
