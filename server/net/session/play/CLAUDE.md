<!-- Parent: ../CLAUDE.md -->
<!-- Generated: 2026-04-16 | Updated: 2026-04-16 -->

# server/net/session/play

## Purpose

Игровой слой сессии: движение, копание, чанки, спавн и паки.

## Key Files

| File | Description |
|------|-------------|
| `death.rs` | Смерть/респавн (`handle_death`, `hurt_player_pure`, `flush_player_death_queue_after_tick`), двухфазная модель (ECS → broadcast) |
| `dig_build.rs` | Логика Xdig/Xbld: кристаллы на каждый удар, dig exp на destroy, build chain G→Y→R, durability |
| `geo.rs` | Геология (Xgeo): pickup/place блоков, cooldown 200ms |
| `movement.rs` | Валидация и обработка перемещения робота, Movement skill exp |
| `chunks.rs` | Управление видимостью чанков и отправкой HB-пакетов |
| `packs.rs` | Показ GUI пака и управление доступом |
| `spawn.rs` | Спавн временных сущностей в мире |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `-` | Нет вложенных рабочих директорий |

## For AI Agents

### Working In This Directory

- Не менять физику/координаты без проверки `movement/chunks`.
- `dig_build.rs`: кристаллы + MineGeneral exp на каждый удар (не только при destroy). Dig exp ("d") только при destroy.
- MineGeneral exp = pre-multiplier value (до `dob *= crystal_multiplier`).
- Build Yellow/Red: durability СКЛАДЫВАЕТСЯ с существующей (`get_durability + effect`).
- Build cost для Green/Yellow/Red = всегда 1 (C# `effectfunc = (x) => 1`).
- Movement: skill exp на каждый успешный ход.
- Перед `Xdig/Xbld` прогонять сценарии.

### Testing Requirements

- Проверять смену чанков и broadcast после move/dig.
- Прогонять quick-игру на базовом клиенте.

### Common Patterns

- Менять мир только через `state.world`.
- Для сетевых эффектов: `send_u_packet` + `hb_bundle`.

## Dependencies

### Internal

- `server/net/session`
- `server/game`
- `server/world`

### External

- `tracing`

<!-- MANUAL: -->

## Movement: специфика применения методологии

Общие правила server-authoritative см. в `server/CLAUDE.md` → «Методология сервера».
Здесь — только уточнения для movement/dig/chunks.

### Порядок валидации в `handle_move`

1. Игрок активен, не в окне.
2. `world.valid_coord(x, y)`.
3. `world.is_empty(x, y)` (иначе ветка `auto_dig` или `@T`).
4. `|Δx| + |Δy| == 1` относительно **серверной** `old_x, old_y`.
5. Rate-limit: `now - last_move_ts >= MIN_MOVE_INTERVAL_MS` (ориентир: 60 мс road / 90 мс обычная). Нарушение → тихий drop, без `@T`.

Нарушение 1–4 → `@T(old_x, old_y)` + `warn!`.

### Broadcast

- `send_player_move_update` обязан вызывать `broadcast_to_nearby(..., Some(pid))`.
- `check_chunk_changed` включает self в HB нового чанка — это ок, клиент при `tail=0` пишет `myBotLastSync`, позицию не снимает.

### Телепорт (`@T`)

Только: rollback rejected move, админ `/tp`, респ, вход в пак. **Никогда** для периодической синхронизации.

