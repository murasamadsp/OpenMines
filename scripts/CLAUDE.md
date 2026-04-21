<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# scripts

## Purpose

Скрипты окружения и quality-проверок.

## Key Files

| File | Description |
| - | - |
| `bootstrap-quality.sh` | Установка quality-инструментов (cargo-* + pre-commit) |
| `build-client.sh` | Headless-сборка Unity-клиента (win/mac) |
| `ci-quality.sh` | Полный локальный прогон качества (fmt/clippy/deny/audit/tests/docs) |
| `deploy-vps.sh` | Деплой на VPS: rsync → `docker run`+cargo (обход runc/BuildKit на Virtuozzo) → образ `ops/Dockerfile.vps` → compose up |
| `full-reinstall-vps.sh` | Переустановка на VPS: compose down (+опциональный wipe volume) → deploy |
| `pre-commit.sh` | Быстрый pre-commit прогон (fmt/clippy/deny/audit/tests + optional extended) |
| `quality-common.sh` | Общие шаги quality-loop (shared между ci-quality/pre-commit) |
| `vps-common.sh` | Общие функции для VPS-скриптов (ssh/rsync/compose, `vps_build_openmines_binary`) |
| `vps-regen-world.sh` | Regen мира на VPS (удаляет только `.mapb` + здания в SQLite) |
| `wipe-players.sh` | Опасно: чистит игроков/здания/кланы/сообщения в SQLite |

## Subdirectories

| Directory | Purpose |
| - | - |
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Держать `set -euo pipefail` и явные проверки зависимостей.
- Не ломать идемпотентность и совместимость macOS/Linux.
- Изменения окружения только согласовывать.

### Testing Requirements

- Проверять скрипты после изменений на чистом окружении.

### Common Patterns

- Последовательные диагностические сообщения в стиле `==>`.
- Защита от отсутствующих инструментов через guard-проверки и понятные сообщения.

## Dependencies

### Internal

- `CLAUDE.md` root

### External

- `cargo`, `pre-commit`, GitHub Actions tooling
