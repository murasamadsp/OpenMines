<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# OpenMines

## Purpose

OpenMines: Rust-сервер, legacy Unity-клиент и вспомогательные скрипты в одном репозитории.

## Key Files

| File | Description |
| - | - |
| `Cargo.toml` | Конфиг Rust workspace |
| `Cargo.lock` | Зафиксированные зависимости |
| `server/main.rs` | Точка входа сервера |
| `server/config.rs` | Парсинг конфигурации |
| `config.json` | Пример runtime-конфига |
| `cells.json` | Описание клеток мира |
| `scripts/ci-quality.sh` | Локальная проверка качества |
| `scripts/pre-commit.sh` | Локальный pre-commit пайплайн |
| `.github/workflows/markdown-lint.yml` | CI markdown-lint |
| `.pre-commit-config.yaml` | Конфигурация хуков |

## Subdirectories

| Directory | Purpose |
| - | - |
| `server/` | Rust-сервер и игровые протоколы |
| `client/` | Unity C# клиент и ассеты |
| `.claude/` | Local agent state и настройки |
| `.github/` | CI/CD |
| `scripts/` | Скрипты качества и сборки |

## For AI Agents

### Working In This Directory

- Не трогать `.omc`, `bin/obj/target` без причины.
- Менять Rust/C# только в своих деревьях.

### Testing Requirements

- Для `server/`: `cargo build` и run-проверка через `config.json`.
- Не использовать `--no-verify` НИ ПРИ КАКИХ ОБСТОЯТЕЛЬСТВАХ!!!

### Common Patterns

- Rust API держи в `server/<module>/mod.rs`.
- C# менять только в текущих неймспейсах.

## Dependencies

### Internal

- `server/` ↔ `config.json`, `cells.json`
- `server/` ↔ `scripts/` (quality-скрипты)

### External

- `tokio`, `rusqlite`, `serde` и т.д.

<!-- MANUAL: -->

ЕСЛИ БЛЯТЬ ЧТО-ТО УДАЛЕНО, НЕ НАДО ЕГО ВОЗВРАЩАТЬ!!!!!!!

Я УЖЕ ЗАЕБАЛСЯ ТО Я УДАЛЮ, ТО ТЫ ВЕРНЕШЬ, ТО Я УДАЛЮ И ТАК БЕСКОНЕЧНО

И ЗАПРЕЩЕНО МЕНЯТЬ ЛИНТЕРЫ И Т.П.

старайся работать только агентами
