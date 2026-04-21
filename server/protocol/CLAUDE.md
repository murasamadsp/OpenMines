<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/protocol

## Purpose

Сетевые протоколы клиента и сервера.

## Key Files

| File | Description |
| - | - |
| `mod.rs` | Экспорт типов, декодер входящих пакетов (U/B/TY), `TyPacket` |
| `packets.rs` | Все билдеры исходящих пакетов: `hb_bot`, `hb_bot_del`, `hb_fx`, `hb_directed_fx`, `hb_cell`, `hb_chat`, `hb_bundle`, `tp`, `health`, `basket`, `speed`, `level`, `money`, `bot_info`, `geo`, `auto_digg`, `clan_show/hide`, `ok_message`, `skills_packet`, `programmator_status`, `config_packet`, `settings_default_wire`, `inventory_*`, `chat_messages` и др. |

## Subdirectories

| Directory | Purpose |
| - | - |
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Wire-формат неизменяем — клиент legacy.
- `hb_directed_fx(bot_id, x, y, fx_type, dir, color)` — порядок аргументов критичен, сверять с C# `SendDFToBots(fx, x, y, botId, dir, color)`.
- Payload строк: разделители `:`, `#`, `,` — порядок полей 1:1 с C#.

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
