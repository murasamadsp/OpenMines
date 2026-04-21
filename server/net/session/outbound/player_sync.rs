#![allow(dead_code, unused_imports)]

use crate::game::player::{PlayerSkills as PlayerSkillsComponent, PlayerStats};
use crate::game::skills::{
    OnHealth, OnMove, OnPackCrys, PlayerSkills as PlayerSkillsHelper, SkillType,
    skill_progress_payload,
};
use crate::net::session::prelude::*;

pub fn send_player_speed(tx: &mpsc::UnboundedSender<Vec<u8>>, skills: &PlayerSkillsComponent) {
    // 1:1 ref (`pSenders.SendSpeed`):
    // `new SpeedPacket((int)(p.pause * 5 * 1.4 / 1000 * 1.7),
    //                 (int)(p.pause * 0.80 * 5 * 1.4 / 1000 * 1.7),
    //                 100000)`
    // where `p.pause = (int)(Movement.Effect * 100)`.
    let move_effect = get_player_skill_effect(&skills.states, SkillType::Movement);
    // D24: C# does `int pause = (int)(Movement.Effect * 100)` — truncate to int first.
    #[allow(clippy::cast_possible_truncation)]
    let pause = f64::from((move_effect * 100.0) as i32);
    let xy_pause = (pause * 5.0 * 1.4 / 1000.0 * 1.7) as i32;
    let road_pause = (pause * 0.80 * 5.0 * 1.4 / 1000.0 * 1.7) as i32;
    send_u_packet(tx, "sp", &speed(xy_pause, road_pause, 100000).1);
}

pub fn send_player_health(tx: &mpsc::UnboundedSender<Vec<u8>>, stats: &PlayerStats) {
    send_u_packet(tx, "@L", &health(stats.health, stats.max_health).1);
}

pub fn send_player_level(tx: &mpsc::UnboundedSender<Vec<u8>>, skills: &PlayerSkillsComponent) {
    let total_lvl: i32 = skills.states.values().map(|s| s.level).sum();
    send_u_packet(tx, "LV", &level(total_lvl).1);
}

pub fn send_player_skills(tx: &mpsc::UnboundedSender<Vec<u8>>, skills: &PlayerSkillsComponent) {
    let payload = skill_progress_payload(&skills.states);
    let sk = skills_packet(&payload);
    send_u_packet(tx, sk.0, &sk.1);
}

pub fn send_player_basket(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    stats: &PlayerStats,
    skills: &PlayerSkillsComponent,
) {
    let sk = PlayerSkillsHelper {
        skills: &skills.states,
    };
    let capacity = sk.on_pack_crys_capacity(1000); // 1000 is base
    send_u_packet(tx, "@B", &basket(&stats.crystals, capacity).1);
}

pub fn send_all_skill_updates(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    stats: &PlayerStats,
    skills: &PlayerSkillsComponent,
) {
    send_player_level(tx, skills);
    send_player_skills(tx, skills);
    send_player_speed(tx, skills);
    send_player_health(tx, stats);
    send_player_basket(tx, stats, skills);
}
