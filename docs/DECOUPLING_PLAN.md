# Plan: Decouple Game Logic from Wire/Network

## Problem

`openmines-server` — 61K строк монолита с циклическими зависимостями:
- `game/` → `net/`: 49 импортов
- `net/` → `game/`: 29 импортов
- `game/logic/` содержит 772 ссылки на wire-код (`send_u_packet`, `Outbox`, `PacketBatch`, `basket`, `money`)

Game logic напрямую шлёт wire-пакеты через `&Outbox`, что делает невозможным выделение крейтов.

## Goal

Game logic возвращает данные (`CommandEffects` с `SessionBatch`), а не шлёт пакеты. Network layer доставляет пакеты.

## Current State

### Already migrated (returns CommandEffects):
- `commands/mod.rs::apply_market_sell/sell_all/buy/get_profit` → `PacketBatch` + `SessionBatch`
- `commands/slash.rs` → `SessionBatch`
- `commands/completion.rs` → `SessionBatch`
- `game/logic/player_init.rs` → `SessionBatch`
- `game/logic/movement.rs` → `SessionBatch` (fanout)

### NOT migrated (sends wire directly via &Outbox):

#### High-priority (mutation paths):
1. `game/logic/gui/gui_buttons.rs` — `handle_gui_button`, `handle_gui_button_sync_fast_path`
2. `game/logic/gui/market_gui.rs` — `handle_market_tab_switch`, `open_market_gui`
3. `game/logic/gui/pack_gui.rs` — `handle_pack_operation`, `handle_pack_save`
4. `game/logic/gui/crafter_gui.rs` — `handle_craft_*`
5. `game/logic/gui/programmator_gui.rs` — `handle_*`
6. `game/logic/up_building.rs` — skill/upgrade mutations
7. `game/logic/heal_inventory.rs` — heal/use items
8. `game/logic/dig_build.rs` — dig/build actions
9. `game/logic/clans.rs` — clan operations
10. `game/logic/packs.rs` — pack operations (resp, gun)
11. `game/logic/consumables.rs` — boom/protector/razryadka
12. `game/logic/buildings.rs` — building placement/deletion
13. `game/logic/misc.rs` — misc handlers

#### Medium-priority (read-only views):
14. `game/logic/horb.rs` — `HorbDelivery` trait (partially done: `&dyn PacketSink`)
15. `game/logic/skills.rs` — skill view
16. `game/logic/settings.rs` — settings view

#### Low-priority (session infrastructure):
17. `net/session/auth/login.rs` — login flow
18. `net/session/auth/gui_flow.rs` — registration flow
19. `net/session/connection.rs` — session lifecycle
20. `net/session/handshake.rs` — handshake

## Progress

### Migrated to typed command pipeline:
- [x] market sell/buy/sellall/getprofit
- [x] pack operations (resp_bind, resp_fill, gun_fill, resp_profit, resp_save)
- [x] teleport
- [x] up_building (skill, upgrade, delete, install, buyslot)

### Still on &Outbox:
- [ ] settings save (net/session/ui/settings.rs)
- [ ] heal_inventory (complex, async, uses due_actions)
- [ ] consumables (boom, protector, razryadka)
- [ ] dig_build
- [ ] clans (GUI rendering)
- [ ] programmator

### Metrics:
- Functions on `&dyn PacketSink`: 34
- Functions still on `&Outbox`: 151

## Execution Order

### Phase 1: GUI handlers (sell/buy done, continue pattern)
For each GUI handler:
1. Intercept button in `apply_gui_button_command` 
2. Call economy/mechanic function (pure ECS mutation)
3. Build wire packets in `PacketBatch`
4. Return `CommandEffects` with `SessionBatch`
5. Remove from `handle_gui_button_sync_fast_path`

Priority order:
- [x] market sell/buy/sellall/getprofit
- [ ] market tab switch (sellcrys/buycrys — read-only, lower priority)
- [ ] pack operations (resp_bind, resp_fill, gun_fill, resp_profit, resp_save, pack_save)
- [ ] teleport
- [ ] settings save
- [ ] up_building (skill/upgrade/delete/install/buyslot)
- [ ] heal_inventory
- [ ] consumables

### Phase 2: Complex handlers (need deeper refactor)
- [ ] dig_build — needs `CommandEffects` with broadcast effects
- [ ] clans — already partially done (ClanCommand), but GUI rendering still coupled
- [ ] programmator — complex state machine, lowest priority

### Phase 3: Cleanup
- [ ] Remove dead code (`#[allow(dead_code)]` functions)
- [ ] Update tests to use new paths
- [ ] Verify no regressions with full test suite

## Verification

After each slice:
```bash
cargo check --all-targets
cargo nextest run --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
cargo fmt --all -- --check
scripts/dev/smoke.sh
```

## Metrics

Track progress:
- `game/` → `net/` imports: currently 49, target 0
- Wire references in `game/logic/`: currently 772, target 0
- `&Outbox` parameters in game logic: count and eliminate
