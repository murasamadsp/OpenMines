//! Движение робота по миру и рассылка HB соседям.
//! Референс: `Session.MoveHandler` → `player.TryAct(() => player.Move(parent.X, parent.Y, dir), player.ServerPause)`
use crate::game::buildings::PackType;
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats, PlayerUI};
use crate::net::session::prelude::*;

#[allow(clippy::similar_names)]
pub fn handle_move(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    client_time: u32,
    target_x: i32,
    target_y: i32,
    dir: i32,
) {
    let tp_back = |reason: &str,
                   txc: &mpsc::UnboundedSender<Vec<u8>>,
                   from_x: i32,
                   from_y: i32,
                   to_x: i32,
                   to_y: i32,
                   extra: &str| {
        tracing::debug!(
            "[Move] TP back reason={reason} pid={pid} from=({from_x},{from_y}) to=({to_x},{to_y}) {extra}"
        );
        send_u_packet(txc, "@T", &tp(from_x, from_y).1);
    };

    let result = state
        .modify_player(pid, |ecs, entity| {
            let (px, py, skin, clan) = {
                let pos = ecs.get::<PlayerPosition>(entity)?;
                let stats = ecs.get::<PlayerStats>(entity)?;
                let ui = ecs.get::<PlayerUI>(entity)?;

                let pos_x = pos.x;
                let pos_y = pos.y;
                let skin = stats.skin;
                let clan = stats.clan_id.unwrap_or(0);

                // Клиент — истина по таймингу движения; серверный cooldown убран.
                // Клиент сам пейсит Xmov по `SpeedPacket`, сервер только валидирует позицию.

                // 1:1 ref: `(win != null && !prog) => tp back`. For normal `Xmov` we treat it as `!prog`.
                if ui.current_window.is_some() {
                    tp_back("window", tx, pos_x, pos_y, target_x, target_y, "");
                    return None;
                }

                // ref Player.cs:214-216: `if (programsData.ProgRunning) return;` — silent drop when programmator is running.
                if let Some(prog) = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                {
                    if prog.running {
                        return None;
                    }
                }

                (pos_x, pos_y, skin, clan)
            };

            // Референс: ValidCoord check
            if !state.world.valid_coord(target_x, target_y) {
                tp_back("invalid_coord", tx, px, py, target_x, target_y, "");
                return None;
            }

            // Референс: dir computation — if position changed, compute from delta; otherwise use client dir
            let actual_dir = if px != target_x || py != target_y {
                if px > target_x {
                    1
                } else if px < target_x {
                    3
                } else if py > target_y {
                    2
                } else {
                    0
                }
            } else {
                let d = if dir > 9 { dir - 10 } else { dir };
                d.clamp(0, 3)
            };

            // Референс: `!GetProp(cell).isEmpty` → tp back
            if !state.world.is_empty(target_x, target_y) {
                let cell = state.world.get_cell(target_x, target_y);
                tp_back(
                    "not_empty",
                    tx,
                    px,
                    py,
                    target_x,
                    target_y,
                    &format!("cell={cell}"),
                );
                return None;
            }

            // 1:1 ref: Gate blocks movement for other clans (`pack is Gate && pack.cid != cid`).
            // Нельзя вызывать `state.get_pack_at()` — она берёт `ecs.read()`, а мы уже под `ecs.write()` (self-deadlock).
            // Используем `building_index` + `ecs` напрямую из замыкания.
            if let Some(bld_entity) = state.building_index.get(&(target_x, target_y)) {
                let bld_entity = *bld_entity;
                if let (Some(meta), Some(ownership)) = (
                    ecs.get::<crate::game::BuildingMetadata>(bld_entity),
                    ecs.get::<crate::game::BuildingOwnership>(bld_entity),
                ) {
                    if meta.pack_type == PackType::Gate && ownership.clan_id != clan {
                        tp_back(
                            "gate",
                            tx,
                            px,
                            py,
                            target_x,
                            target_y,
                            &format!("pack_clan={} player_clan={clan}", ownership.clan_id),
                        );
                        return None;
                    }
                }
            }

            // Референс: `Distance < 1.2` — accept; otherwise tp back
            let dx = (target_x - px) as f32;
            let dy = (target_y - py) as f32;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= 1.2 {
                tp_back(
                    "dist",
                    tx,
                    px,
                    py,
                    target_x,
                    target_y,
                    &format!("dist={dist:.3}"),
                );
                return None;
            }

            {
                let mut pos_mut = ecs.get_mut::<PlayerPosition>(entity)?;
                pos_mut.x = target_x;
                pos_mut.y = target_y;
                pos_mut.dir = actual_dir;
            }
            {
                let mut flags_mut = ecs.get_mut::<PlayerFlags>(entity)?;
                flags_mut.dirty = true;
            }

            Some((target_x, target_y, actual_dir, skin, clan))
        })
        .flatten();

    if let Some((nx, ny, ndir, skin, clan)) = result {
        let (cx, cy) = World::chunk_pos(nx, ny);
        let bot = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(nx),
            net_u16_nonneg(ny),
            net_u8_clamped(ndir, 3),
            net_u8_clamped(skin, 255),
            net_u16_nonneg(clan),
            0,
        );
        let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
        state.broadcast_to_nearby(cx, cy, &hb_data, Some(pid));
        crate::net::session::play::chunks::check_chunk_changed(state, tx, pid);

        // Feature 1: ref Player.cs:462-467 — auto-open pack GUI when landing on a building cell.
        // Note: `get_pack_at` finds buildings whose origin is exactly at (nx, ny). Multi-cell
        // footprint coverage requires a reverse index (separate concern, same limitation as the
        // gate check above). Programmator check mirrors C#: `!programsData.ProgRunning`.
        if let Some(view) = state.get_pack_at(nx, ny) {
            // C# ref: `Gate.GUIWin()` returns null — stepping on a gate never opens a window.
            if view.pack_type != PackType::Gate && (view.clan_id == 0 || view.clan_id == clan) {
                let prog_running = state
                    .query_player(pid, |ecs, entity| {
                        ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                            .map_or(false, |p| p.running)
                    })
                    .unwrap_or(false);
                if !prog_running {
                    crate::net::session::ui::gui_buttons::open_pack_gui(state, tx, pid, &view);
                }
            }
        }

        // Feature 2: ref Player.cs:428-436 (dir == -1 autoDig branch) — N/A for the Xmov path.
        // In the Rust architecture, `handle_dig` never calls `handle_move`; it reads player
        // position independently and digs the adjacent cell directly. The `dir == -1` case in C#
        // is an internal Move() call from the dig handler, which does not exist here.
    }
}
