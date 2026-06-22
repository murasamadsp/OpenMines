# Remove `Arc<GameState>` from ECS systems

## Why

ECS systems access `Res<GameStateResource>` just to get `state.world` (cells/durability)
and `state.box_take()` (box pickup). Arc adds unnecessary coupling — systems touch
2 fields of a 20-field struct.

## Changes

### 1. New ECS resources (server/game/mod.rs)

Replace `GameStateResource` with:
- `WorldResource(pub Arc<World>)` — world map access
- `BoxIndexResource(pub DashMap<(i32, i32), [i64; 6]>)` — box crystals
- `BoxPersistQueue(pub Mutex<Vec<BoxPersist>>)` — deferred box DB persist

### 2. Insert in GameState::new()

Replace:
```rust
ecs.insert_resource(GameStateResource(state.clone()));
```
With:
```rust
ecs.insert_resource(WorldResource(state.world.clone()));
ecs.insert_resource(BoxIndexResource(DashMap::new()));
ecs.insert_resource(BoxPersistQueue::default()));
```

### 3. Delete `GameStateResource` struct

### 4. Update 6 ECS systems

| System | File | Change |
|---|---|---|
| acid_physics_system | acid.rs | `GameStateResource` → `WorldResource` |
| sand_physics_system | sand.rs | same |
| standing_cell_hazard_system | combat.rs | `WorldResource` + `BoxIndexResource` + `BoxPersistQueue` |
| gun_firing_system | combat.rs | remove unused `_state_res` param |
| alive_physics_system | alive.rs | `WorldResource` |
| programmator_system | programmator.rs | `WorldResource` |

### 5. lifecycle.rs — drain BoxPersistQueue inside ecs.write()

### 6. Files (7 total)

1. server/game/mod.rs — new resources + insert
2. server/game/acid.rs
3. server/game/sand.rs
4. server/game/combat.rs
5. server/game/alive.rs
6. server/game/programmator.rs
7. server/net/lifecycle.rs

## Verify

- cargo check + cargo test
- cargo test --release bench_tick -- --ignored
