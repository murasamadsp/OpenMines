use crate::game::buildings::{
    BuildingDeletePending, BuildingMetadata, BuildingOwnership, BuildingStats, GridPosition,
    PackType,
};
use crate::game::logic::numeric::saturating_trunc_f32_to_i32;
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerId, PlayerMetadata, PlayerPosition,
    PlayerSkillsComp as PlayerSkillsCom, PlayerStats,
};
use crate::game::programmator::ProgrammatorState;
use crate::game::skills::{OnHurt, PlayerSkills as SkillHurt};
use crate::game::{
    BoxPickupIntent, BoxPickupQueue, BoxPickupSource, BroadcastEffect, BroadcastQueue,
    CombatConfigResource, PackResendQueue, ScheduleConfigResource, WorldResource,
};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;
use num_traits::ToPrimitive;
use parking_lot::Mutex;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Default)]
struct DeathQueueState {
    queue: VecDeque<PlayerId>,
    players: HashSet<PlayerId>,
}

#[derive(Resource, Clone, Default)]
pub struct DeathQueue(Arc<Mutex<DeathQueueState>>);

impl DeathQueue {
    pub fn push(&self, player_id: PlayerId) {
        let mut state = self.0.lock();
        if state.players.insert(player_id) {
            state.queue.push_back(player_id);
        }
    }

    pub fn drain(&self) -> Vec<PlayerId> {
        let mut state = self.0.lock();
        state.players.clear();
        state.queue.drain(..).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.lock().queue.is_empty()
    }
}

struct HazardProfile {
    started_at: Instant,
    players_scanned: usize,
    active_cells: usize,
    fall_damage_hits: usize,
    boxes_seen: usize,
    destructible_cells: usize,
    lookup_time: Duration,
    fall_damage_time: Duration,
    box_time: Duration,
    destroy_time: Duration,
}

type HazardQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        &'static PlayerMetadata,
        &'static PlayerPosition,
        &'static mut PlayerStats,
        Option<&'static PlayerConnection>,
        Option<&'static ProgrammatorState>,
        &'static mut PlayerFlags,
        &'static mut PlayerSkillsCom,
        &'static mut PlayerCooldowns,
    ),
>;

impl HazardProfile {
    fn start() -> Self {
        Self {
            started_at: Instant::now(),
            players_scanned: 0,
            active_cells: 0,
            fall_damage_hits: 0,
            boxes_seen: 0,
            destructible_cells: 0,
            lookup_time: Duration::ZERO,
            fall_damage_time: Duration::ZERO,
            box_time: Duration::ZERO,
            destroy_time: Duration::ZERO,
        }
    }

    fn log_if_slow(&self, threshold: Duration) {
        let total = self.started_at.elapsed();
        if total <= threshold {
            return;
        }
        let accounted =
            self.lookup_time + self.fall_damage_time + self.box_time + self.destroy_time;
        let unaccounted = total.saturating_sub(accounted);
        tracing::warn!(
            target: "tickprof",
            players_scanned = self.players_scanned,
            active_cells = self.active_cells,
            fall_damage_hits = self.fall_damage_hits,
            boxes_seen = self.boxes_seen,
            destructible_cells = self.destructible_cells,
            lookup_time = ?self.lookup_time,
            fall_damage_time = ?self.fall_damage_time,
            box_time = ?self.box_time,
            destroy_time = ?self.destroy_time,
            accounted_time = ?accounted,
            unaccounted_time = ?unaccounted,
            total = ?total,
            threshold = ?threshold,
            "SLOW hazards system"
        );
    }
}

fn reset_c190_if_due(cooldowns: &mut PlayerCooldowns) {
    if cooldowns
        .last_c190_hit
        .is_some_and(|t| t.elapsed() >= Duration::from_mins(1))
    {
        cooldowns.c190_stacks = 1;
        cooldowns.last_c190_hit = Some(Instant::now());
    }
}

fn send_direct(bcast_q: &mut BroadcastQueue, conn: &PlayerConnection, data: Vec<u8>) {
    bcast_q.0.push(BroadcastEffect::Direct {
        session_id: conn.session_id,
        data,
    });
}

/// `Player.Update`: стоя на непустой клетке — `Hurt(fall_damage)`; далее ящик 90 / `is_destructible` (как `Player.cs` при `!isEmpty`).
#[allow(clippy::needless_pass_by_value)]
pub fn standing_cell_hazard_system(
    world_res: Res<WorldResource>,
    schedule_cfg: Res<ScheduleConfigResource>,
    box_pickups: Res<BoxPickupQueue>,
    death_q: Res<DeathQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut dirty_players: ResMut<crate::game::DirtyPlayers>,
    mut q: HazardQuery<'_, '_>,
) {
    let mut profile = HazardProfile::start();
    let world = &world_res.0;
    let cell_defs = world.cell_defs();

    for (entity, p_meta, pos, mut stats, conn, prog, mut flags, mut skills, mut cooldowns) in &mut q
    {
        if conn.is_none() && !prog.is_some_and(|prog| prog.running) {
            continue;
        }
        if stats.health <= 0 {
            continue;
        }
        profile.players_scanned += 1;
        // C# ref Player.Update: reset c190stacks to 1 after 1 minute
        reset_c190_if_due(&mut cooldowns);
        let (px, py) = (pos.x, pos.y);
        if !world.valid_coord(px, py) {
            continue;
        }
        let lookup_t0 = Instant::now();
        let cell = world.get_cell_typed(px, py);
        let pdef = cell_defs.get_typed(cell);
        profile.lookup_time += lookup_t0.elapsed();
        if pdef.cell_is_empty() {
            continue;
        }
        profile.active_cells += 1;
        if pdef.fall_damage > 0 {
            profile.fall_damage_hits += 1;
            let fall_t0 = Instant::now();
            let fd = pdef.fall_damage;
            // C# ref: Player.Hurt → Health.AddExp (on every hurt)
            if crate::game::skills::add_skill_exp(&mut skills.states, "l", 1.0) {
                let sk = crate::protocol::packets::skills_packet(
                    &crate::game::skills::skill_progress_payload(&skills.states),
                );
                if let Some(conn) = conn {
                    send_direct(
                        &mut bcast_q,
                        conn,
                        crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1),
                    );
                }
            }

            if stats.health > fd {
                stats.health -= fd;
            } else {
                stats.health = 0;
                death_q.push(p_meta.id);
            }
            flags.dirty = true;
            dirty_players.0.insert((entity, flags.incarnation));
            if let Some(conn) = conn {
                send_direct(
                    &mut bcast_q,
                    conn,
                    crate::net::session::wire::make_u_packet_bytes(
                        "@L",
                        &crate::protocol::packets::health(stats.health, stats.max_health).1,
                    ),
                );
            }
            // C# ref: Player.Hurt → SendDFToBots(6, 0, 0, id, 0) when not lethal
            if stats.health > 0 {
                bcast_q
                    .0
                    .push(hurt_fx_broadcast(p_meta.id, px, py, Some(p_meta.id)));
            }
            profile.fall_damage_time += fall_t0.elapsed();
        }

        if cell == crate::world::CellType(cell_type::BOX) {
            profile.boxes_seen += 1;
            let box_t0 = Instant::now();
            box_pickups.push(BoxPickupIntent {
                player_id: p_meta.id,
                player_pos: (px, py).into(),
                box_pos: (px, py).into(),
                source: BoxPickupSource::Standing,
            });
            profile.box_time += box_t0.elapsed();
        } else if pdef.physical.is_destructible {
            profile.destructible_cells += 1;
            let destroy_t0 = Instant::now();
            world.destroy(px, py);
            bcast_q.0.push(BroadcastEffect::CellUpdate((px, py).into()));
            profile.destroy_time += destroy_t0.elapsed();
        }
    }
    profile.log_if_slow(
        Duration::from_millis(schedule_cfg.0.schedule_warn_threshold_ms)
            .min(Duration::from_millis(schedule_cfg.0.game_loop_tick_rate_ms)),
    );
}

/// Таймер залпа пушек. C# зовёт `gun.Update()` каждые 0.5с (`World.Update`
/// `lastpackeffect >= FromSeconds(0.5)`), НЕ каждый тик. Без троттла пушка била
/// бы 60 HP каждые 10мс = мгновенная смерть.
#[derive(Resource)]
pub struct GunTickTimer(pub std::time::Instant);

#[derive(Resource, Default)]
pub struct GunCandidateBatch {
    pub guns: Vec<Entity>,
    pub players: Vec<Entity>,
}

impl Default for GunTickTimer {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}

impl GunTickTimer {
    pub(crate) fn is_due_at(&self, now: Instant, interval: Duration) -> bool {
        now.saturating_duration_since(self.0) >= interval
    }

    const fn mark_completed_at(&mut self, completed_at: Instant) {
        self.0 = completed_at;
    }
}

fn inside_gun_radius(player: &PlayerPosition, gun: &GridPosition, radius_cells: i32) -> bool {
    let dx = i64::from(player.x) - i64::from(gun.x);
    let dy = i64::from(player.y) - i64::from(gun.y);
    let radius = i64::from(radius_cells);
    dx * dx + dy * dy <= radius * radius
}

fn gun_damage_after_skills(base_damage: i32, skills: &crate::db::SkillSlots) -> i32 {
    let sk = SkillHurt { skills };
    // 1:1 ref default Gun damage = 60 * (1 - AntiGun/100); AntiGun effect — f32
    // из get_player_skill_effect (1:1 с C#, нельзя переводить в int без
    // потери паритета). Округлённый каст намеренный и ограничен снизу.
    saturating_trunc_f32_to_i32(
        sk.on_hurt(
            base_damage
                .to_f32()
                .expect("validated positive gun_damage must fit f32"),
        )
        .round(),
    )
    .max(0)
}

fn hurt_fx_broadcast(
    player_id: PlayerId,
    x: i32,
    y: i32,
    exclude: Option<PlayerId>,
) -> BroadcastEffect {
    let fx =
        crate::protocol::packets::hb_hurt_fx(crate::net::session::util::net_u16_nonneg(player_id));
    let (cx, cy) = crate::world::World::chunk_pos(x, y);
    BroadcastEffect::Nearby {
        cx,
        cy,
        data: crate::net::session::wire::encode_hb_bundle(
            &crate::protocol::packets::hb_bundle(&[fx]).1,
        ),
        exclude,
    }
}

fn is_same_gun_clan(player_clan_id: Option<i32>, gun_clan_id: i32) -> bool {
    player_clan_id.unwrap_or(0) == gun_clan_id
}

fn send_gun_skill_progress(
    queue: &mut BroadcastQueue,
    connection: &PlayerConnection,
    skills: &PlayerSkillsCom,
) {
    let packet = crate::protocol::packets::skills_packet(
        &crate::game::skills::skill_progress_payload(&skills.states),
    );
    send_direct(
        queue,
        connection,
        crate::net::session::wire::make_u_packet_bytes(packet.0, &packet.1),
    );
}

fn gun_shot_effect(
    player: &PlayerMetadata,
    position: &PlayerPosition,
    gun: &GridPosition,
) -> BroadcastEffect {
    let fx = crate::protocol::packets::hb_gun_shot_fx(
        crate::net::session::util::net_u16_nonneg(player.id),
        u16::try_from(gun.x.rem_euclid(65536)).unwrap_or(0),
        u16::try_from(gun.y.rem_euclid(65536)).unwrap_or(0),
    );
    let (cx, cy) = crate::world::World::chunk_pos(position.x, position.y);
    BroadcastEffect::Nearby {
        cx,
        cy,
        data: crate::net::session::wire::encode_hb_bundle(
            &crate::protocol::packets::hb_bundle(&[fx]).1,
        ),
        exclude: None,
    }
}

/// Радиус 20 (см. `Vector2.Distance(…) <= 20` в `Gun.cs`), 60 HP, `DamageType.Gun` → `AntiGun`.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub fn gun_firing_system(
    combat_cfg: Res<CombatConfigResource>,
    death_q: Res<DeathQueue>,
    output: (
        ResMut<BroadcastQueue>,
        ResMut<crate::game::DirtyPlayers>,
        ResMut<GunCandidateBatch>,
        ResMut<crate::game::DirtyBuildings>,
    ),
    mut pack_resend_q: ResMut<PackResendQueue>,
    mut fire_timer: ResMut<GunTickTimer>,
    mut guns_query: Query<
        (
            &BuildingMetadata,
            &mut BuildingStats,
            &BuildingOwnership,
            &GridPosition,
        ),
        Without<BuildingDeletePending>,
    >,
    mut players_query: Query<(
        Entity,
        &PlayerMetadata,
        &PlayerPosition,
        &mut PlayerSkillsCom,
        &mut PlayerStats,
        &PlayerCooldowns,
        &PlayerConnection,
        &mut PlayerFlags,
    )>,
) {
    let (mut bcast_q, mut dirty_players, mut candidates, mut dirty_buildings) = output;
    let now = std::time::Instant::now();
    let combat = combat_cfg.0;
    let fire_interval = Duration::from_millis(combat.gun_fire_interval_ms);
    if !fire_timer.is_due_at(now, fire_interval) {
        return;
    }
    let GunCandidateBatch { guns, players } = std::mem::take(&mut *candidates);
    for gun_entity in guns {
        let Ok((meta, mut b_stats, b_ownership, b_pos)) = guns_query.get_mut(gun_entity) else {
            continue;
        };
        if meta.pack_type != PackType::Gun || b_stats.charge <= 0 {
            continue;
        }
        for player_entity in &players {
            let Ok((player_entity, p_meta, p_pos, mut p_sk, mut stats, p_cd, conn, mut flags)) =
                players_query.get_mut(*player_entity)
            else {
                continue;
            };
            if stats.health <= 0 {
                continue;
            }
            // C# clan immunity, including clanless `0`.
            if is_same_gun_clan(stats.clan_id, b_ownership.clan_id) {
                continue;
            }
            // Rust protector guard; C# has no equivalent.
            if p_cd.protection_until.is_some_and(|u| now < u) {
                continue;
            }
            if !inside_gun_radius(p_pos, b_pos, combat.gun_radius_cells) {
                continue;
            }
            let mut changed = false;
            changed |= crate::game::skills::add_skill_exp(&mut p_sk.states, "l", 1.0); // Health
            changed |= crate::game::skills::add_skill_exp(&mut p_sk.states, "*I", 1.0); // Induction
            changed |= crate::game::skills::add_skill_exp(&mut p_sk.states, "u", 1.0); // AntiGun
            if changed {
                send_gun_skill_progress(&mut bcast_q, conn, &p_sk);
            }
            let dmg = gun_damage_after_skills(combat.gun_damage, &p_sk.states);
            if dmg > 0 {
                if stats.health > dmg {
                    stats.health -= dmg;
                    flags.dirty = true;
                    dirty_players.0.insert((player_entity, flags.incarnation));
                    bcast_q
                        .0
                        .push(hurt_fx_broadcast(p_meta.id, p_pos.x, p_pos.y, None));
                } else {
                    stats.health = 0;
                    flags.dirty = true;
                    dirty_players.0.insert((player_entity, flags.incarnation));
                    death_q.push(p_meta.id);
                }
            }
            send_direct(
                &mut bcast_q,
                conn,
                crate::net::session::wire::make_u_packet_bytes(
                    "@L",
                    &crate::protocol::packets::health(stats.health, stats.max_health).1,
                ),
            );
            let induction_effect = crate::game::skills::get_player_skill_effect(
                &p_sk.states,
                crate::game::skills::SkillType::Induction,
            );
            let charge_cost = if induction_effect > 0.0 {
                0.5 * (induction_effect / 100.0)
            } else {
                0.5
            };
            let charge_cost = crate::game::mechanics::random::probabilistic_i32(charge_cost);
            if charge_cost > 0 {
                if b_stats.charge > charge_cost {
                    b_stats.charge -= charge_cost;
                    dirty_buildings.0.insert(gun_entity);
                } else if b_stats.charge > 0 {
                    // Charge just depleted — broadcast HB O update (C# `ResendPack`)
                    b_stats.charge = 0;
                    dirty_buildings.0.insert(gun_entity);
                    pack_resend_q.0.push((b_pos.x, b_pos.y));
                }
            }
            bcast_q.0.push(gun_shot_effect(p_meta, p_pos, b_pos));
        }
    }

    // C# `World.Update` обновляет `lastpackeffect` после полного обхода паков.
    // Отсчёт от начала залпа сжимал интервалы после долгого ECS-прохода.
    fire_timer.mark_completed_at(Instant::now());
}

#[cfg(test)]
mod tests {
    use super::{GunTickTimer, gun_damage_after_skills, hurt_fx_broadcast, is_same_gun_clan};
    use crate::db::{SkillEntry, SkillSlots};
    use crate::game::BroadcastEffect;
    use crate::game::player::PlayerId;
    use crate::game::skills::SkillType;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    fn apply_test_gun_hit(
        timer: &mut GunTickTimer,
        started_at: Instant,
        completed_at: Instant,
        interval: Duration,
        health: &mut i32,
    ) -> bool {
        if !timer.is_due_at(started_at, interval) {
            return false;
        }
        *health = health.saturating_sub(60);
        timer.mark_completed_at(completed_at);
        true
    }

    #[test]
    fn gun_cadence_waits_500ms_and_does_not_burst_after_time_jump() {
        let base = Instant::now();
        let interval = Duration::from_millis(500);
        let mut timer = GunTickTimer(base);
        let mut health = 300;

        assert!(!apply_test_gun_hit(
            &mut timer,
            base + Duration::from_millis(499),
            base + Duration::from_millis(499),
            interval,
            &mut health,
        ));
        assert_eq!(health, 300, "damage before the 500ms deadline");

        let first_deadline = base + interval;
        let slow_fire_completed_at = base + Duration::from_millis(1_200);
        assert!(apply_test_gun_hit(
            &mut timer,
            first_deadline,
            slow_fire_completed_at,
            interval,
            &mut health,
        ));
        assert_eq!(health, 240, "deadline must apply exactly one hit");
        assert!(!apply_test_gun_hit(
            &mut timer,
            slow_fire_completed_at,
            slow_fire_completed_at,
            interval,
            &mut health,
        ));
        assert_eq!(
            health, 240,
            "slow fire must not make another hit immediately due"
        );
        assert!(!apply_test_gun_hit(
            &mut timer,
            base + Duration::from_millis(1_699),
            base + Duration::from_millis(1_699),
            interval,
            &mut health,
        ));
        assert!(apply_test_gun_hit(
            &mut timer,
            base + Duration::from_millis(1_700),
            base + Duration::from_millis(1_700),
            interval,
            &mut health,
        ));
        assert_eq!(health, 180, "next hit must wait from completion time");

        let after_stall = base + Duration::from_secs(10);
        let stalled_fire_completed_at = base + Duration::from_secs(12);
        assert!(apply_test_gun_hit(
            &mut timer,
            after_stall,
            stalled_fire_completed_at,
            interval,
            &mut health,
        ));
        assert!(!apply_test_gun_hit(
            &mut timer,
            stalled_fire_completed_at,
            stalled_fire_completed_at,
            interval,
            &mut health,
        ));
        assert_eq!(health, 120, "missed intervals must not replay as a burst");
    }

    #[test]
    fn gun_damage_truncates_antigun_reduction_like_reference() {
        let skills = SkillSlots {
            skills: HashMap::from([(
                0,
                SkillEntry {
                    code: SkillType::AntiGun.code().to_owned(),
                    level: 2,
                    exp: 0.0,
                },
            )]),
            total_slots: 20,
        };

        assert_eq!(gun_damage_after_skills(60, &skills), 59);
    }

    #[test]
    fn hurt_fx_broadcast_uses_df6_payload() {
        let player_id = PlayerId(9);
        let effect = hurt_fx_broadcast(player_id, 40, 70, None);
        let wire_effect = crate::protocol::packets::hb_hurt_fx(9);
        let expected_data = crate::net::session::wire::encode_hb_bundle(
            &crate::protocol::packets::hb_bundle(&[wire_effect]).1,
        );
        let (chunk_x, chunk_y) = crate::world::World::chunk_pos(40, 70);

        match effect {
            BroadcastEffect::Nearby {
                cx,
                cy,
                data,
                exclude,
            } => {
                assert_eq!((cx, cy), (chunk_x, chunk_y));
                assert_eq!(data, expected_data);
                assert_eq!(exclude, None);
            }
            BroadcastEffect::CellUpdate(_)
            | BroadcastEffect::BlockUpdate(_)
            | BroadcastEffect::Direct { .. } => {
                panic!("expected nearby hurt FX broadcast");
            }
        }
    }

    #[test]
    fn gun_clan_immunity_treats_clanless_as_zero_like_reference() {
        assert!(is_same_gun_clan(None, 0));
        assert!(is_same_gun_clan(Some(7), 7));
        assert!(!is_same_gun_clan(None, 7));
        assert!(!is_same_gun_clan(Some(7), 0));
    }
}
