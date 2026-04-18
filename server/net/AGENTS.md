<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

## Логи и отладка

Глобальная инициализация — `server/logging.rs`, настройки в `config.json` → `logging` (`filter`, `format`: `pretty` / `compact` / `json`, опционально `file` с путём и отдельным `format` для файла). В `filter` по умолчанию целевой крейт — `openmines_server`. Переменные окружения **`RUST_LOG`** или **`M3R_LOG`** перекрывают `filter` из файла. У TCP-сессии в `connection` есть span `session` с полями `peer.addr` и `session.id` на время `read_loop`.

# server/net

## Purpose

Сеть: listener, сессии и интеграция с состоянием игры.

## Key Files

| File | Description |
| - | - |
| `mod.rs` | TCP accept loop; фоновые задачи подключаются из `lifecycle` |
| `lifecycle.rs` | Периодический flush мира, сохранение игроков, shutdown (SRP) |
| `session/mod.rs` | Корень сессии: `outbound`, `dispatch`, `auth`, `play`, `player`, `social`, `ui`, `connection` |
| `session/connection.rs` | Handshake, `read_loop` / `write_loop` |
| `session/prelude.rs` | Общие `use` для подмодулей |
| `session/constants.rs` | Heartbeat, лимиты HB-бандлов |
| `session/util.rs` | Утилиты координат для пакетов |
| `session/wire.rs` | Кодирование U/B/HB |

## Слойность (зависимости)

Направление сверху вниз; нижний слой не импортирует верхний.

1. **`prelude` / `constants` / `util` / `wire`** — только протокол/типы.
2. **`outbound`** — готовые U/B пакеты, без `social/ui`.
3. **`play`** — чанки, копание, паки, `spawn`.
4. **`player`** — `init_player` и `on_disconnect`.
5. **`auth`** — шаг до входа, с `player::init`.
6. **`dispatch`** — `TY` → `play/social/ui`.
7. **`ui` / `social`** — GUI/события через `outbound`, `play`, `player`.

Цикл **`player` ↔ `social`** снят: чат/инвентарь-синк в `outbound`, спавн ящика — в `play::spawn`.

## Subdirectories

| Directory | Purpose |
| - | - |
| `session/outbound/` | Тонкая отправка: `chat_sync`, `inventory_sync` |
| `session/dispatch/` | `ty` — разбор `TY` и вызов обработчиков |
| `session/auth/` | `login` (AU, `AuthState`), `gui_flow` (регистрация до входа) |
| `session/play/` | `movement`, `dig_build`, `chunks`, `packs`, `spawn` |
| `session/player/` | `init` — вход в мир, disconnect |
| `session/ui/` | `gui_buttons`, `heal_inventory` |
| `session/social/` | `misc` (чат, `/`, смерть), `clans`, `buildings` |

## For AI Agents

### Working In This Directory

- Протокол сессии меняется только вместе с `protocol`.
- Избегать блокирующих операций в async.

### Testing Requirements

- Проверить базовый путь подключения клиента и корректное завершение сессии.

### Common Patterns

- Активировать сетевой слой через async-рантайм (`tokio`).

## Dependencies

### Internal

- `server/AGENTS.md`
- `server/protocol`, `server/game`

### External

- `tokio`

<!-- MANUAL: -->
