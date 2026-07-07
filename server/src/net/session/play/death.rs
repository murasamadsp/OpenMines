//! Смерть, респавн, урон (Player.Death / Player.Hurt).
use crate::db::pick_box_coord;
use crate::game::broadcast_cell_update;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;

/// Broadcast-данные, собранные внутри `ecs.write()`, выполняются снаружи.
pub struct DeathBroadcasts {
    pub box_cell: Option<(i32, i32)>,
    pub fx_death: Option<(i32, i32)>,
    pub death_pos: (i32, i32),
    pub money: i64,
    pub creds: i64,
    pub resp_used: bool,
    /// Корзина была непустой и очищена при смерти. C# шлёт `@B` (`SendCrys`)
    /// только при `AllCry > 0` (Basket.ClearCrys → Changed) — иначе @B не шлётся.
    pub basket_cleared: bool,
    /// Программа была запущена и остановлена смертью (не `RespawnOnProg`-продолжение).
    /// C# шлёт `ProgrammatorPacket(false)` (@P) только в этом случае (Player.cs:935).
    pub prog_stopped: bool,
    pub cleared_spawn_cell: Option<(i32, i32)>, // временная система
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathCoreError {
    NoPlayerEntity,
    PlayerState(&'static str),
    RespState(&'static str),
}

type DeathCoreOutput = (i32, i32, i32, DeathBroadcasts);

fn send_death_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("СМЕРТЬ", "Состояние игрока недоступно.").1,
    );
}

/// Мутации ECS как в `Player.Death()` (`Player.cs`).
/// **НЕ** вызывает ничего, что лочит `state.ecs` (`broadcast/get_pack_at`) —
/// вместо этого возвращает `DeathBroadcasts` для вызывающего.
pub fn apply_player_death_core(
    state: &Arc<GameState>,
    ecs: &mut bevy_ecs::prelude::World,
    pid: PlayerId,
) -> std::result::Result<DeathCoreOutput, DeathCoreError> {
    let entity = state
        .get_player_entity(pid)
        .ok_or(DeathCoreError::NoPlayerEntity)?;
    let (pos_x, pos_y, cry, rebind_x, rebind_y, mh) = {
        let s = ecs
            .get::<crate::game::player::PlayerStats>(entity)
            .ok_or(DeathCoreError::PlayerState("PlayerStats"))?;
        let p = ecs
            .get::<crate::game::player::PlayerPosition>(entity)
            .ok_or(DeathCoreError::PlayerState("PlayerPosition"))?;
        let m = ecs
            .get::<crate::game::player::PlayerMetadata>(entity)
            .ok_or(DeathCoreError::PlayerState("PlayerMetadata"))?;
        (p.x, p.y, s.crystals, m.resp_x, m.resp_y, s.max_health)
    };
    let _ = ecs
        .get::<crate::game::player::PlayerUI>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerUI"))?;
    let _ = ecs
        .get::<crate::game::player::PlayerView>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerView"))?;
    let _ = ecs
        .get::<crate::game::player::PlayerFlags>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerFlags"))?;
    let _ = ecs
        .get::<crate::game::programmator::ProgrammatorState>(entity)
        .ok_or(DeathCoreError::PlayerState("ProgrammatorState"))?;

    let mut bcast = DeathBroadcasts {
        box_cell: None,
        // 1:1 C# `Player.Death` (Player.cs:912) — `SendFXoBots(2,x,y)`
        // БЕЗУСЛОВНО (даже с пустой корзиной соседи видят анимацию смерти).
        fx_death: Some((pos_x, pos_y)),
        death_pos: (pos_x, pos_y),
        money: 0,
        creds: 0,
        resp_used: false,
        basket_cleared: false,
        prog_stopped: false,
        cleared_spawn_cell: None, // временная система
    };

    if cry.iter().sum::<i64>() > 0 {
        let c = cry;
        let box_placed = match pick_box_coord(
            pos_x,
            pos_y,
            |x, y| state.world.valid_coord(x, y),
            |x, y| {
                if !state.world.is_empty(x, y) {
                    return false;
                }
                let cell = state.world.get_cell_typed(x, y);
                state.world.cell_defs().get_typed(cell).can_place_over()
            },
        ) {
            Some((bx, by))
                if GameState::find_pack_covering_with(ecs, &state.chunk_buildings, bx, by)
                    .is_none() =>
            {
                // C-2 фикс: in-memory put + отложенная персистенция вместо
                // sync `db.upsert_box` под удерживаемым `ecs.write()` (death
                // flush в tick-цикле) — фризило при каждой смерти.
                state.put_box_cell(bx, by, c);
                let mut s = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .ok_or(DeathCoreError::PlayerState("PlayerStats"))?;
                s.crystals = [0; 6];
                Some((bx, by))
            }
            _ => {
                // Даже без бокса — обнулить кристаллы
                let mut s = ecs
                    .get_mut::<crate::game::player::PlayerStats>(entity)
                    .ok_or(DeathCoreError::PlayerState("PlayerStats"))?;
                s.crystals = [0; 6];
                None
            }
        };
        bcast.box_cell = box_placed;
        // C# Basket: @B шлётся при AllCry>0 (корзина очищена) — независимо от
        // того, удалось ли поставить бокс.
        bcast.basket_cleared = true;
        // fx_death уже выставлен безусловно при создании bcast (см. выше).
    }

    let mut is_free_resp = false;
    // Респаун: проверяем pack через уже имеющийся &mut ecs (без отдельного лока)
    let (rx, ry) = if let (Some(x), Some(y)) = (rebind_x, rebind_y) {
        // Collect resp building data immutably first, then mutate.
        let resp_data = state.building_entity_at(x, y).map(|bld_ent| {
            let Some(meta) = ecs.get::<crate::game::buildings::BuildingMetadata>(bld_ent) else {
                return Err(DeathCoreError::RespState("BuildingMetadata"));
            };
            if meta.pack_type != crate::game::buildings::PackType::Resp {
                return Ok(None);
            }
            let b_stats = ecs
                .get::<crate::game::buildings::BuildingStats>(bld_ent)
                .ok_or(DeathCoreError::RespState("BuildingStats"))?;
            let owner = ecs
                .get::<crate::game::buildings::BuildingOwnership>(bld_ent)
                .ok_or(DeathCoreError::RespState("BuildingOwnership"))?;
            Ok(Some((
                bld_ent,
                i64::from(b_stats.cost),
                owner.owner_id,
                b_stats.charge,
            )))
        });
        let resp_data = resp_data.transpose()?.flatten();
        if let Some((bld_ent, resp_cost, owner_id, charge)) = resp_data {
            // C# `Resp.OnRespawn`: cost/charge списываются ТОЛЬКО при ownerid>0.
            // `is_free_resp` — по cost-полю ИСХОДНОГО привязанного респа (C#
            // `RespawnOnProg => resp.cost==0`), независимо от того, где в итоге спавн.
            if resp_cost == 0 {
                is_free_resp = true;
            }
            let money = ecs
                .get::<crate::game::player::PlayerStats>(entity)
                .ok_or(DeathCoreError::PlayerState("PlayerStats"))?
                .money;
            // Публичный респ (owner==0) — бесплатно. Owned: нужно charge>0 И
            // money>cost (строгое, как C#). Иначе → ребинд на случайный публичный
            // (бесплатный) респ — здравый смысл вместо патологической C#-рекурсии
            // `p.resp = null; p.resp.OnRespawn(p)`.
            let respawn_here = owner_id == 0 || (charge > 0 && money > resp_cost);
            if respawn_here {
                if owner_id > PlayerId(0) {
                    // Платный owned-респ: списать cost, заряд--, добавить в копилку.
                    let _ = ecs
                        .get::<crate::game::buildings::BuildingStorage>(bld_ent)
                        .ok_or(DeathCoreError::RespState("BuildingStorage"))?;
                    let _ = ecs
                        .get::<crate::game::buildings::BuildingFlags>(bld_ent)
                        .ok_or(DeathCoreError::RespState("BuildingFlags"))?;
                    let mut s = ecs
                        .get_mut::<crate::game::player::PlayerStats>(entity)
                        .ok_or(DeathCoreError::PlayerState("PlayerStats"))?;
                    s.money -= resp_cost;
                    let mut bld_stats = ecs
                        .get_mut::<crate::game::buildings::BuildingStats>(bld_ent)
                        .ok_or(DeathCoreError::RespState("BuildingStats"))?;
                    bld_stats.charge -= 1;
                    let mut bld_storage = ecs
                        .get_mut::<crate::game::buildings::BuildingStorage>(bld_ent)
                        .ok_or(DeathCoreError::RespState("BuildingStorage"))?;
                    bld_storage.money += resp_cost;
                    let mut bld_flags = ecs
                        .get_mut::<crate::game::buildings::BuildingFlags>(bld_ent)
                        .ok_or(DeathCoreError::RespState("BuildingFlags"))?;
                    bld_flags.dirty = true;
                    // C# ref: Resp.OnRespawn calls p.SendMoney() — capture for later
                    let s = ecs
                        .get::<crate::game::player::PlayerStats>(entity)
                        .ok_or(DeathCoreError::PlayerState("PlayerStats"))?;
                    bcast.money = s.money;
                    bcast.creds = s.creds;
                    bcast.resp_used = true;
                }
                use rand::Rng;
                let mut rng = rand::rng();
                let ox = rng.random_range(2..5i32);
                let oy = rng.random_range(-1..3i32);
                let (cx, cy) = (x + ox, y + oy);
                if state.world.valid_coord(cx, cy) && state.world.is_empty(cx, cy) {
                    (cx, cy)
                } else {
                    (x + 2, y)
                }
            } else {
                find_random_public_resp(state, ecs)
            }
        } else {
            find_random_public_resp(state, ecs)
        }
    } else {
        find_random_public_resp(state, ecs)
    };

    {
        let mut p = ecs
            .get_mut::<crate::game::player::PlayerPosition>(entity)
            .ok_or(DeathCoreError::PlayerState("PlayerPosition"))?;
        p.x = rx;
        p.y = ry;
    }

    // Clear military block at respawn position if present
    let spawn_cell = state.world.get_cell_typed(rx, ry);
    if spawn_cell.is(cell_type::MILITARY_BLOCK) || spawn_cell.is(cell_type::MILITARY_BLOCK_FRAME) {
        state.world.destroy(rx, ry);
        bcast.cleared_spawn_cell = Some((rx, ry));
    } // временная система
    let mut s = ecs
        .get_mut::<crate::game::player::PlayerStats>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerStats"))?;
    s.health = mh;
    let mut ui = ecs
        .get_mut::<crate::game::player::PlayerUI>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerUI"))?;
    ui.current_window = None;
    let mut v = ecs
        .get_mut::<crate::game::player::PlayerView>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerView"))?;
    v.last_chunk = None;
    v.visible_chunks.clear();
    let mut f = ecs
        .get_mut::<crate::game::player::PlayerFlags>(entity)
        .ok_or(DeathCoreError::PlayerState("PlayerFlags"))?;
    f.dirty = true;
    let mut prog = ecs
        .get_mut::<crate::game::programmator::ProgrammatorState>(entity)
        .ok_or(DeathCoreError::PlayerState("ProgrammatorState"))?;
    if prog.running {
        if let Some(label) = prog.goto_death.clone().filter(|_| is_free_resp) {
            if prog.current_prog.contains_key(&label) {
                // C# ProgrammatorData.OnDeath(): current.Reset() (уходящая функция),
                // затем cFunction = GotoDeath. GotoDeath НЕ ресетится (продолжается).
                let departing = prog.current_function.clone();
                if let Some(f) = prog.current_prog.get_mut(&departing) {
                    f.reset();
                }
                prog.current_function = label;
            } else {
                prog.running = false;
                bcast.prog_stopped = true;
            }
        } else {
            prog.running = false;
            bcast.prog_stopped = true;
        }
    }

    Ok((rx, ry, mh, bcast))
}

/// C# ref: Player.resp getter — when null, pick random public resp (ownerid==0).
fn find_random_public_resp(state: &Arc<GameState>, ecs: &bevy_ecs::prelude::World) -> (i32, i32) {
    use rand::Rng;
    let public_resps: Vec<(i32, i32)> = state
        .building_index
        .iter()
        .filter_map(|entry| {
            let entity = *entry.value();
            let meta = ecs.get::<crate::game::buildings::BuildingMetadata>(entity)?;
            if meta.pack_type != crate::game::buildings::PackType::Resp {
                return None;
            }
            let ownership = ecs.get::<crate::game::buildings::BuildingOwnership>(entity)?;
            if ownership.owner_id != 0 {
                return None;
            }
            let pos = ecs.get::<crate::game::buildings::GridPosition>(entity)?;
            Some((pos.x, pos.y))
        })
        .collect();
    if public_resps.is_empty() {
        return (10, 10);
    }
    let mut rng = rand::rng();
    let (rx, ry) = public_resps[rng.random_range(0..public_resps.len())];
    let ox = rng.random_range(2..5i32);
    let oy = rng.random_range(-1..3i32);
    (rx + ox, ry + oy)
}

/// Выполнить отложенные broadcast'ы после отпускания `ecs.write()`.
pub fn run_death_broadcasts(state: &Arc<GameState>, bcast: &DeathBroadcasts, pid: PlayerId) {
    // Сообщить всем соседям, что бот исчез
    let (pos_x, pos_y) = bcast.death_pos;
    let del = hb_bot_del(net_u16_nonneg(pid));
    state.broadcast_hb_at(pos_x, pos_y, &[del], Some(pid));

    if let Some((bx, by)) = bcast.box_cell {
        broadcast_cell_update(state, bx, by);
    }
    if let Some((pos_x, pos_y)) = bcast.fx_death {
        let fx = hb_fx(pos_x as u16, pos_y as u16, 2);
        state.broadcast_hb_at(pos_x, pos_y, &[fx], None);
    }
    if let Some((cx, cy)) = bcast.cleared_spawn_cell {
        broadcast_cell_update(state, cx, cy);
    } // временная система
}

pub fn send_respawn_after_death(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    rx: i32,
    ry: i32,
    mh: i32,
    bcast: &DeathBroadcasts,
) {
    tracing::warn!(
        player_id = %pid,
        x = rx,
        y = ry,
        max_health = mh,
        "Player respawned after death"
    );
    // C# ref Death(): win=null → SendWindow() → SendMoney → tp → SendHealth().
    send_u_packet(tx, "Gu", &gu_close().1);
    // C# Basket: @B (SendCrys) только если корзина была очищена (AllCry>0).
    if bcast.basket_cleared {
        send_u_packet(tx, "@B", &basket(&[0; 6], 1).1);
    }
    // C# Resp.OnRespawn → p.SendMoney() ДО tp (P$ перед @T).
    if bcast.resp_used {
        send_u_packet(tx, "P$", &money(bcast.money, bcast.creds).1);
    }
    send_u_packet(tx, "@T", &tp(rx, ry).1);
    send_u_packet(tx, "@L", &health(mh, mh).1);
    // C# Player.cs:935: @P(false) только при остановке программы смертью.
    // RespawnOnProg-продолжение и not-running — @P НЕ шлётся.
    if bcast.prog_stopped {
        send_u_packet(tx, "@P", &programmator_status(false).1);
    }
}

/// `RESP` / очередь после пушки: `ecs.write()` для мутаций, broadcast'ы снаружи.
pub fn handle_death(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let result = {
        let mut ecs = state.ecs.write();
        apply_player_death_core(state, &mut ecs, pid)
    };
    match result {
        Ok((rx, ry, mh, bcast)) => {
            run_death_broadcasts(state, &bcast, pid);
            send_respawn_after_death(tx, pid, rx, ry, mh, &bcast);
            broadcast_self_after_respawn(state, pid, rx, ry);
            check_chunk_changed(state, tx, pid);
        }
        Err(error) => {
            tracing::error!(player_id = %pid, ?error, "Player death aborted");
            send_death_state_error(tx);
        }
    }
}

/// C# `tp` → `SendMyMove`: после респавна разослать `hb_bot` СЕБЯ соседям новой
/// позиции. Иначе воскресший бот невидим соседям до следующего `bots_render`
/// (~4с), т.к. `run_death_broadcasts` уже удалил его на старой позиции.
fn broadcast_self_after_respawn(state: &Arc<GameState>, pid: PlayerId, rx: i32, ry: i32) {
    let attrs = state.query_player_opt(pid, |ecs, entity| {
        let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let p_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
        let tail = ecs
            .get::<crate::game::programmator::ProgrammatorState>(entity)
            .map_or(0, |ps| u8::from(ps.running));
        Some((pos.dir, p_stats.skin, p_stats.clan_id.unwrap_or(0), tail))
    });
    if let Some((dir, skin, clan, tail)) = attrs {
        let bot = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(rx),
            net_u16_nonneg(ry),
            net_u8_clamped(dir, 3),
            net_u8_clamped(skin, 255),
            net_u16_nonneg(clan),
            tail,
        );
        let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
        let (cx, cy) = crate::world::World::chunk_pos(rx, ry);
        state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
    }
}

/// `Player.Hurt(num, Pure)` — без `AntiGun`; смерть через `handle_death` после отпускания ECS (как предметы в `heal_inventory`).
pub fn hurt_player_pure(state: &Arc<GameState>, pid: PlayerId, damage: i32) {
    if damage <= 0 {
        return;
    }
    let result = state
        .modify_player(pid, |ecs, entity| {
            let (h, mh, conn_tx, px, py) = {
                let c = ecs.get::<crate::game::player::PlayerConnection>(entity)?;
                let conn_tx = c.tx.clone();
                let Some(s) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
                    send_death_state_error(&conn_tx);
                    return Some((None, None));
                };
                let Some(p) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                    send_death_state_error(&conn_tx);
                    return Some((None, None));
                };
                if ecs
                    .get::<crate::game::player::PlayerFlags>(entity)
                    .is_none()
                {
                    send_death_state_error(&conn_tx);
                    return Some((None, None));
                }
                (s.health, s.max_health, conn_tx, p.x, p.y)
            };

            // S3-1: Health skill exp on every hurt (C# Player.Hurt → Health.AddExp)
            if let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity) {
                let ctx = crate::game::ExpContext::from_state(state);
                if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "l", 1.0) {
                    if let Some(c) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        let _ =
                            c.tx.send(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));
                    }
                }
            }

            let lethal = h <= damage;
            let new_h = if lethal { 0 } else { h - damage };
            {
                let mut s_mut = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                s_mut.health = new_h;
            }
            {
                let mut f_mut = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)?;
                f_mut.dirty = true;
            }
            let _ = conn_tx.send(crate::net::session::wire::make_u_packet_bytes(
                "@L",
                &health(new_h, mh).1,
            ));
            Some(if lethal {
                (Some(conn_tx), Some((px, py)))
            } else {
                (None, Some((px, py)))
            })
        })
        .flatten();
    if let Some((dead_tx, pos)) = result {
        if let Some(conn_tx) = dead_tx {
            handle_death(state, &conn_tx, pid);
        } else if let Some((px, py)) = pos {
            // S3-1: Hurt FX broadcast to nearby (C# SendDFToBots(6, 0, 0, id, 0))
            use crate::protocol::packets::hb_directed_fx;
            let fx = hb_directed_fx(
                crate::net::session::util::net_u16_nonneg(pid),
                0,
                0,
                6,
                0,
                0,
            );
            state.broadcast_hb_at(px, py, &[fx], Some(pid));
        }
    }
}

/// Внутри одного `ecs.write()` после `schedule.run`: снять `DeathQueue` и применить `Player.Death` для пушки.
/// Возвращает `(pid, rx, ry, mh, broadcasts)` — broadcast'ы выполнить ПОСЛЕ отпускания `ecs.write()`.
pub fn flush_player_death_queue_after_tick(
    state: &Arc<GameState>,
    ecs: &mut bevy_ecs::prelude::World,
) -> Vec<(PlayerId, i32, i32, i32, DeathBroadcasts)> {
    use std::collections::HashSet;
    let raw = std::mem::take(&mut ecs.resource_mut::<crate::game::combat::DeathQueue>().0);
    let mut seen = HashSet::new();
    let pids: Vec<PlayerId> = raw.into_iter().filter(|p| seen.insert(*p)).collect();
    let mut pending = Vec::new();
    for pid in pids {
        match apply_player_death_core(state, ecs, pid) {
            Ok((rx, ry, mh, bcast)) => pending.push((pid, rx, ry, mh, bcast)),
            Err(error) => {
                tracing::error!(player_id = %pid, ?error, "Queued player death aborted");
            }
        }
    }
    pending
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::players::PlayerRow;
    use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct DeathTestState {
        state: Arc<GameState>,
        player: PlayerRow,
        db_path: PathBuf,
        world_name: String,
        dir: PathBuf,
    }

    impl DeathTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(
                self.dir
                    .join(format!("{}_durability.mapb", self.world_name)),
            );
        }
    }

    async fn make_death_test_state(label: &str) -> DeathTestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("death_{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("death-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("death_world_{label}_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        DeathTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = bytes::BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn player_health(state: &Arc<GameState>, pid: PlayerId) -> i32 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let health_stats = ecs.get::<PlayerStats>(entity)?;
                Some(health_stats.health)
            })
            .unwrap()
    }

    fn player_pos(state: &Arc<GameState>, pid: PlayerId) -> (i32, i32) {
        state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<PlayerPosition>(entity)?;
                Some((pos.x, pos.y))
            })
            .unwrap()
    }

    #[tokio::test]
    async fn handle_death_missing_flags_is_explicit_error_without_respawn_mutation() {
        let test = make_death_test_state("missing_flags_death").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let before_health = player_health(&test.state, pid);
        let before_pos = player_pos(&test.state, pid);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        handle_death(&test.state, &tx, pid);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_health(&test.state, pid), before_health);
        assert_eq!(player_pos(&test.state, pid), before_pos);

        test.cleanup();
    }

    #[tokio::test]
    async fn hurt_player_pure_missing_flags_is_explicit_error_without_damage_mutation() {
        let test = make_death_test_state("missing_flags_hurt").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            let mut stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
            stats.health = 90;
            ecs.entity_mut(entity).remove::<PlayerFlags>();
        }

        hurt_player_pure(&test.state, pid, 10);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_health(&test.state, pid), 90);

        test.cleanup();
    }
}
