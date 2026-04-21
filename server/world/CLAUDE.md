<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/world

## Purpose

Мир на mmap-слоях (`.mapb`): cells, road, durability. Чанки 32×32. Dirty-tracking + atomic backup.

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | `World` struct, mmap-слои, `valid_coord`, `get_cell/set_cell`, `damage_cell`, `destroy`, `get_durability/set_durability`, `is_empty`, chunk API |
| `cells.rs` | Загрузка `cells.json`, `CellDefs`, `CellProp`, `cell_type` constants, `is_crystal`, `is_boulder`, `crystal_type`, `crystal_multiplier` |
| `generator.rs` | Генератор мира (секторы) |
| `sector_palette.rs` | Палитра секторов для генератора |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Менять формат мира только с `config.json`.
- `cell_type` constants — добавлять при необходимости, сверять с `cells.json`.
- `damage_cell` возвращает `bool` (destroyed).

### Testing Requirements

- Проверить размеры `world` и `chunks`.
- `cargo build` после изменения cell constants.

### Common Patterns

- mmap zero-copy доступ, изменения через dirty chunk marking.
- `WorldProvider` trait для тестируемости.

## Dependencies

### Internal

- `server/config.rs`

### External

- `memmap2`, `serde`, `serde_json`

<!-- MANUAL: -->
