use crate::game::{GameState, PlayerId};
use crate::world::WorldProvider;
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Eq, PartialEq)]
pub enum GeologyResult {
    Applied {
        geo_name: String,
        changed_cells: Vec<(i32, i32)>,
    },
    SilentNoop,
    MissingState(&'static str),
    MissingEntity,
}

fn place_geo_stack_cell(
    state: &GameState,
    cell_defs: &crate::world::cells::CellDefs,
    ecs: &mut bevy_ecs::prelude::World,
    entity: bevy_ecs::prelude::Entity,
    target_x: i32,
    target_y: i32,
) -> bool {
    let Some(cell_code) = ecs
        .get_mut::<crate::game::player::PlayerGeoStack>(entity)
        .expect("PlayerGeoStack checked before geo place")
        .0
        .pop()
    else {
        return false;
    };
    let place_cell = crate::world::CellType(cell_code);
    let durability = if place_cell.is_crystal() || rand::rng().random_range(1..=100) > 99 {
        0.0
    } else {
        cell_defs.get_typed(place_cell).durability
    };
    state.world.write_world_cell(
        target_x,
        target_y,
        crate::world::WorldCell {
            cell_type: place_cell,
            durability,
        },
    );
    true
}

pub fn apply_geology(state: &Arc<GameState>, pid: PlayerId, programmatic: bool) -> GeologyResult {
    let cell_defs = state.world.cell_defs();
    state
        .modify_player(pid, |ecs, entity| {
            let Some(program_state) =
                ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
            else {
                tracing::error!(player_id = %pid, component = "ProgrammatorState", "Player component missing for geo");
                return Some(GeologyResult::MissingState("ProgrammatorState"));
            };
            if !programmatic && !program_state.is_manual_control_allowed() {
                return Some(GeologyResult::SilentNoop);
            }

            let Some(cooldowns) = ecs.get::<crate::game::player::PlayerCooldowns>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerCooldowns", "Player component missing for geo");
                return Some(GeologyResult::MissingState("PlayerCooldowns"));
            };
            if !programmatic
                && cooldowns.last_geo.elapsed()
                    < Duration::from_millis(state.config.gameplay.cooldowns.geo_ms)
            {
                return Some(GeologyResult::SilentNoop);
            }

            let Some(skills) = ecs.get::<crate::game::player::PlayerSkillsComp>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for geo");
                return Some(GeologyResult::MissingState("PlayerSkillsComp"));
            };
            if skills
                .states
                .find(crate::game::skills::SkillType::Geology.code())
                .is_none()
            {
                return Some(GeologyResult::SilentNoop);
            }

            let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerPosition", "Player component missing for geo");
                return Some(GeologyResult::MissingState("PlayerPosition"));
            };
            let Some(player_stats) = ecs.get::<crate::game::player::PlayerStats>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for geo");
                return Some(GeologyResult::MissingState("PlayerStats"));
            };
            if ecs
                .get::<crate::game::player::PlayerGeoStack>(entity)
                .is_none()
            {
                tracing::error!(player_id = %pid, component = "PlayerGeoStack", "Player component missing for geo");
                return Some(GeologyResult::MissingState("PlayerGeoStack"));
            }

            let (dx, dy) = crate::game::direction::dir_offset(pos.dir);
            let (tgt_x, tgt_y) = (pos.x + dx, pos.y + dy);
            let clan_id = player_stats.clan_id.unwrap_or(0);
            let mut changed_cells = Vec::new();

            if state.world.valid_coord(tgt_x, tgt_y)
                && state.access_gun_full_in_ecs(ecs, tgt_x, tgt_y, clan_id).0
            {
                let cell = state.world.get_cell_typed(tgt_x, tgt_y);
                let cell_props = cell_defs.get_typed(cell);
                let pickable = cell_props.nature.is_pickable && !cell_props.cell_is_empty();
                let place_here = cell_props.cell_is_empty()
                    && cell_props.can_place_over()
                    && state.find_pack_covering_in_ecs(ecs, tgt_x, tgt_y).is_none();

                if pickable {
                    ecs.get_mut::<crate::game::player::PlayerGeoStack>(entity)
                        .expect("PlayerGeoStack checked before geo pick")
                        .0
                        .push(cell.0);
                    state.world.destroy(tgt_x, tgt_y);
                    changed_cells.push((tgt_x, tgt_y));
                } else if place_here
                    && place_geo_stack_cell(state, &cell_defs, ecs, entity, tgt_x, tgt_y)
                {
                    changed_cells.push((tgt_x, tgt_y));
                }
            }

            let geo_name = ecs
                .get::<crate::game::player::PlayerGeoStack>(entity)
                .and_then(|stack| stack.0.last())
                .map(|&cell| cell_defs.get(cell).name.clone())
                .unwrap_or_default();

            ecs.get_mut::<crate::game::player::PlayerCooldowns>(entity)
                .expect("PlayerCooldowns checked before geo cooldown update")
                .last_geo = Instant::now();

            Some(GeologyResult::Applied {
                geo_name,
                changed_cells,
            })
        })
        .flatten()
        .unwrap_or(GeologyResult::MissingEntity)
}
