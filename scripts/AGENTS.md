<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# scripts

## Purpose

Скрипты окружения и quality-проверок.

## Key Files

| File | Description |
|------|-------------|
| `bootstrap-quality.sh` | Установка quality-инструментов |
| `ci-quality.sh` | Полный локальный прогон |
| `pre-commit.sh` | Быстрый pre-commit прогон |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
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

- `AGENTS.md` root

### External

- `cargo`, `pre-commit`, GitHub Actions tooling

