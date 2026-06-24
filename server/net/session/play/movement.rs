//! Движение робота по миру и рассылка HB соседям.
//! Референс: C# `Player.Move` — БЕЗ серверного cooldown внутри Move
//! (тайминг движения клиентский, `SpeedPacket`). Серверный silent-drop
//! cooldown ломал client-prediction → rubber-band, убран (1:1 C#).
use crate::game::buildings::PackType;
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats};
use crate::net::session::prelude::*;

/// Исход `Move` внутри ECS-лока. `Autodig` сигнализирует, что нужно копнуть
/// ПОСЛЕ освобождения лока (`handle_dig` сам берёт `modify_player` —
/// реентрантность лока недопустима).
enum MoveOutcome {
    Moved {
        nx: i32,
        ny: i32,
        ndir: i32,
        skin: i32,
        clan: i32,
    },
    Autodig(i32),
}

#[allow(clippy::similar_names, clippy::too_many_arguments)]
pub fn handle_move(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    _client_time: u32,
    target_x: i32,
    target_y: i32,
    dir: i32,
    // `true` — ход инициирован программатором (а не игроком). Пропускает guards
    // «программа бежит»/«окно открыто», но ВСЯ валидация (coord/коллизия/ворота/
    // дистанция) общая — чтобы программатор делал ровно то же, что ручной ход
    // (no-DRY): нельзя пройти сквозь блоки.
    programmatic: bool,
) {
    // C# `Player.cs:414-415`: Ctrl-move шлёт `(dir+10)` — снять флаг (клиент
    // `ClientController.cs:1022`). `dir==-1` (autoDig-в-стену) проходит без изменений.
    let dir = if dir > 9 { dir - 10 } else { dir };

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
            // 1. Immutable data gathering
            let (px, py, skin, clan, window_open, prog_running, auto_dig) = {
                let pos = ecs.get::<PlayerPosition>(entity)?;
                let stats = ecs.get::<PlayerStats>(entity)?;
                let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
                let prog = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                let settings = ecs.get::<crate::game::player::PlayerSettings>(entity)?;
                (
                    pos.x,
                    pos.y,
                    stats.skin,
                    stats.clan_id.unwrap_or(0),
                    ui.current_window.is_some(),
                    prog.running,
                    settings.auto_dig,
                )
            };

            // 2. 1:1 C# `Player.Move`: ВНУТРИ Move НЕТ серверного cooldown.
            // Тайминг движения — клиентский (SpeedPacket pacing). Серверный
            // silent-drop cooldown (re-added рефактором) ломал client-
            // prediction: при любом стопоре tick'а очередь Xmov батчилась,
            // лишние ходы тихо дропались, клиент уходил вперёд → dist>порог
            // → жёсткий @T rubber-band. Убрано (1:1 C# Move + server/
            // CLAUDE.md методология «rate-limit делает клиент» + ae637b9).
            // C# `Move`: `(win != null && !prog)` → tp; `TryAct`:
            // ProgRunning → silent return.
            if prog_running && !programmatic {
                return None;
            }
            if window_open && !programmatic {
                tracing::debug!(
                    player_id = pid,
                    x = px,
                    y = py,
                    reason = "window_open",
                    "Movement rejected"
                );
                tp_back("window", tx, px, py, target_x, target_y, "");
                return None;
            }

            // 3. Movement validation
            if !state.world.valid_coord(target_x, target_y) {
                tp_back("invalid_coord", tx, px, py, target_x, target_y, "");
                return None;
            }

            if !state.world.is_empty(target_x, target_y) {
                let cell = state.world.get_cell(target_x, target_y);
                tracing::debug!(
                    player_id = pid,
                    cell,
                    x = px,
                    y = py,
                    dest_x = target_x,
                    dest_y = target_y,
                    reason = "obstacle",
                    "Movement rejected"
                );
                tp_back(
                    "not_empty",
                    tx,
                    px,
                    py,
                    target_x,
                    target_y,
                    &format!("cell={cell}"),
                );
                // 1:1 C# `Player.cs:429-437`: непустая клетка + `dir==-1` + autoDig →
                // tp назад и копнуть (`Bz`). Направление копки — из дельты (как this.dir
                // в C# `Player.cs:416-417`), совпадает с `dir_offset`. Иначе просто tp.
                if dir == -1 && auto_dig {
                    let dig_dir = if px > target_x {
                        1
                    } else if px < target_x {
                        3
                    } else if py > target_y {
                        2
                    } else {
                        0
                    };
                    return Some(MoveOutcome::Autodig(dig_dir));
                }
                return None;
            }

            // Gate check (footprint-aware): ворота многоклеточные, а
            // `building_index` хранит ТОЛЬКО origin — раньше вход в не-origin
            // клетку ворот обходил чек, и игрок застревал внутри. Ищем пак,
            // ПОКРЫВАЮЩИЙ клетку (1:1 C# `PackPart`), затем здание по его origin.
            if let Some((ox, oy)) =
                GameState::find_pack_covering_with(ecs, &state.chunk_buildings, target_x, target_y)
                && let Some(bld_entity) = state.building_index.get(&(ox, oy))
            {
                let bld_entity = *bld_entity;
                if let (Some(meta), Some(ownership)) = (
                    ecs.get::<crate::game::BuildingMetadata>(bld_entity),
                    ecs.get::<crate::game::BuildingOwnership>(bld_entity),
                ) {
                    if meta.pack_type == PackType::Gate && ownership.clan_id != clan {
                        tracing::debug!(
                            player_id = pid,
                            gate_clan = ownership.clan_id,
                            player_clan = clan,
                            x = px,
                            y = py,
                            dest_x = target_x,
                            dest_y = target_y,
                            reason = "gate",
                            "Movement rejected"
                        );
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

            let dx = (target_x - px) as f32;
            let dy = (target_y - py) as f32;
            let dist = dx.hypot(dy);
            // 1:1 C# `Player.cs:441`: `if (Distance < 1.2f) accept else tp`.
            // Безопасно теперь, когда cooldown-дропов нет (сервер
            // обрабатывает каждый ход → dist всегда ~1.0 при честной игре).
            if dist >= 1.2 {
                tracing::debug!(
                    player_id = pid,
                    dist,
                    x = px,
                    y = py,
                    dest_x = target_x,
                    dest_y = target_y,
                    reason = "distance",
                    "Movement rejected"
                );
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

            // 1:1 C# `Player.cs:416-418`: позиция меняется (или dir==-1) →
            // направление из дельты реального хода; иначе — присланный dir.
            let actual_dir = if dir == -1 || px != target_x || py != target_y {
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
                dir
            };

            // 4. State updates
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

            // Exp and skills
            {
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkills>(entity)?;
                if add_skill_exp(&mut skills.states, "M", 1.0) {
                    let sk = skills_packet(&skill_progress_payload(&skills.states));
                    send_u_packet(tx, sk.0, &sk.1);
                }
            }

            Some(MoveOutcome::Moved {
                nx: target_x,
                ny: target_y,
                ndir: actual_dir,
                skin,
                clan,
            })
        })
        .flatten();

    let (nx, ny, ndir, skin, clan) = match result {
        Some(MoveOutcome::Moved {
            nx,
            ny,
            ndir,
            skin,
            clan,
        }) => (nx, ny, ndir, skin, clan),
        Some(MoveOutcome::Autodig(dig_dir)) => {
            // C# `Player.cs:434`: Bz() после tp назад. handle_dig сам берёт лок —
            // вызываем ПОСЛЕ закрытия modify_player.
            crate::net::session::play::dig_build::handle_dig(state, tx, pid, dig_dir);
            return;
        }
        None => return,
    };

    {
        let tail = state
            .query_player(pid, |ecs, entity| {
                ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                    .map_or(0, |ps| u8::from(ps.running))
            })
            .unwrap_or(0);
        let (cx, cy) = World::chunk_pos(nx, ny);
        let bot = hb_bot(
            net_u16_nonneg(pid),
            net_u16_nonneg(nx),
            net_u16_nonneg(ny),
            net_u8_clamped(ndir, 3),
            net_u8_clamped(skin, 255),
            net_u16_nonneg(clan),
            tail,
        );
        let hb_data = encode_hb_bundle(&hb_bundle(&[bot]).1);
        // Включаем владельца (None): клиентский `RobotRenderer.XYBot` ЖДЁТ X своего
        // бота. Ручной ход (tail=0) → пишет `myBotLastSync` (гейт-реконсиляция);
        // программаторный (tail=1) → `SetXY + SetRotation` (бот идёт И поворачивается).
        state.broadcast_to_nearby(cx, cy, &hb_data, None);
        crate::net::session::play::chunks::check_chunk_changed(state, tx, pid);

        // Feature 1: ref Player.cs:462-467 — auto-open pack GUI на ORIGIN-клетке пака.
        // C# `World.AddPack` регистрирует пак ТОЛЬКО в одной клетке (origin, `ch.SetPack(x,y,p)`),
        // поэтому `ContainsPack`/`GetPack` срабатывает лишь на origin, НЕ на всём футпринте.
        // Footprint-aware `find_pack_covering` (80967d4) был РЕГРЕССОМ: площадка спавна Resp
        // (road-клетки футпринта) открывала GUI. Возврат к origin-only = 1:1 C#.
        if let Some(view) = state.get_pack_at(nx, ny) {
            if view.pack_type != PackType::Gate && (view.clan_id == 0 || view.clan_id == clan) {
                let prog_running = state
                    .query_player(pid, |ecs, entity| {
                        ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                            .is_some_and(|p| p.running)
                    })
                    .unwrap_or(false);
                if !prog_running {
                    crate::net::session::ui::gui_buttons::open_pack_gui(state, tx, pid, &view);
                }
            }
        }
    }
}

// handle_move_pure удалён (no-DRY): программатор зовёт handle_move(..., true) —
// та же валидация коллизии/ворот/дистанции, что и ручной ход.
