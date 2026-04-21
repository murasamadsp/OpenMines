<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/ui

## Purpose

Клиентские UI-события: кнопки GUI, инвентарь, предметы (boom/protector/raz/C190/geopack), лечение, строительство зданий из предметов.

## Key Files

| File | Description |
|------|-------------|
| `gui_buttons.rs` | Разбор и обработка кнопок интерфейса, pack GUI (Gun/Resp/Market/Teleport) |
| `heal_inventory.rs` | Лечение (Xhea), инвентарь (INVN/INUS/INCL), все предметы: boom, protector(AoE), razryadka, C190, geopack, poli, building placement |
| `up_building.rs` | Up building GUI (скиллы) |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Предметы 1:1 с `server_reference/GameShit/Consumables/ShitClass.cs` и `Inventory.cs`.
- `heal_inventory.rs` содержит AoE-хелперы (`aoe_damage_players`) — при изменении урона сверяться с `Player.Hurt`.
- `aoe_damage_players` и все предметы должны давать Health skill exp + hurt FX (fx=6).
- Boom/Protector/Razryadka — центр на facing cell, не на игроке.
- C190 — 10 клеток без остановки, stacking damage, FX type=7.
- Geopack — pickup любого alive cell (включая HypnoRock/BlackRock/RedRock).

### Testing Requirements

- Прогонять open/close UI-инвентаря.
- Проверять порядок пакетов `@L/@B` и `GU/Gu`.
- Предметы: `AccessGun` check, `can_place_over` / `ContainsPack` pre-checks.

### Common Patterns

- Сначала auth/context, потом бизнес-логика.
- Валидацию кнопок делать через префиксные разборщики.
- `is_exempt_item()` — предметы 40, 10-16, 34, 42, 43, 46 освобождены от pre-use checks.

## Dependencies

### Internal

- `server/net/session`
- `server/net/session/social`
- `server/net/session/play`

### External

- `serde_json`
- `tracing`
- `rand`

<!-- MANUAL: -->
