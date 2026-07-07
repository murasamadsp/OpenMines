use crate::game::buildings::{
    BuildingMetadata, BuildingOwnership, BuildingStats, GridPosition, PackType,
};
use crate::game::player::{
    PlayerConnection, PlayerCooldowns, PlayerFlags, PlayerId, PlayerMetadata, PlayerPosition,
    PlayerSkillsComp as PlayerSkillsCom, PlayerStats,
};
use crate::game::skills::{OnHurt, PlayerSkills as SkillHurt};
use crate::game::{
    BoxIndexResource, BoxPersistQueue, BroadcastEffect, BroadcastQueue, CombatConfigResource,
    PackResendQueue, WorldResource,
};
use crate::world::WorldProvider;
use crate::world::cells::cell_type;
use bevy_ecs::prelude::*;
use num_traits::ToPrimitive;
use std::time::{Duration, Instant};

/// Очередь смерти после `gun_firing_system`: нельзя вызывать `handle_death` изнутри `schedule.run` (вложенный `ecs.write()`).
#[derive(Resource, Default)]
pub struct DeathQueue(pub Vec<PlayerId>);

struct HazardProfile {
    started_at: Instant,
    players_scanned: usize,
    active_cells: usize,
    fall_damage_hits: usize,
    boxes_seen: usize,
    boxes_taken: usize,
    destructible_cells: usize,
    lookup_time: Duration,
    fall_damage_time: Duration,
    box_time: Duration,
    destroy_time: Duration,
}

impl HazardProfile {
    fn start() -> Self {
        Self {
            started_at: Instant::now(),
            players_scanned: 0,
            active_cells: 0,
            fall_damage_hits: 0,
            boxes_seen: 0,
            boxes_taken: 0,
            destructible_cells: 0,
            lookup_time: Duration::ZERO,
            fall_damage_time: Duration::ZERO,
            box_time: Duration::ZERO,
            destroy_time: Duration::ZERO,
        }
    }

    fn log_if_slow(&self) {
        let total = self.started_at.elapsed();
        if total <= Duration::from_millis(5) {
            return;
        }
        tracing::warn!(
            target: "tickprof",
            players_scanned = self.players_scanned,
            active_cells = self.active_cells,
            fall_damage_hits = self.fall_damage_hits,
            boxes_seen = self.boxes_seen,
            boxes_taken = self.boxes_taken,
            destructible_cells = self.destructible_cells,
            lookup_time = ?self.lookup_time,
            fall_damage_time = ?self.fall_damage_time,
            box_time = ?self.box_time,
            destroy_time = ?self.destroy_time,
            total = ?total,
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

/// `Player.Update`: стоя на непустой клетке — `Hurt(fall_damage)`; далее ящик 90 / `is_destructible` (как `Player.cs` при `!isEmpty`).
#[allow(clippy::needless_pass_by_value)]
pub fn standing_cell_hazard_system(
    world_res: Res<WorldResource>,
    box_index: Res<BoxIndexResource>,
    box_persist: Res<BoxPersistQueue>,
    mut death_q: ResMut<DeathQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut q: Query<(
        &PlayerMetadata,
        &PlayerPosition,
        &mut PlayerStats,
        &PlayerConnection,
        &mut PlayerFlags,
        &mut PlayerSkillsCom,
        &mut PlayerCooldowns,
    )>,
) {
    let mut profile = HazardProfile::start();
    let world = &world_res.0;
    let cell_defs = world.cell_defs();

    for (p_meta, pos, mut stats, conn, mut flags, mut skills, mut cooldowns) in &mut q {
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
                conn.send_or_log(crate::net::session::wire::make_u_packet_bytes(sk.0, &sk.1));
            }

            if stats.health > fd {
                stats.health -= fd;
            } else {
                stats.health = 0;
                death_q.0.push(p_meta.id);
            }
            flags.dirty = true;
            conn.send_or_log(crate::net::session::wire::make_u_packet_bytes(
                "@L",
                &crate::protocol::packets::health(stats.health, stats.max_health).1,
            ));
            // C# ref: Player.Hurt → SendDFToBots(6, 0, 0, id, 0) when not lethal
            if stats.health > 0 {
                let fx = crate::protocol::packets::hb_directed_fx(
                    crate::net::session::util::net_u16_nonneg(p_meta.id),
                    0,
                    0,
                    6,
                    0,
                    0,
                );
                let (cx, cy) = crate::world::World::chunk_pos(px, py);
                bcast_q.0.push(BroadcastEffect::Nearby {
                    cx,
                    cy,
                    data: crate::net::session::wire::encode_hb_bundle(
                        &crate::protocol::packets::hb_bundle(&[fx]).1,
                    ),
                    exclude: Some(p_meta.id),
                });
            }
            profile.fall_damage_time += fall_t0.elapsed();
        }

        if cell == crate::world::CellType(cell_type::BOX) {
            profile.boxes_seen += 1;
            let box_t0 = Instant::now();
            // C-1 фикс: in-memory `box_take` вместо sync SQLite под `ecs.write()`
            // (get_box_at/delete_box_at тут фризили весь сервер каждые 10ms при
            // игроке на BOX). Поведение 1:1 (`PEntity.GetBox`: всегда удаляет,
            // кристаллы могут быть 0); персистенция отложена (box_persist_q).
            if let Some(crys) = box_index.0.remove(&(px, py).into()).map(|(_, v)| v) {
                profile.boxes_taken += 1;
                box_persist.0.lock().push(((px, py).into(), None));
                for (i, &c) in crys.iter().enumerate() {
                    stats.crystals[i] = stats.crystals[i].saturating_add(c);
                }
                flags.dirty = true;
                conn.send_or_log(crate::net::session::wire::make_u_packet_bytes(
                    "@B",
                    &crate::protocol::packets::basket(&stats.crystals, 1).1,
                ));
            }
            let _ = world.damage_cell(px, py, 1.0);
            bcast_q.0.push(BroadcastEffect::CellUpdate((px, py).into()));
            profile.box_time += box_t0.elapsed();
        } else if pdef.physical.is_destructible {
            profile.destructible_cells += 1;
            let destroy_t0 = Instant::now();
            world.destroy(px, py);
            bcast_q.0.push(BroadcastEffect::CellUpdate((px, py).into()));
            profile.destroy_time += destroy_t0.elapsed();
        }
    }
    profile.log_if_slow();
}

/// Таймер залпа пушек. C# зовёт `gun.Update()` каждые 0.5с (`World.Update`
/// `lastpackeffect >= FromSeconds(0.5)`), НЕ каждый тик. Без троттла пушка била
/// бы 60 HP каждые 10мс = мгновенная смерть.
#[derive(Resource)]
pub struct GunTickTimer(pub std::time::Instant);

impl Default for GunTickTimer {
    fn default() -> Self {
        Self(std::time::Instant::now())
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
    #[allow(clippy::cast_possible_truncation)]
    (sk.on_hurt(
        base_damage
            .to_f32()
            .expect("validated positive gun_damage must fit f32"),
    )
    .round() as i32)
        .max(0)
}

/// Радиус 20 (см. `Vector2.Distance(…) <= 20` в `Gun.cs`), 60 HP, `DamageType.Gun` → `AntiGun`.
#[allow(clippy::needless_pass_by_value, clippy::type_complexity)]
pub fn gun_firing_system(
    combat_cfg: Res<CombatConfigResource>,
    mut death_q: ResMut<DeathQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut pack_resend_q: ResMut<PackResendQueue>,
    mut fire_timer: ResMut<GunTickTimer>,
    mut guns_query: Query<(
        &BuildingMetadata,
        &mut BuildingStats,
        &BuildingOwnership,
        &GridPosition,
    )>,
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
    let now = std::time::Instant::now();
    let combat = combat_cfg.0;
    // Залп раз в `gameplay.combat.gun_fire_interval_ms`
    // (default 500ms = 1:1 C# `lastpackeffect`), а не каждый тик.
    if fire_timer.0.elapsed().as_millis() < u128::from(combat.gun_fire_interval_ms) {
        return;
    }
    fire_timer.0 = now;

    for (meta, mut b_stats, b_ownership, b_pos) in &mut guns_query {
        if meta.pack_type != PackType::Gun || b_stats.charge <= 0 {
            continue;
        }

        // 1:1 C# `Gun.Update`: бьёт КАЖДОГО игрока в радиусе 20 (foreach), а не
        // одного. Charge списывается per-hit; C# НЕ прерывает цикл при обнулении
        // charge (оставшиеся жертвы всё равно получают урон в этот тик) — top-guard
        // `charge <= 0` лишь пропускает пушку на СЛЕДУЮЩЕМ тике.
        for (_p_entity, p_meta, p_pos, mut p_sk, mut stats, p_cd, conn, mut flags) in
            &mut players_query
        {
            // clan immunity (C# `player.cid == cid` → continue). cid у безкланового
            // = 0; нормализуем Option→0, иначе `None == Some(0)` ложно и пушка
            // без клана била бесклановых игроков (баг: «бьют, я у них во врагах»).
            if stats.clan_id.unwrap_or(0) == b_ownership.clan_id {
                continue;
            }
            // Protector-скип. ВНИМАНИЕ: в C# `Player.Hurt`/`Gun.Update` проверки
            // неуязвимости НЕТ — это добавление Rust (protector-механика идёт иным
            // путём). См. `docs/CLIENT_PROTOCOL_GAPS.md`.
            if p_cd.protection_until.is_some_and(|u| now < u) {
                continue;
            }

            if !inside_gun_radius(p_pos, b_pos, combat.gun_radius_cells) {
                continue;
            }

            // C# Player.Hurt(60, DamageType.Gun): skill exp before damage
            let mut changed = false;
            changed |= crate::game::skills::add_skill_exp(&mut p_sk.states, "l", 1.0); // Health
            changed |= crate::game::skills::add_skill_exp(&mut p_sk.states, "*I", 1.0); // Induction
            changed |= crate::game::skills::add_skill_exp(&mut p_sk.states, "u", 1.0); // AntiGun
            // Always send @S after skill exp IF changed (C# Skill.AddExp always sends if pct changed)
            if changed {
                let sk_pkt = crate::protocol::packets::skills_packet(
                    &crate::game::skills::skill_progress_payload(&p_sk.states),
                );
                conn.send_or_log(crate::net::session::wire::make_u_packet_bytes(
                    sk_pkt.0, &sk_pkt.1,
                ));
            }
            // C# Gun.Update order: damage first, then deduct charge
            let dmg = gun_damage_after_skills(combat.gun_damage, &p_sk.states);
            if dmg > 0 {
                if stats.health > dmg {
                    stats.health -= dmg;
                    flags.dirty = true;
                } else {
                    stats.health = 0;
                    flags.dirty = true;
                    death_q.0.push(p_meta.id);
                }
            }
            conn.send_or_log(crate::net::session::wire::make_u_packet_bytes(
                "@L",
                &crate::protocol::packets::health(stats.health, stats.max_health).1,
            ));

            // Charge cost: дробная часть не хранится в BuildingStats, а применяется
            // как шанс списать ещё 1 единицу.
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
                } else if b_stats.charge > 0 {
                    // Charge just depleted — broadcast HB O update (C# `ResendPack`)
                    b_stats.charge = 0;
                    pack_resend_q.0.push((b_pos.x, b_pos.y));
                }
            }

            // C# `Gun.Update`: `player.SendDFToBots(7, gun.x, gun.y, player.id, 1)` —
            // directed gun-shot FX (`D`-тег), бродкаст вокруг ЖЕРТВЫ
            // (`vChunksAroundEx` игрока). Клиент `case 7 → AddGunShot(x, y, bid)`
            // рисует выстрел пушка(x,y)→жертва(bid). Раньше слался `hb_fx` (`F`-тег,
            // fx=1) у пушки → клиент `AddFX case 1: break` → выстрел был НЕВИДИМ.
            let fx = crate::protocol::packets::hb_directed_fx(
                crate::net::session::util::net_u16_nonneg(p_meta.id),
                u16::try_from(b_pos.x.rem_euclid(65536)).unwrap_or(0),
                u16::try_from(b_pos.y.rem_euclid(65536)).unwrap_or(0),
                7,
                1,
                0,
            );
            let data = crate::net::session::wire::encode_hb_bundle(
                &crate::protocol::packets::hb_bundle(&[fx]).1,
            );
            let (cx, cy) = crate::world::World::chunk_pos(p_pos.x, p_pos.y);
            bcast_q.0.push(BroadcastEffect::Nearby {
                cx,
                cy,
                data,
                exclude: None,
            });
        }
    }
}
