#![allow(dead_code, unused_imports)]

use crate::game::skills::{
    OnHealth, OnMove, OnPackCrys, PlayerSkills, SkillType, skill_progress_payload,
};
use crate::net::session::prelude::*;

pub fn send_player_speed(tx: &mpsc::UnboundedSender<Vec<u8>>, skills: &crate::db::PlayerRow) {
    let sk = PlayerSkills {
        skills: &skills.skills,
    };
    let xy_pause = sk.on_move(ROBOT_XY_PAUSE_MS as f32) as i32;
    let road_pause = sk.on_move_road(ROBOT_ROAD_PAUSE_MS as f32) as i32;
    send_u_packet(tx, "sp", &speed(xy_pause, road_pause, 5000).1);
}

pub fn send_player_health(tx: &mpsc::UnboundedSender<Vec<u8>>, player: &crate::db::PlayerRow) {
    send_u_packet(tx, "@L", &health(player.health, player.max_health).1);
}

pub fn send_player_level(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    skills: &std::collections::HashMap<String, crate::db::SkillState>,
) {
    let total_lvl: i32 = skills.values().map(|s| s.level).sum();
    send_u_packet(tx, "LV", &level(total_lvl).1);
}

pub fn send_player_skills(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    skills: &std::collections::HashMap<String, crate::db::SkillState>,
) {
    let payload = skill_progress_payload(skills);
    if !payload.is_empty() {
        send_u_packet(tx, "SK", &skills_packet(&payload).1);
    }
}

pub fn send_player_basket(tx: &mpsc::UnboundedSender<Vec<u8>>, player: &crate::db::PlayerRow) {
    let sk = PlayerSkills {
        skills: &player.skills,
    };
    let capacity = sk.on_pack_crys_capacity(1000); // 1000 is base
    send_u_packet(tx, "@B", &basket(&player.crystals, capacity).1);
}

pub fn send_all_skill_updates(tx: &mpsc::UnboundedSender<Vec<u8>>, player: &crate::db::PlayerRow) {
    send_player_level(tx, &player.skills);
    send_player_skills(tx, &player.skills);
    send_player_speed(tx, player);
    send_player_health(tx, player);
    send_player_basket(tx, player);
}
