<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/game

## Purpose

Игровое состояние, ECS-компоненты и системы (Bevy ECS).

## Key Files

| File | Description |
|------|-------------|
| `mod.rs` | `GameState` (центральный Arc-объект), ECS-системы, очереди broadcast/programmator |
| `player.rs` | ECS-компоненты игрока (Position, Stats, Inventory, Skills, Cooldowns, Connection, UI, Flags и др.) |
| `buildings.rs` | ECS-компоненты зданий, `PackType` enum (15 вариантов), `PackView` |
| `combat.rs` | `standing_cell_hazard_system` (урон от клеток), `gun_firing_system` (пушки, AntiGun) |
| `sand.rs` | `sand_physics_system` (гравитация песка и валунов, Gate pass-through) |
| `skills.rs` | 58 типов навыков, формулы эффектов/exp, `add_skill_exp`, `skill_effect` |
| `programmator.rs` | `ProgrammatorState`, парсер программ, step execution |
| `chat.rs` | `ChatChannel`, `ChatMessage` структуры |
| `crafting.rs` | 8 рецептов крафтинга |
| `direction.rs` | `dir_offset()` — смещение по сетке для направления 0–3 |
| `acid.rs` | Stub для кислотной физики |
| `alive.rs` | Stub для alive-клеток (7 типов поведения) |
| `botspot.rs` | BotSpot сущность |
| `market.rs` | Stub для маркета |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Менять `GameState` атомарно.
- ECS-системы не модифицируют мир напрямую — используют `BroadcastQueue` и `ProgrammatorQueue`.
- Формулы скиллов — 1:1 с `server_reference/GameShit/Skills/PlayerSkills.cs`.
- `combat.rs` gun: damage → exp → charge (в этом порядке).

### Testing Requirements

- Проверить инициализацию `GameState` из `main.rs`.
- `cargo build` + clippy после изменения формул или компонентов.

### Common Patterns

- Bevy ECS: компоненты через `#[derive(Component)]`, системы через `Query<>`.
- Dirty flag tracking — периодические flush-циклы сохраняют помеченных игроков/здания.
- `DeathQueue` — отложенные смерти после `schedule.run`.

## Dependencies

### Internal

- `server/world`, `server/db`, `server/net`

### External

- `bevy_ecs`, `tokio`, `tracing`, `rand`

<!-- MANUAL: -->
