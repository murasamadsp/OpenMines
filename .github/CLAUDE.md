<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# .github

## Purpose

CI и workflow-манифесты проекта.

## Key Files

| File | Description |
|------|-------------|
| `.github/workflows/markdown-lint.yml` | CI для markdown |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `workflows/` | CI workflows definitions |

## For AI Agents

### Working In This Directory

- Менять workflow только при необходимости и после проверки CI.
- Оставлять стабильный минимум шагов для GitHub-hosted раннеров.
- Проверять чувствительные ключи (`push`, `pull_request`).

### Testing Requirements

- Проверять workflow через PR/локальный `act`.

### Common Patterns

- Использовать pinned-версии actions.
- Логический, воспроизводимый порядок шагов.

## Dependencies

### Internal

- `CLAUDE.md` root
- `.github/workflows/CLAUDE.md` for workflow-level rules

### External

- GitHub Actions ecosystem

