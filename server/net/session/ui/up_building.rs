//! Up building (PackType::Up) — skill management GUI.
//!
//! 1:1 with C# `Buildings/Up.cs` + `GUI/UP/UpPage.cs` + `PlayerSkills.cs`.
//!
//! Wire format: `"up:{json}"` (distinct from `"horb:{json}"`).
//! JSON fields: `k`, `s`, `b`, `ba`, `i`, `del`, `sl`, `si`, `txt`, `buttons`, `back`.
//!
//! Button actions from client:
//! - `skill:{slot}` — select a slot
//! - `upgrade` — upgrade selected skill
//! - `delete:{slot}` — delete skill from slot
//! - `install:{code}#{slot}` — install a new skill
//! - `buyslot` — purchase an additional slot
//! - `exit` / `exit:0` — close window (handled upstream)

use crate::db::SkillState;
use crate::game::player::{PlayerSkills as PlayerSkillsComp, PlayerStats, PlayerUI};
use crate::game::skills::{
    self, OnHealth, PlayerSkills as PlayerSkillsHelper, SkillType, exp_needed,
    get_skill_requirements, skill_effect,
};
use crate::net::session::outbound::player_sync::{
    send_player_level, send_player_skills, send_player_speed,
};
use crate::net::session::prelude::*;
use std::collections::HashMap;

/// Maximum number of skill slots a player can have.
const MAX_SLOTS: i32 = 34;

/// Cost in creds to buy an additional slot.
const SLOT_COST: i64 = 1000;

// ─── Public API ───────��─────────────────────────────���────────────────────────

/// Open the Up building GUI for a player. Called from `open_pack_gui`.
pub fn open_up_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    // Send UpPage with no selected slot (initial state)
    send_up_page(state, tx, pid, -1);

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            ui.current_window = Some(format!("up:{}:{}", view.x, view.y));
        }
        Some(())
    });
}

/// Handle Up building button presses.
/// Returns `true` if the button was handled.
pub fn handle_up_button(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    button: &str,
) -> bool {
    // Check that the player has an Up window open
    let has_up_window = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<PlayerUI>(entity)
                .and_then(|ui| ui.current_window.as_deref())
                .is_some_and(|w| w.starts_with("up:"))
        })
        .unwrap_or(false);

    if !has_up_window {
        return false;
    }

    // Parse button action
    if let Some(rest) = button.strip_prefix("skill:") {
        if let Ok(slot) = rest.parse::<i32>() {
            handle_skill_select(state, tx, pid, slot);
            return true;
        }
    } else if button == "upgrade" {
        handle_skill_upgrade(state, tx, pid);
        return true;
    } else if let Some(rest) = button.strip_prefix("delete:") {
        if let Ok(slot) = rest.parse::<i32>() {
            handle_skill_delete(state, tx, pid, slot);
            return true;
        }
    } else if let Some(rest) = button.strip_prefix("install:") {
        // Format: "install:{code}#{slot}"
        if let Some(hash_pos) = rest.find('#') {
            let code = &rest[..hash_pos];
            if let Ok(slot) = rest[hash_pos + 1..].parse::<i32>() {
                handle_skill_install(state, tx, pid, code, slot);
                return true;
            }
        }
    } else if button == "buyslot" {
        handle_buy_slot(state, tx, pid);
        return true;
    }

    false
}

// ─── Internal handlers ────────────────────���──────────────────────────────────

/// Select a skill slot — re-render the page with the slot selected.
fn handle_skill_select(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    slot: i32,
) {
    send_up_page(state, tx, pid, slot);
}

/// Upgrade the skill in the currently selected slot (increase level by 1).
/// C# ref: `Skill.Up(Player p)` — requires `exp >= Experience`.
fn handle_skill_upgrade(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let selected_slot = get_selected_slot(state, pid);
    if selected_slot < 0 {
        return;
    }

    let upgraded = state
        .modify_player(pid, |ecs, entity| {
            // Read skill code and check upgrade readiness
            let (skill_code, skill_type, needed) = {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                let code = get_skill_code_at_slot(&skills.states, selected_slot)?;
                let stype = SkillType::from_code(&code)?;
                let state_entry = skills.states.get(&code)?;
                let need = exp_needed(stype, state_entry.level);
                if need > 0.0 && state_entry.exp < need {
                    return Some(false);
                }
                (code, stype, need)
            };

            // Perform upgrade (mutable borrow)
            {
                let mut skills_mut = ecs.get_mut::<PlayerSkillsComp>(entity)?;
                let entry = skills_mut.states.get_mut(&skill_code)?;
                if needed > 0.0 {
                    entry.exp -= needed;
                }
                entry.level += 1;
            }

            // Send updated skills progress and compute health changes
            let new_max_health = {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                send_player_skills(tx, skills);
                send_player_level(tx, skills);

                if skill_type.effect_type() == skills::SkillEffectType::OnMove {
                    send_player_speed(tx, skills);
                }

                if skill_type == SkillType::Health {
                    let sk_helper = PlayerSkillsHelper {
                        skills: &skills.states,
                    };
                    Some(sk_helper.on_health_max(100))
                } else {
                    None
                }
            };

            // If health skill, update max health (separate mutable borrow)
            if let Some(new_max) = new_max_health {
                let mut stats = ecs.get_mut::<PlayerStats>(entity)?;
                stats.max_health = new_max;
                if stats.health > stats.max_health {
                    stats.health = stats.max_health;
                }
                send_u_packet(tx, "@L", &health(stats.health, stats.max_health).1);
            }

            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if upgraded {
        // Re-render the page with the same slot selected
        send_up_page(state, tx, pid, selected_slot);
    }
}

/// Delete skill from the selected slot.
/// C# ref: `PlayerSkills.DeleteSkill(Player p)`.
fn handle_skill_delete(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    slot: i32,
) {
    let selected_slot = get_selected_slot(state, pid);
    if selected_slot < 0 || slot != selected_slot {
        return;
    }

    state.modify_player(pid, |ecs, entity| {
        let skill_code = {
            let skills = ecs.get::<PlayerSkillsComp>(entity)?;
            get_skill_code_at_slot(&skills.states, slot)?
        };

        {
            let mut skills_mut = ecs.get_mut::<PlayerSkillsComp>(entity)?;
            skills_mut.states.remove(&skill_code);
        }

        let skills = ecs.get::<PlayerSkillsComp>(entity)?;
        send_player_level(tx, skills);
        send_player_skills(tx, skills);

        Some(())
    });

    // Re-render with no selection
    send_up_page(state, tx, pid, -1);
}

/// Install a new skill into the selected empty slot.
/// C# ref: `PlayerSkills.InstallSkill(string type, int slot, Player p)`.
fn handle_skill_install(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    code: &str,
    slot: i32,
) {
    let Some(skill_type) = SkillType::from_code(code) else {
        tracing::warn!(pid, code, "Up: invalid skill code for install");
        return;
    };

    let selected_slot = get_selected_slot(state, pid);
    if selected_slot < 0 || slot != selected_slot {
        return;
    }

    let installed = state
        .modify_player(pid, |ecs, entity| {
            // Validate before mutating
            {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;

                // Check the slot is empty
                if get_skill_code_at_slot(&skills.states, slot).is_some() {
                    return Some(false);
                }

                // Check slot is within bounds
                let total_slots = get_player_slot_count(skills);
                if slot >= total_slots {
                    return Some(false);
                }

                // Check skill is not already installed
                if skills.states.contains_key(skill_type.code()) {
                    return Some(false);
                }

                // Check visibility & requirements (1:1 with C# `Visible` + `meet`)
                if !is_skill_visible_and_meets_reqs(&skills.states, skill_type) {
                    return Some(false);
                }
            }

            // Install (mutable borrow)
            {
                let mut skills_mut = ecs.get_mut::<PlayerSkillsComp>(entity)?;
                skills_mut.states.insert(
                    skill_type.code().to_string(),
                    SkillState { level: 1, exp: 0.0 },
                );
            }

            // Send updates (immutable borrow)
            let skills = ecs.get::<PlayerSkillsComp>(entity)?;
            send_player_level(tx, skills);
            send_player_skills(tx, skills);

            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if installed {
        send_up_page(state, tx, pid, slot);
    }
}

/// Buy an additional slot (costs creds).
/// C# ref: `PlayerSkills.slots++` if `p.creds > 1000 && slots < 34`.
fn handle_buy_slot(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let bought = state
        .modify_player(pid, |ecs, entity| {
            // Validate (immutable borrows)
            {
                let stats = ecs.get::<PlayerStats>(entity)?;
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                let current_slots = get_player_slot_count(skills);

                if stats.creds <= SLOT_COST || current_slots >= MAX_SLOTS {
                    return Some(false);
                }
            }

            // Deduct creds (mutable borrow on stats)
            {
                let mut stats_mut = ecs.get_mut::<PlayerStats>(entity)?;
                stats_mut.creds -= SLOT_COST;
                send_u_packet(tx, "P$", &money(stats_mut.money, stats_mut.creds).1);
            }

            // Increment slot count (mutable borrow on skills)
            {
                let mut skills_mut = ecs.get_mut::<PlayerSkillsComp>(entity)?;
                skills_mut.total_slots += 1;
            }

            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if bought {
        send_up_page(state, tx, pid, -1);
    }
}

// ─── GUI rendering ──────────���────────────────────────────────────────────────

/// Build and send the UpPage JSON to the client.
/// Format: `"up:{json}"` sent via GU event.
fn send_up_page(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    selected_slot: i32,
) {
    let page_data = state
        .query_player(pid, |ecs, entity| {
            let skills = ecs.get::<PlayerSkillsComp>(entity)?;
            Some(build_up_page_json(
                &skills.states,
                skills.total_slots,
                selected_slot,
            ))
        })
        .flatten();

    if let Some(json_str) = page_data {
        send_u_packet(tx, "GU", format!("up:{json_str}").as_bytes());
    }

    // Store selected slot in window state
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) {
            // Preserve the "up:x:y" prefix, append selected slot
            if let Some(window) = &ui.current_window {
                if window.starts_with("up:") {
                    let base = window.split(':').take(3).collect::<Vec<_>>().join(":");
                    ui.current_window = Some(format!("{base}:{selected_slot}"));
                }
            }
        }
        Some(())
    });
}

/// Build the `UpPage` JSON string (1:1 with C# `Window.ToString()` for `UpPage`).
fn build_up_page_json(
    skills: &HashMap<String, SkillState>,
    total_slots: i32,
    selected_slot: i32,
) -> String {
    // Build skills list: "code:level:slot:can_upgrade" entries
    // C# ref: `GetSkills()` → `UpSkill(slot, lvl, isUpReady, type)`
    // `obj["k"] = join("#", skills.Select(x => "{type.GetCode()}:{level}:{slot}:{canUpgrade}"))`
    let mut skill_entries: Vec<String> = Vec::new();
    let sorted_codes = get_sorted_skill_codes(skills);
    for (slot, code) in sorted_codes.iter().enumerate() {
        if let Some(st) = skills.get(*code) {
            let skill_type = SkillType::from_code(code);
            let can_upgrade = skill_type.is_some_and(|st_type| {
                let needed = exp_needed(st_type, st.level);
                needed > 0.0 && st.exp >= needed
            });
            skill_entries.push(format!(
                "{}:{}:{}:{}",
                code,
                st.level,
                slot,
                if can_upgrade { "1" } else { "0" }
            ));
        }
    }
    let k_value = if skill_entries.is_empty() {
        "#".to_string()
    } else {
        format!("{}#", skill_entries.join("#"))
    };

    let mut obj = serde_json::Map::new();

    // Back button: always false for initial page
    obj.insert("back".into(), serde_json::Value::Bool(false));

    // Buttons: exit
    let buttons = serde_json::json!(["ВЫЙТИ", "exit"]);
    obj.insert("buttons".into(), buttons);

    // Skills list
    obj.insert("k".into(), serde_json::Value::String(k_value));
    // Slot amount
    obj.insert(
        "s".into(),
        serde_json::Value::Number(serde_json::Number::from(total_slots)),
    );
    // Selected slot
    obj.insert(
        "sl".into(),
        serde_json::Value::Number(serde_json::Number::from(selected_slot)),
    );

    if selected_slot < 0 {
        // No slot selected: show "choose a slot" text and possibly buy-slot button
        obj.insert(
            "txt".into(),
            serde_json::Value::String("Выберите скилл или пустой слот".into()),
        );

        if total_slots < MAX_SLOTS {
            obj.insert(
                "b".into(),
                serde_json::Value::String(format!("Купить слот ({SLOT_COST} кредов)")),
            );
            obj.insert("ba".into(), serde_json::Value::String("buyslot".into()));
        } else {
            obj.insert("b".into(), serde_json::Value::String(String::new()));
        }

        // No install list, no delete, no skill icon
        obj.insert("i".into(), serde_json::Value::String(String::new()));
        obj.insert("si".into(), serde_json::Value::String(String::new()));
    } else {
        // Slot selected: check if it has a skill or is empty
        let skill_at_slot = get_skill_code_at_slot(skills, selected_slot);

        if let Some(code) = skill_at_slot {
            // Slot has a skill — show description, upgrade button if ready, delete option
            let skill_type = SkillType::from_code(&code);
            let st = skills.get(&code);

            let description = if let (Some(stype), Some(state)) = (skill_type, st) {
                build_skill_description(stype, state)
            } else {
                String::new()
            };

            obj.insert("txt".into(), serde_json::Value::String(description));

            // Upgrade button if exp >= needed
            let can_upgrade = skill_type
                .and_then(|stype| {
                    let state = skills.get(&code)?;
                    let needed = exp_needed(stype, state.level);
                    Some(needed > 0.0 && state.exp >= needed)
                })
                .unwrap_or(false);

            if can_upgrade {
                obj.insert("b".into(), serde_json::Value::String("Улучшить".into()));
                obj.insert("ba".into(), serde_json::Value::String("upgrade".into()));
            } else {
                obj.insert("b".into(), serde_json::Value::String(String::new()));
            }

            // Delete available
            obj.insert(
                "del".into(),
                serde_json::Value::Number(serde_json::Number::from(1)),
            );

            // No install list (slot is occupied)
            obj.insert("i".into(), serde_json::Value::String(String::new()));

            // Skill icon
            obj.insert("si".into(), serde_json::Value::String(code));
        } else {
            // Slot is empty — show installable skills
            obj.insert(
                "txt".into(),
                serde_json::Value::String("Выберите навык для установки".into()),
            );
            obj.insert("b".into(), serde_json::Value::String(String::new()));

            // Build installable skills list
            // C# format: "code" if meets reqs, "_code" if visible but doesn't meet
            let install_list = get_installable_skills(skills);
            let i_value = install_list
                .iter()
                .map(|(stype, meets)| {
                    if *meets {
                        stype.code().to_string()
                    } else {
                        format!("_{}", stype.code())
                    }
                })
                .collect::<Vec<_>>()
                .join(":");

            obj.insert("i".into(), serde_json::Value::String(i_value));
            obj.insert("si".into(), serde_json::Value::String(String::new()));
        }
    }

    serde_json::Value::Object(obj).to_string()
}

// ─── Helpers ───────────────────────────────────────────────���─────────────────

/// Get the selected slot from the player's current_window state.
/// Window format: "up:{x}:{y}:{selected_slot}"
fn get_selected_slot(state: &Arc<GameState>, pid: PlayerId) -> i32 {
    state
        .query_player(pid, |ecs, entity| {
            let ui = ecs.get::<PlayerUI>(entity)?;
            let window = ui.current_window.as_deref()?;
            // "up:x:y:slot"
            let parts: Vec<&str> = window.split(':').collect();
            if parts.len() >= 4 {
                parts[3].parse::<i32>().ok()
            } else {
                Some(-1)
            }
        })
        .flatten()
        .unwrap_or(-1)
}

/// Get the total number of skill slots for a player from the component.
fn get_player_slot_count(comp: &PlayerSkillsComp) -> i32 {
    comp.total_slots
}

/// Get sorted skill codes (excluding meta keys like "__slots").
/// The sort order gives stable slot indices.
fn get_sorted_skill_codes(skills: &HashMap<String, SkillState>) -> Vec<&str> {
    let mut codes: Vec<&str> = skills
        .keys()
        .filter(|k| !k.starts_with("__"))
        .map(String::as_str)
        .collect();
    codes.sort_unstable();
    codes
}

/// Get the skill code at a given slot index.
/// Slot indices correspond to the sorted order of skill codes.
fn get_skill_code_at_slot(skills: &HashMap<String, SkillState>, slot: i32) -> Option<String> {
    if slot < 0 {
        return None;
    }
    let sorted = get_sorted_skill_codes(skills);
    sorted.get(slot as usize).map(|s| (*s).to_string())
}

/// Check if a skill is visible (requirements installed) and meets level requirements.
/// C# ref: `Skill.Visible(Player p, out bool meet)`:
/// - If any requirement skill is not installed → not visible (return false)
/// - If requirement skill level - 3 < required level → visible but doesn't meet
fn is_skill_visible_and_meets_reqs(skills: &HashMap<String, SkillState>, skill: SkillType) -> bool {
    if let Some(reqs) = get_skill_requirements(skill) {
        for (req_skill, req_lvl) in &reqs {
            if let Some(s) = skills.get(req_skill.code()) {
                // C# ref: `skill.lvl - 3 < req.Value` → meet = false
                if s.level - 3 < *req_lvl {
                    return false;
                }
            } else {
                // Requirement skill not installed ��� not visible
                return false;
            }
        }
    }
    true
}

/// Check if a skill is visible (requirement skills installed), regardless of level.
/// Returns (visible, meets_reqs).
fn skill_visibility(skills: &HashMap<String, SkillState>, skill: SkillType) -> (bool, bool) {
    if let Some(reqs) = get_skill_requirements(skill) {
        for (req_skill, req_lvl) in &reqs {
            if let Some(s) = skills.get(req_skill.code()) {
                // C# ref: `skill.lvl - 3 < req.Value` → meet = false
                if s.level - 3 < *req_lvl {
                    return (true, false);
                }
            } else {
                return (false, false);
            }
        }
    }
    (true, true)
}

/// Get the list of skills available for installation.
/// C# ref: `PlayerSkills.SkillToInstall(Player p)` → Dict<SkillType, bool>.
/// Returns Vec<(SkillType, meets_requirements)>.
fn get_installable_skills(skills: &HashMap<String, SkillState>) -> Vec<(SkillType, bool)> {
    use SkillType::*;
    // All known skill types (same order as C# `skillz` list)
    let all_skills = [
        Digging,
        BuildRoad,
        BuildGreen,
        BuildYellow,
        BuildRed,
        BuildStructure,
        BuildWar,
        Fridge,
        Movement,
        RoadMovement,
        Packing,
        Health,
        PackingBlue,
        PackingCyan,
        PackingGreen,
        PackingRed,
        PackingViolet,
        PackingWhite,
        MineGeneral,
        Induction,
        AntiGun,
        Repair,
    ];

    let mut result = Vec::new();
    for &stype in &all_skills {
        // Skip already installed
        if skills.contains_key(stype.code()) {
            continue;
        }
        let (visible, meets) = skill_visibility(skills, stype);
        if visible {
            result.push((stype, meets));
        }
    }
    result
}

/// Build a human-readable skill description.
/// C# ref: `Skill.Description` property.
fn build_skill_description(skill_type: SkillType, state: &SkillState) -> String {
    let lvl = state.level;
    let effect = skill_effect(skill_type, lvl);
    let needed = exp_needed(skill_type, lvl);
    let exp_str = if needed > 0.0 {
        format!("{:.1}/{:.1}", state.exp, needed)
    } else {
        "MAX".to_string()
    };

    match skill_type {
        SkillType::Digging => {
            format!("Копание Уровень:{lvl}\nExp - {exp_str}\nСила копания: {effect}")
        }
        SkillType::Movement => {
            let speed_kmh = 1.0 / (effect * 1.2 * 0.001) * 0.3 * 3.6;
            format!(
                "Передвижение Уровень:{lvl}\nExp - {exp_str}\nСкорость передвижения {speed_kmh:.2} км/ч"
            )
        }
        SkillType::BuildGreen => {
            let dur = lvl as f32;
            format!(
                "Стройка зеленых Уровень:{lvl}\nExp - {exp_str}\nСтоимость блока: {effect}\nПрочность блока: {dur}"
            )
        }
        SkillType::BuildYellow => {
            let dur = lvl as f32;
            format!(
                "Стройка желтых Уровень:{lvl}\nExp - {exp_str}\nСтоимость блока: {effect}\nПрочность блока: {dur}"
            )
        }
        SkillType::BuildRed => {
            let dur = lvl as f32;
            format!(
                "Стройка красных Урове��ь:{lvl}\nExp - {exp_str}\nСтоимость блока: {effect}\nПрочность блока: {dur}"
            )
        }
        SkillType::BuildWar => {
            let dur = lvl as f32;
            format!(
                "Стройка ВБ:{lvl}\nExp - {exp_str}\nСтоимость блока: {effect}\nПрочность блока: {dur}"
            )
        }
        SkillType::Health => {
            format!("Защита Уровень:{lvl}\nExp - {exp_str}\nМакс. здоровье: {effect}")
        }
        SkillType::Packing => {
            format!("Вместимость Уровень:{lvl}\nExp - {exp_str}\nЕмкость корзины: {effect}")
        }
        SkillType::AntiGun => {
            format!("Защита от пушек Уровень:{lvl}\nExp - {exp_str}\nСнижение урона: {effect}%")
        }
        SkillType::Repair => {
            format!("Ремонт Ур��вень:{lvl}\nExp - {exp_str}\nСила лечени��: {effect}")
        }
        SkillType::MineGeneral => {
            format!("Добыча Уровень:{lvl}\nExp - {exp_str}\nМножитель добычи: {effect:.2}")
        }
        SkillType::BuildRoad => {
            format!("��тройка дорог Уровень:{lvl}\nExp - {exp_str}\nСтоимо��ть блока: {effect}")
        }
        _ => {
            format!("lvl:{lvl} effect:{effect:.2} exp:{exp_str}")
        }
    }
}
