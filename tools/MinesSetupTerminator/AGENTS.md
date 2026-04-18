<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# scripts/MinesSetupTerminator

## Purpose

Утилита завершения игрового процесса и упаковки runtime.

## Key Files

| File | Description |
|------|-------------|
| `MinesSetupTerminator.csproj` | Конфиг `.NET 8` |
| `Program.cs` | Логика закрытия приложения |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `bin/` | Сборочные бинарники .NET (сгенерированные) |
| `obj/` | Промежуточные артефакты сборки (сгенерированные) |
| `publish-win/` | Публикационные бинарники для Windows |

## For AI Agents

### Working In This Directory

- Не менять артефакты `bin/`, `obj/`, `publish-win/` без причины.
- Проверять `Program.cs` с целевой runtime-версией.

### Testing Requirements

- Проверять `dotnet build .../MinesSetupTerminator.csproj` после логики.
- Прогонять close-flow в целевой ОС.

### Common Patterns

- Держать `Program.cs` компактным.
- Источник истины — `Program.cs`, а не артефакты.

## Dependencies

### Internal

- `../AGENTS.md`

### External

- `dotnet`
- `Windows SDK` (где применимо)

<!-- MANUAL: -->
