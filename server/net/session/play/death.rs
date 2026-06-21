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
    /// Программа была запущена и остановлена смертью (не `RespawnOnProg`-продолжение).
    /// C# шлёт `ProgrammatorPacket(false)` (@P) только в этом случае (Player.cs:935).
    pub prog_stopped: bool,
}

/// Мутации ECS как в `Player.Death()` (`Player.cs`).
/// **НЕ** вызывает ничего, что лочит `state.ecs` (`broadcast/get_pack_at`) —
/// вместо этого возвращает `DeathBroadcasts` для вызывающего.
pub fn apply_player_death_core(
    state: &Arc<GameState>,
    ecs: &mut bevy_ecs::prelude::World,
    pid: PlayerId,
) -> Option<(i32, i32, i32, DeathBroadcasts)> {
    let entity = state.get_player_entity(pid)?;
    let (pos_x, pos_y, cry, rebind_x, rebind_y, mh) = {
        let s = ecs.get::<crate::game::player::PlayerStats>(entity)?;
        let p = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let m = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
        (p.x, p.y, s.crystals, m.resp_x, m.resp_y, s.max_health)
    };

    let mut bcast = DeathBroadcasts {
        box_cell: None,
        // 1:1 C# `Player.Death` (Player.cs:912) — `SendFXoBots(2,x,y)`
        // БЕЗУСЛОВНО (даже с пустой корзиной соседи видят анимацию смерти).
        fx_death: Some((pos_x, pos_y)),
        death_pos: (pos_x, pos_y),
        money: 0,
        creds: 0,
        resp_used: false,
        prog_stopped: false,
    };

    if cry.iter().sum::<i64>() > 0 {
        let c = cry;
        let box_placed = pick_box_coord(
            pos_x,
            pos_y,
            |x, y| state.world.valid_coord(x, y),
            |x, y| {
                if !state.world.is_empty(x, y) {
                    return false;
                }
                let cell = state.world.get_cell(x, y);
                state.world.cell_defs().get(cell).can_place_over()
            },
        )
        .and_then(|(bx, by)| {
            if GameState::find_pack_covering_with(ecs, &state.chunk_buildings, bx, by).is_none() {
                state
                    .world
                    .set_cell(bx, by, crate::world::cells::cell_type::BOX);
                // C-2 фикс: in-memory put + отложенная персистенция вместо
                // sync `db.upsert_box` под удерживаемым `ecs.write()` (death
                // flush в tick-цикле) — фризило при каждой смерти.
                state.box_put(bx, by, c);
                if let Some(mut s) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
                    s.crystals = [0; 6];
                }
                Some((bx, by))
            } else {
                // Даже без бокса — обнулить кристаллы
                if let Some(mut s) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
                    s.crystals = [0; 6];
                }
                None
            }
        });
        bcast.box_cell = box_placed;
        // fx_death уже выставлен безусловно при создании bcast (см. выше).
    }

    let mut is_free_resp = false;
    // Респаун: проверяем pack через уже имеющийся &mut ecs (без отдельного лока)
    let (rx, ry) = if let (Some(x), Some(y)) = (rebind_x, rebind_y) {
        // Collect resp building data immutably first, then mutate.
        let resp_data = state.building_index.get(&(x, y)).and_then(|ent| {
            let bld_ent = *ent;
            let meta = ecs.get::<crate::game::buildings::BuildingMetadata>(bld_ent)?;
            let b_stats = ecs.get::<crate::game::buildings::BuildingStats>(bld_ent)?;
            if meta.pack_type == crate::game::buildings::PackType::Resp && b_stats.charge > 0.0 {
                Some((bld_ent, b_stats.cost))
            } else {
                None
            }
        });
        if let Some((bld_ent, resp_cost)) = resp_data {
            if resp_cost == 0 {
                is_free_resp = true;
            }
            // Deduct resp cost from player money, add to building storage.
            let cost = if resp_cost > 0 {
                i64::from(resp_cost)
            } else {
                10i64
            };
            if let Some(mut s) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
                s.money -= cost;
            }
            if let Some(mut bld_stats) =
                ecs.get_mut::<crate::game::buildings::BuildingStats>(bld_ent)
            {
                bld_stats.charge -= 1.0;
            }
            if let Some(mut bld_storage) =
                ecs.get_mut::<crate::game::buildings::BuildingStorage>(bld_ent)
            {
                bld_storage.money += cost;
            }
            if let Some(mut bld_flags) =
                ecs.get_mut::<crate::game::buildings::BuildingFlags>(bld_ent)
            {
                bld_flags.dirty = true;
            }
            // C# ref: Resp.OnRespawn calls p.SendMoney() — capture for later
            if let Some(s) = ecs.get::<crate::game::player::PlayerStats>(entity) {
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
    };

    {
        let mut p = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
        p.x = rx;
        p.y = ry;
    }
    if let Some(mut s) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) {
        s.health = mh;
    }
    if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
        ui.current_window = None;
    }
    if let Some(mut v) = ecs.get_mut::<crate::game::player::PlayerView>(entity) {
        v.last_chunk = None;
        v.visible_chunks.clear();
    }
    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
        f.dirty = true;
    }
    if let Some(mut prog) = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity) {
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
    }

    Some((rx, ry, mh, bcast))
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
}

pub fn send_respawn_after_death(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    rx: i32,
    ry: i32,
    mh: i32,
    bcast: &DeathBroadcasts,
) {
    tracing::warn!("[Respawn] @T pid={pid} to=({rx},{ry}) mh={mh}");
    // C# ref Death(): win=null → SendWindow() → tp → SendHealth()
    send_u_packet(tx, "Gu", &gu_close().1);
    send_u_packet(tx, "@T", &tp(rx, ry).1);
    send_u_packet(tx, "@L", &health(mh, mh).1);
    send_u_packet(tx, "@B", &basket(&[0; 6], 1).1);
    // C# ref: Resp.OnRespawn calls p.SendMoney() after cost deduction
    if bcast.resp_used {
        send_u_packet(tx, "P$", &money(bcast.money, bcast.creds).1);
    }
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
    if let Some((rx, ry, mh, bcast)) = result {
        run_death_broadcasts(state, &bcast, pid);
        send_respawn_after_death(tx, pid, rx, ry, mh, &bcast);
        check_chunk_changed(state, tx, pid);
    }
}

/// `Player.Hurt(num, Pure)` — без `AntiGun`; смерть через `handle_death` после отпускания ECS (как предметы в `heal_inventory`).
pub fn hurt_player_pure(state: &Arc<GameState>, pid: PlayerId, damage: i32) {
    if damage <= 0 {
        return;
    }
    let result = state
        .modify_player(pid, |ecs, entity| {
            // S3-1: Health skill exp on every hurt (C# Player.Hurt → Health.AddExp)
            if let Some(mut skills) = ecs.get_mut::<crate::game::player::PlayerSkills>(entity) {
                crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0);
                // Always send @S after skill exp (C# Skill.AddExp always sends)
                let sk = crate::protocol::packets::skills_packet(
                    &crate::game::skills::skill_progress_payload(&skills.states),
                );
                if let Some(c) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                    let _ =
                        c.tx.send(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));
                }
            }

            let (h, mh, conn_tx, px, py) = {
                let s = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                let c = ecs.get::<crate::game::player::PlayerConnection>(entity)?;
                let p = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                (s.health, s.max_health, c.tx.clone(), p.x, p.y)
            };
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
                (Some(conn_tx), px, py)
            } else {
                (None, px, py)
            })
        })
        .flatten();
    if let Some((dead_tx, px, py)) = result {
        if let Some(conn_tx) = dead_tx {
            handle_death(state, &conn_tx, pid);
        } else {
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
        if let Some((rx, ry, mh, bcast)) = apply_player_death_core(state, ecs, pid) {
            pending.push((pid, rx, ry, mh, bcast));
        }
    }
    pending
}
