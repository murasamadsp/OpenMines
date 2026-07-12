use crate::game::player::{
    PlayerCooldowns, PlayerFlags, PlayerPosition, PlayerSkillsComp, PlayerStats,
};
use crate::game::{ExpContext, GameState, PlayerId, WorldPos};
use crate::world::WorldProvider;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use std::sync::Arc;
use std::time::Instant;

const BOOM_DAMAGE: i32 = 40;
const BOOM_SCAN_RANGE: i32 = 4;
const BOOM_RADIUS_SQUARED: i64 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoomDueAction {
    pub center: WorldPos,
    pub rng_seed: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BoomPlayerHealthEffect {
    pub player_id: PlayerId,
    pub position: WorldPos,
    pub health: i32,
    pub max_health: i32,
    pub skill_progress: Option<crate::game::mechanics::events::SkillProgressSnapshot>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BoomFxEffect {
    Hurt {
        player_id: PlayerId,
        position: WorldPos,
    },
    Blast {
        position: WorldPos,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BoomApplyEffects {
    pub changed_cells: Vec<WorldPos>,
    pub player_health: Vec<BoomPlayerHealthEffect>,
    pub deaths: Vec<PlayerId>,
    pub cleared_pack: WorldPos,
    pub fx: Vec<BoomFxEffect>,
}

fn inside_boom_radius(position: WorldPos, center: WorldPos) -> bool {
    let dx = i64::from(position.0) - i64::from(center.0);
    let dy = i64::from(position.1) - i64::from(center.1);
    dx * dx + dy * dy <= BOOM_RADIUS_SQUARED
}

fn boom_cell_offsets() -> impl Iterator<Item = (i32, i32)> {
    (-BOOM_SCAN_RANGE..=BOOM_SCAN_RANGE).flat_map(|dx| {
        (-BOOM_SCAN_RANGE..=BOOM_SCAN_RANGE)
            .filter(move |&dy| inside_boom_radius(WorldPos(dx, dy), WorldPos(0, 0)))
            .map(move |dy| (dx, dy))
    })
}

fn red_rock_converts(rng_seed: u64, position: WorldPos) -> bool {
    let coordinate =
        (u64::from(position.0.cast_unsigned()) << 32) | u64::from(position.1.cast_unsigned());
    let mut mixed = rng_seed ^ coordinate;
    mixed = mixed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let mut rng = SmallRng::seed_from_u64(mixed ^ (mixed >> 31));
    rng.random_range(1_u8..=100) >= 99
}

fn nearby_player_candidates(state: &GameState, center: WorldPos) -> Vec<PlayerId> {
    if !state.world.valid_coord(center.0, center.1) {
        return Vec::new();
    }

    let max_world_x =
        i32::try_from(state.world.cells_width().saturating_sub(1)).unwrap_or(i32::MAX);
    let max_world_y =
        i32::try_from(state.world.cells_height().saturating_sub(1)).unwrap_or(i32::MAX);
    let min_x = center.0.saturating_sub(BOOM_SCAN_RANGE).max(0);
    let min_y = center.1.saturating_sub(BOOM_SCAN_RANGE).max(0);
    let max_x = center.0.saturating_add(BOOM_SCAN_RANGE).min(max_world_x);
    let max_y = center.1.saturating_add(BOOM_SCAN_RANGE).min(max_world_y);
    let (min_chunk_x, min_chunk_y) = crate::world::World::chunk_pos(min_x, min_y);
    let (max_chunk_x, max_chunk_y) = crate::world::World::chunk_pos(max_x, max_y);

    let mut candidates = Vec::new();
    for chunk_x in min_chunk_x..=max_chunk_x {
        for chunk_y in min_chunk_y..=max_chunk_y {
            candidates.extend(state.players_in_chunk(chunk_x, chunk_y));
        }
    }
    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

fn apply_boom_cells(state: &GameState, action: BoomDueAction) -> Vec<WorldPos> {
    let cell_defs = state.world.cell_defs();
    let ecs = state.ecs_read_profiled("boom.apply.cells");
    let mut changed = Vec::new();

    for (dx, dy) in boom_cell_offsets() {
        let Some(x) = action.center.0.checked_add(dx) else {
            continue;
        };
        let Some(y) = action.center.1.checked_add(dy) else {
            continue;
        };
        if !state.world.valid_coord(x, y) || state.find_pack_covering_in_ecs(&ecs, x, y).is_some() {
            continue;
        }

        let cell = state.world.get_cell_typed(x, y);
        if !cell_defs.get_typed(cell).physical.is_destructible {
            continue;
        }

        let target = if cell.is(crate::world::cells::cell_type::RED_ROCK) {
            if !red_rock_converts(action.rng_seed, WorldPos(x, y)) {
                continue;
            }
            crate::world::CellType(crate::world::cells::cell_type::ACID_ROCK)
        } else if cell.is(crate::world::cells::cell_type::ACID_ROCK) {
            crate::world::CellType(crate::world::cells::cell_type::ROCK)
        } else {
            state.world.destroy_cell_and_road(x, y);
            if state.world.get_cell_typed(x, y)
                == crate::world::CellType(crate::world::cells::cell_type::EMPTY)
            {
                changed.push(WorldPos(x, y));
            }
            continue;
        };

        state.world.set_cell_typed(x, y, target);
        if state.world.get_cell_typed(x, y) == target {
            changed.push(WorldPos(x, y));
        }
    }
    drop(ecs);
    changed
}

fn apply_boom_damage(
    state: &GameState,
    center: WorldPos,
) -> (Vec<BoomPlayerHealthEffect>, Vec<PlayerId>) {
    let candidates = nearby_player_candidates(state, center);
    let exp_context = ExpContext::from_state(state);
    let now = Instant::now();
    let mut ecs = state.ecs_write_profiled("boom.apply.players");
    let mut player_health = Vec::new();
    let mut deaths = Vec::new();

    for player_id in candidates {
        let Some(entity) = state.get_player_entity(player_id) else {
            continue;
        };
        let Some(position) = ecs
            .get::<PlayerPosition>(entity)
            .map(|position| WorldPos(position.x, position.y))
        else {
            continue;
        };
        if !inside_boom_radius(position, center)
            || ecs
                .get::<PlayerCooldowns>(entity)
                .and_then(|cooldowns| cooldowns.protection_until)
                .is_some_and(|until| now < until)
            || ecs
                .get::<PlayerStats>(entity)
                .is_none_or(|stats| stats.health <= 0)
            || ecs.get::<PlayerSkillsComp>(entity).is_none()
            || ecs.get::<PlayerFlags>(entity).is_none()
        {
            continue;
        }

        let skill_progress = exp_context.add_typed_skill_exp(
            &mut ecs
                .get_mut::<PlayerSkillsComp>(entity)
                .expect("PlayerSkillsComp checked before Boom damage")
                .states,
            crate::game::skills::SkillType::Health,
            1.0,
        );
        let (health, max_health) = {
            let mut player_stats = ecs
                .get_mut::<PlayerStats>(entity)
                .expect("PlayerStats checked before Boom damage");
            player_stats.health = player_stats.health.saturating_sub(BOOM_DAMAGE).max(0);
            (player_stats.health, player_stats.max_health)
        };
        ecs.get_mut::<PlayerFlags>(entity)
            .expect("PlayerFlags checked before Boom damage")
            .dirty = true;

        player_health.push(BoomPlayerHealthEffect {
            player_id,
            position,
            health,
            max_health,
            skill_progress,
        });
        if health == 0 {
            deaths.push(player_id);
        }
    }
    drop(ecs);
    (player_health, deaths)
}

pub fn apply_boom(state: &Arc<GameState>, action: BoomDueAction) -> BoomApplyEffects {
    let changed_cells = apply_boom_cells(state, action);
    let (player_health, deaths) = apply_boom_damage(state, action.center);
    state.remove_consumable_pack(action.center.0, action.center.1);
    let mut fx = player_health
        .iter()
        .filter(|effect| effect.health > 0)
        .map(|effect| BoomFxEffect::Hurt {
            player_id: effect.player_id,
            position: effect.position,
        })
        .collect::<Vec<_>>();
    fx.push(BoomFxEffect::Blast {
        position: action.center,
    });

    BoomApplyEffects {
        changed_cells,
        player_health,
        deaths,
        cleared_pack: action.center,
        fx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn move_player(
        state: &Arc<GameState>,
        player_id: PlayerId,
        position: WorldPos,
        protection_until: Option<Instant>,
    ) {
        state
            .modify_player(player_id, |ecs, entity| {
                let mut player_position = ecs.get_mut::<PlayerPosition>(entity).unwrap();
                player_position.x = position.0;
                player_position.y = position.1;
                let mut player_stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
                player_stats.health = 100;
                player_stats.max_health = 100;
                ecs.get_mut::<PlayerCooldowns>(entity)
                    .unwrap()
                    .protection_until = protection_until;
                ecs.get_mut::<PlayerFlags>(entity).unwrap().dirty = false;
            })
            .unwrap();
        state.unregister_player_from_all_chunks(player_id);
        let (chunk_x, chunk_y) = crate::world::World::chunk_pos(position.0, position.1);
        state.register_player_chunk(player_id, chunk_x, chunk_y);
    }

    fn clear_boom_area(state: &GameState, center: WorldPos) {
        for (dx, dy) in boom_cell_offsets() {
            state
                .world
                .destroy_cell_and_road(center.0 + dx, center.1 + dy);
        }
    }

    #[test]
    fn boom_cell_area_is_the_37_integer_points_inside_radius() {
        let offsets = boom_cell_offsets().collect::<Vec<_>>();
        assert_eq!(offsets.len(), 37);
        assert!(
            offsets
                .iter()
                .all(|&(x, y)| inside_boom_radius(WorldPos(x, y), WorldPos(0, 0)))
        );
        assert!(!offsets.contains(&(4, 0)));
    }

    #[test]
    fn red_rock_roll_is_stable_per_seed_and_coordinate() {
        let positions = [WorldPos(-1, 9), WorldPos(0, 9), WorldPos(1, 9)];
        let forward = positions.map(|position| red_rock_converts(42, position));
        let repeated = positions.map(|position| red_rock_converts(42, position));
        assert_eq!(forward, repeated);
    }

    #[tokio::test]
    async fn apply_boom_is_wire_free_and_returns_authoritative_effects() {
        let test = crate::test_support::ServerTestHarness::new("boom_apply", "boom-user").await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = PlayerId(test.player.id);
        let player_position = WorldPos(31, 31);
        let center = WorldPos(32, 31);
        move_player(&test.state, player_id, player_position, None);
        clear_boom_area(&test.state, center);

        test.state.world.set_cell_typed(
            center.0,
            center.1,
            crate::world::CellType(crate::world::cells::cell_type::ROAD),
        );
        test.state.world.set_cell_typed(
            center.0,
            center.1,
            crate::world::CellType(crate::world::cells::cell_type::ROCK),
        );
        test.state.put_consumable_pack(center.0, center.1, b'B', 0);

        let effects = apply_boom(
            &test.state,
            BoomDueAction {
                center,
                rng_seed: 7,
            },
        );

        assert_eq!(effects.changed_cells, vec![center]);
        assert_eq!(effects.cleared_pack, center);
        assert!(effects.deaths.is_empty());
        assert_eq!(effects.player_health.len(), 1);
        let health = &effects.player_health[0];
        assert_eq!(health.player_id, player_id);
        assert_eq!(health.position, player_position);
        assert_eq!((health.health, health.max_health), (60, 100));
        assert!(health.skill_progress.is_some());
        assert_eq!(
            effects.fx,
            vec![
                BoomFxEffect::Hurt {
                    player_id,
                    position: player_position,
                },
                BoomFxEffect::Blast { position: center },
            ]
        );
        assert_eq!(test.state.world.get_solid_cell(center.0, center.1), 0);
        assert_eq!(
            test.state.world.get_road_cell(center.0, center.1),
            crate::world::cells::cell_type::EMPTY
        );
        assert!(!test.state.consumable_packs.contains_key(&center));
        assert!(
            test.state
                .query_player_opt(player_id, |ecs, entity| {
                    Some(ecs.get::<PlayerFlags>(entity)?.dirty)
                })
                .unwrap()
        );

        test.state
            .modify_player(player_id, |ecs, entity| {
                ecs.get_mut::<PlayerStats>(entity).unwrap().health = BOOM_DAMAGE;
                ecs.get_mut::<PlayerFlags>(entity).unwrap().dirty = false;
            })
            .unwrap();
        test.state.put_consumable_pack(center.0, center.1, b'B', 0);
        let lethal = apply_boom(
            &test.state,
            BoomDueAction {
                center,
                rng_seed: 8,
            },
        );
        assert_eq!(lethal.deaths, vec![player_id]);
        assert_eq!(lethal.player_health[0].health, 0);
        assert_eq!(lethal.fx, vec![BoomFxEffect::Blast { position: center }]);
        let skill_after_lethal = test.state.query_player_opt(player_id, |ecs, entity| {
            ecs.get::<PlayerSkillsComp>(entity)?
                .states
                .find("l")
                .map(|skill| (skill.level, skill.exp.to_bits()))
        });

        test.state.put_consumable_pack(center.0, center.1, b'B', 0);
        let repeated = apply_boom(
            &test.state,
            BoomDueAction {
                center,
                rng_seed: 9,
            },
        );
        assert!(repeated.player_health.is_empty());
        assert!(repeated.deaths.is_empty());
        assert_eq!(repeated.fx, vec![BoomFxEffect::Blast { position: center }]);
        let skill_after_repeated = test.state.query_player_opt(player_id, |ecs, entity| {
            ecs.get::<PlayerSkillsComp>(entity)?
                .states
                .find("l")
                .map(|skill| (skill.level, skill.exp.to_bits()))
        });
        assert_eq!(skill_after_repeated, skill_after_lethal);
        assert!(crate::test_support::ServerTestHarness::drain_events(&mut receiver).is_empty());
    }

    #[tokio::test]
    async fn red_rock_changes_only_on_the_deterministic_two_percent_roll() {
        let test =
            crate::test_support::ServerTestHarness::new("boom_red_rock", "red-rock-user").await;
        let center = WorldPos(10, 10);
        clear_boom_area(&test.state, center);
        let failed_seed = (0..10_000)
            .find(|&seed| !red_rock_converts(seed, center))
            .unwrap();
        let successful_seed = (0..10_000)
            .find(|&seed| red_rock_converts(seed, center))
            .unwrap();
        test.state.world.set_cell_typed(
            center.0,
            center.1,
            crate::world::CellType(crate::world::cells::cell_type::RED_ROCK),
        );

        let failed = apply_boom(
            &test.state,
            BoomDueAction {
                center,
                rng_seed: failed_seed,
            },
        );
        assert!(failed.changed_cells.is_empty());
        assert_eq!(
            test.state.world.get_cell_typed(center.0, center.1),
            crate::world::CellType(crate::world::cells::cell_type::RED_ROCK)
        );

        let successful = apply_boom(
            &test.state,
            BoomDueAction {
                center,
                rng_seed: successful_seed,
            },
        );
        assert_eq!(successful.changed_cells, vec![center]);
        assert_eq!(
            test.state.world.get_cell_typed(center.0, center.1),
            crate::world::CellType(crate::world::cells::cell_type::ACID_ROCK)
        );
    }

    #[tokio::test]
    async fn protection_blocks_boom_damage_but_pack_is_still_cleared() {
        let test =
            crate::test_support::ServerTestHarness::new("boom_protection", "protected-user").await;
        let mut receiver = test.connect(1);
        crate::test_support::ServerTestHarness::drain_events(&mut receiver);
        let player_id = PlayerId(test.player.id);
        let center = WorldPos(31, 31);
        clear_boom_area(&test.state, center);
        move_player(
            &test.state,
            player_id,
            center,
            Some(Instant::now() + Duration::from_secs(10)),
        );
        test.state.put_consumable_pack(center.0, center.1, b'B', 0);

        let effects = apply_boom(
            &test.state,
            BoomDueAction {
                center,
                rng_seed: 11,
            },
        );

        assert!(effects.player_health.is_empty());
        assert!(effects.deaths.is_empty());
        assert_eq!(effects.fx, vec![BoomFxEffect::Blast { position: center }]);
        assert!(!test.state.consumable_packs.contains_key(&center));
        assert_eq!(
            test.state.query_player_opt(player_id, |ecs, entity| {
                Some(ecs.get::<PlayerStats>(entity)?.health)
            }),
            Some(100)
        );
    }
}
