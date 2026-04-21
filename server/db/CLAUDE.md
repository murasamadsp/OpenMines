<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/db

## Purpose

SQLite (WAL mode). Таблицы: players, buildings, clans, chats, chat_messages, boxes, programs.

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | `Database` struct, миграции, координатор запросов |
| `players.rs` | CRUD игроков, `PlayerRow`, `SkillState` |
| `buildings.rs` | CRUD зданий, `BuildingRow` |
| `clans.rs` | CRUD кланов, ранги, заявки |
| `chats.rs` | Каналы чата, сообщения |
| `boxes.rs` | Crystal boxes (ячейка 90), `BoxRow` |
| `programs.rs` | Программы программатора |
| `provider.rs` | `pick_box_coord` — поиск пустой клетки для бокса |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Не вносить миграции без согласованной схемы.
- Проверять корректное закрытие и обработку ошибок.
- `save_player` вызывается из flush-цикла (`lifecycle.rs`) каждые 10s.

### Testing Requirements

- Build + прогон с временной БД.

### Common Patterns

- `rusqlite` с явным управлением подключениями.
- WAL mode для concurrent read/write.

## Dependencies

### Internal

- `server/config.rs`

### External

- `rusqlite`, `serde`, `serde_json`

<!-- MANUAL: -->
