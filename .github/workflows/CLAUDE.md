




<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# .github/workflows

## Purpose

Описания CI workflow и их параметров.

## Key Files

| File                | Description            |
| ------------------- | ---------------------- |
| `markdown-lint.yml` | Markdown lint в CI |


## Subdirectories


| Directory | Purpose                          |
| --------- | -------------------------------- |
| `-`       | Нет вложенных рабочих директорий |


## For AI Agents

### Working In This Directory

- Менять workflow только с пониманием рисков.
- Не менять триггеры без причины.
- Тяжелые действия только при необходимости.

### Testing Requirements

- Проверять запуск на `pull_request`/`push`.

### Common Patterns

- Короткий, явный pipeline: `checkout`, env, lint/scan.
- Понятные `name:` для каждого шага.

## Dependencies

### Internal

- `CLAUDE.md`
- `.github/CLAUDE.md`

### External

- GitHub Actions marketplace actions