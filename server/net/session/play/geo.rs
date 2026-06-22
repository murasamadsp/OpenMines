//! Геология (Xgeo) — pickup/place блоков.
use crate::game::broadcast_cell_update;
use crate::game::player::{
    PlayerCooldowns, PlayerGeoStack, PlayerPosition, PlayerSkills, PlayerStats,
};
use crate::game::programmator::ProgrammatorState;
use crate::game::skills::SkillType;
use crate::net::session::prelude::*;
use rand::Rng;

/// `Session.GeoHandler` → `TryAct(player.Geo, 200)` → `PEntity.Geo` + `SendGeo` (`pSenders.cs`).
pub fn handle_geo(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let result = state
        .modify_player(pid, |ecs, entity| {
            if ecs
                .get::<ProgrammatorState>(entity)
                .is_some_and(|p| p.running)
            {
                return None;
            }
            {
                let cd = ecs.get::<PlayerCooldowns>(entity)?;
                if cd.last_geo.elapsed() < Duration::from_millis(200) {
                    return None;
                }
            }

            // НАМЕРЕННАЯ ДЕВИАЦИЯ от C# по ПРЯМОМУ ТРЕБОВАНИЮ ПОЛЬЗОВАТЕЛЯ:
            // геология работает ТОЛЬКО при УСТАНОВЛЕННОМ в слот скилле Geology.
            // В эталоне (`PEntity.Geo`) гейта нет — гео доступно без скилла; юзер
            // явно указал требовать установленный скилл. `find(code)` = в слоте
            // (НЕ `get_player_skill_effect`, который для неустановленного даёт
            // эффект уровня 0 и может быть >0).
            let geology_installed = ecs
                .get::<PlayerSkills>(entity)
                .is_some_and(|sk| sk.states.find(SkillType::Geology.code()).is_some());
            if !geology_installed {
                return None;
            }

            let (px, py, dir) = {
                let pos = ecs.get::<PlayerPosition>(entity)?;
                (pos.x, pos.y, pos.dir)
            };
            let cid = ecs.get::<PlayerStats>(entity)?.clan_id.unwrap_or(0);
            let (dx, dy) = dir_offset(dir);
            let (tgt_x, tgt_y) = (px + dx, py + dy);

            let mut broadcast: Vec<(i32, i32)> = Vec::new();

            if state.world.valid_coord(tgt_x, tgt_y)
                && GameState::access_gun_with(ecs, &state.chunk_buildings, tgt_x, tgt_y, cid).0
            {
                let cell = state.world.get_cell(tgt_x, tgt_y);
                let defs = state.world.cell_defs();
                let prop = defs.get(cell);
                let pickable = prop.nature.is_pickable && !prop.cell_is_empty();
                let place_here = prop.cell_is_empty()
                    && prop.can_place_over()
                    && GameState::find_pack_covering_with(
                        ecs,
                        &state.chunk_buildings,
                        tgt_x,
                        tgt_y,
                    )
                    .is_none();

                if pickable {
                    {
                        let mut stack = ecs.get_mut::<PlayerGeoStack>(entity)?;
                        stack.0.push(cell);
                    }
                    state.world.destroy(tgt_x, tgt_y);
                    broadcast.push((tgt_x, tgt_y));
                } else if place_here {
                    if let Some(cplaceable) = ecs.get_mut::<PlayerGeoStack>(entity)?.0.pop() {
                        state.world.set_cell(tgt_x, tgt_y, cplaceable);
                        let d = if is_crystal(cplaceable) {
                            0.0
                        } else {
                            let mut rng = rand::rng();
                            if rng.random_range(1..=100) > 99 {
                                0.0
                            } else {
                                defs.get(cplaceable).durability
                            }
                        };
                        state.world.set_durability(tgt_x, tgt_y, d);
                        broadcast.push((tgt_x, tgt_y));
                    }
                }
            }

            let geo_name = ecs
                .get::<PlayerGeoStack>(entity)
                .and_then(|s| s.0.last())
                .map(|&c| state.world.cell_defs().get(c).name.clone())
                .unwrap_or_default();

            {
                let mut cd = ecs.get_mut::<PlayerCooldowns>(entity)?;
                cd.last_geo = Instant::now();
            }

            Some((geo_name, broadcast))
        })
        .flatten();

    let Some((geo_name, broadcast)) = result else {
        return;
    };
    for (x, y) in broadcast {
        broadcast_cell_update(state, x, y);
    }
    send_u_packet(tx, "GE", &geo(&geo_name).1);
}
