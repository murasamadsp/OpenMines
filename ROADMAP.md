# OpenMines roadmap

> [!WARNING]
> Этот файл больше не является источником правды по готовности фич.
> Старый roadmap удален из файла, потому что он завышал статус реализации и
> регулярно противоречил текущему коду.

## Где смотреть актуальный статус

- `SERVER_MIGRATION_STATUS.md` — единственный текущий checkpoint, проверенные
  результаты и следующий server migration slice.
- `AGENTS.md` — ограничения, источники правды и правила работы.
- `docs/ARCHITECTURE.md` — фактическая runtime topology текущего кода.
- `SIMULATION_KERNEL_PLAN.md` — ownership, latency, active work и spatial multicore.
- `SERVER_CONSISTENCY_PLAN.md` — единая форма feature-модулей и capability gates.
- `TODO.md` — product/parity/tooling backlog, не порядок simulation migration.
- `AUDIT_STATE.md` — исторический аудит 2026-07-07, не текущий handoff.
- `docs/backlog/` — непортированные или недоделанные подсистемы.
- `docs/reference/PARITY_AUDIT.md` — подробный parity-аудит. Использовать только
  как evidence-карту, а не как product-roadmap.
- `docs/reference/CLIENT_PROTOCOL_GAPS.md` — расхождения клиента и C# reference.
- `docs/DEVIATIONS.md` — намеренные отклонения от C# reference.

## Правило обновления

Новые пункты не добавлять сюда. Текущий server status обновлять в
`SERVER_MIGRATION_STATUS.md`; отдельный product backlog - в `TODO.md` или
`docs/backlog/` с проверяемыми критериями готовности.
