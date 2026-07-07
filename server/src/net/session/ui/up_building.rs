//! Up building (`PackType::Up`) — skill management GUI.
//!
//! 1:1 with C# `Buildings/Up.cs` + `GUI/UP/UpPage.cs` + `PlayerSkillsComp.cs`.
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

use crate::db::{SkillEntry, SkillSlots};
use crate::game::player::{PlayerSkillsComp, PlayerStats, PlayerUI};
use crate::game::skills::{
    self, OnHealth, PlayerSkills as PlayerSkillsHelper, SkillType, exp_needed,
    get_skill_requirements, skill_effect,
};
use crate::net::session::outbound::player_sync::{
    send_player_health, send_player_level, send_player_skills, send_player_speed,
};
use crate::net::session::prelude::*;
use crate::net::session::social::commands::send_ok;

/// Maximum number of skill slots a player can have.
const MAX_SLOTS: i32 = 34;

/// Minimum creds gate for buying an additional slot; C# checks it but does not spend it.
const SLOT_COST: i64 = 1000;

fn send_up_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("UP", "Состояние апгрейда недоступно.").1,
    );
}

// ─── Public API ─────────────────────────────────────────────────────────────────

/// Open the Up building GUI for a player. Called from `open_pack_gui`.
pub fn open_up_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    view: &PackView,
) {
    let opened = state
        .modify_player(pid, |ecs, entity| {
            let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing while opening Up GUI");
                return None;
            };
            ui.current_window = Some(format!("up:{}:{}", view.x, view.y));
            Some(())
        })
        .is_some();
    if !opened {
        send_up_state_error(tx);
        return;
    }

    // Send UpPage with no selected slot (initial state)
    send_up_page(state, tx, pid, -1);
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
    let has_up_window = state.query_player_opt(pid, |ecs, entity| {
        let Some(ui) = ecs.get::<PlayerUI>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for Up button");
            return None;
        };
        Some(
            ui.current_window
                .as_deref()
                .is_some_and(|w| w.starts_with("up:")),
        )
    });
    let Some(has_up_window) = has_up_window else {
        send_up_state_error(tx);
        return true;
    };

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

pub fn open_up_admin_gui(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    pack_x: i32,
    pack_y: i32,
) {
    let Some(view) = state.get_pack_at(pack_x, pack_y) else {
        return;
    };
    if view.owner_id != pid {
        return;
    }

    let payload = format!("horb:{}", build_up_admin_page_json(&view));
    send_u_packet(tx, "GU", payload.as_bytes());
    let opened = state
        .modify_player(pid, |ecs, entity| {
            let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing while opening Up admin GUI");
                return None;
            };
            ui.current_window = Some(format!("up:{pack_x}:{pack_y}:admin"));
            Some(())
        })
        .is_some();
    if !opened {
        send_up_state_error(tx);
    }
}

// ─── Internal handlers ─────────────────────────────────────────────────────────

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
    let Some(selected_slot) = get_selected_slot(state, tx, pid) else {
        return;
    };
    if selected_slot < 0 {
        return;
    }

    let upgraded = state
        .modify_player(pid, |ecs, entity| {
            // Read skill code, check exp-readiness и считаем цену апгрейда (деньги).
            let (skill_type, needed, cost) = {
                let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                let entry = skills.states.skills.get(&selected_slot)?;
                let stype = SkillType::from_code(&entry.code)?;
                let need = exp_needed(stype, entry.level);
                if need > 0.0 && entry.exp < need {
                    return Some(false);
                }
                // Цена в деньгах: base * текущий уровень (config-driven). В C#
                // апгрейд бесплатный — намеренная экономик-девиация (DEVIATIONS.md).
                let cost =
                    state.config.gameplay.skills.upgrade_cost_base * i64::from(entry.level).max(1);
                (stype, need, cost)
            };

            // Не хватает денег → сообщение и стоп (без апгрейда).
            {
                let Some(pstats) = ecs.get::<PlayerStats>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                if pstats.money < cost {
                    send_ok(tx, "Апгрейд", &format!("Недостаточно денег: нужно {cost}"));
                    return Some(false);
                }
                let Some(_) = ecs.get::<crate::game::PlayerFlags>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
            }

            // Списываем деньги + P$ + dirty (иначе списание не сохранится).
            {
                let Some(mut pstats_mut) = ecs.get_mut::<PlayerStats>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing while applying Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                pstats_mut.money -= cost;
                send_u_packet(tx, "P$", &money(pstats_mut.money, pstats_mut.creds).1);
            }
            let Some(mut flags) = ecs.get_mut::<crate::game::PlayerFlags>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while applying Up skill upgrade");
                send_up_state_error(tx);
                return None;
            };
            flags.dirty = true;

            // Perform upgrade (mutable borrow) — 1:1 C# `Skill.Up`: exp-=need, lvl+1.
            {
                let Some(mut skills_mut) = ecs.get_mut::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing while applying Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                let entry = skills_mut.states.skills.get_mut(&selected_slot)?;
                if needed > 0.0 {
                    entry.exp -= needed;
                }
                entry.level += 1;
            }

            // Send updated skills progress and compute health changes
            let new_max_health = {
                let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing after Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                send_player_skills(tx, skills);
                send_player_level(tx, skills);
                let Some(pstats) = ecs.get::<PlayerStats>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing after Up skill upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                send_player_health(tx, pstats);

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
                let Some(mut pstats) = ecs.get_mut::<PlayerStats>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing while applying Up health upgrade");
                    send_up_state_error(tx);
                    return None;
                };
                pstats.max_health = new_max;
                if pstats.health > pstats.max_health {
                    pstats.health = pstats.max_health;
                }
                send_u_packet(tx, "@L", &health(pstats.health, pstats.max_health).1);
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
/// C# ref: `PlayerSkillsComp.DeleteSkill(Player p)`.
fn handle_skill_delete(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    slot: i32,
) {
    let Some(selected_slot) = get_selected_slot(state, tx, pid) else {
        return;
    };
    if selected_slot < 0 || slot != selected_slot {
        return;
    }

    let deleted = state
        .modify_player(pid, |ecs, entity| {
        {
            let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for Up skill delete");
                send_up_state_error(tx);
                return None;
            };
            if !skills.states.skills.contains_key(&slot) {
                return Some(false);
            }
            let Some(_) = ecs.get::<crate::game::PlayerFlags>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for Up skill delete");
                send_up_state_error(tx);
                return None;
            };
        }

        {
            let Some(mut skills_mut) = ecs.get_mut::<PlayerSkillsComp>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing while applying Up skill delete");
                send_up_state_error(tx);
                return None;
            };
            skills_mut.states.skills.remove(&slot);
        }
        let Some(mut flags) = ecs.get_mut::<crate::game::PlayerFlags>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while applying Up skill delete");
            send_up_state_error(tx);
            return None;
        };
        flags.dirty = true;

        let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing after Up skill delete");
            send_up_state_error(tx);
            return None;
        };
        send_player_level(tx, skills);

        Some(true)
    })
        .flatten()
        .unwrap_or(false);

    // Re-render with no selection
    if deleted {
        send_up_page(state, tx, pid, -1);
    }
}

/// Install a new skill into the selected empty slot.
/// C# ref: `PlayerSkillsComp.InstallSkill(string type, int slot, Player p)`.
fn handle_skill_install(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    code: &str,
    slot: i32,
) {
    let Some(skill_type) = SkillType::from_code(code) else {
        tracing::warn!(pid = %pid, code, "Up: invalid skill code for install");
        return;
    };

    let Some(selected_slot) = get_selected_slot(state, tx, pid) else {
        return;
    };
    if selected_slot < 0 || slot != selected_slot {
        return;
    }

    let installed = state
        .modify_player(pid, |ecs, entity| {
            // Validate before mutating
            {
                let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for Up skill install");
                    send_up_state_error(tx);
                    return None;
                };

                // Check the slot is empty
                if skills.states.skills.contains_key(&slot) {
                    return Some(false);
                }

                // Check slot is within bounds
                let total_slots = get_player_slot_count(skills);
                if slot >= total_slots {
                    return Some(false);
                }

                // Check skill is not already installed (1:1 C# SkillToInstall filter)
                if skills.states.find(skill_type.code()).is_some() {
                    return Some(false);
                }

                // Check visibility & requirements (1:1 with C# `Visible` + `meet`)
                if !is_skill_visible_and_meets_reqs(&skills.states, skill_type) {
                    return Some(false);
                }
                let Some(_) = ecs.get::<crate::game::PlayerFlags>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for Up skill install");
                    send_up_state_error(tx);
                    return None;
                };
            }

            // Install (mutable borrow)
            {
                let Some(mut skills_mut) = ecs.get_mut::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing while applying Up skill install");
                    send_up_state_error(tx);
                    return None;
                };
                skills_mut.states.skills.insert(
                    slot,
                    SkillEntry {
                        code: skill_type.code().to_string(),
                        level: 1,
                        exp: 0.0,
                    },
                );
            }
            let Some(mut flags) = ecs.get_mut::<crate::game::PlayerFlags>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while applying Up skill install");
                send_up_state_error(tx);
                return None;
            };
            flags.dirty = true;

            // Send updates (immutable borrow)
            let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing after Up skill install");
                send_up_state_error(tx);
                return None;
            };
            send_player_level(tx, skills);

            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if installed {
        send_up_page(state, tx, pid, slot);
    }
}

/// Buy an additional slot.
/// C# ref: `PlayerSkillsComp.slots++` if `p.creds > 1000 && slots < 34`.
fn handle_buy_slot(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let bought = state
        .modify_player(pid, |ecs, entity| {
            // Validate (immutable borrows)
            {
                let Some(pstats) = ecs.get::<PlayerStats>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for Up slot purchase");
                    send_up_state_error(tx);
                    return None;
                };
                let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for Up slot purchase");
                    send_up_state_error(tx);
                    return None;
                };
                let current_slots = get_player_slot_count(skills);

                if pstats.creds <= SLOT_COST || current_slots >= MAX_SLOTS {
                    return Some(false);
                }
                let Some(_) = ecs.get::<crate::game::PlayerFlags>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing for Up slot purchase");
                    send_up_state_error(tx);
                    return None;
                };
            }

            // Increment slot count (mutable borrow on skills)
            {
                let Some(mut skills_mut) = ecs.get_mut::<PlayerSkillsComp>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing while applying Up slot purchase");
                    send_up_state_error(tx);
                    return None;
                };
                skills_mut.states.total_slots += 1;
            }
            let Some(mut flags) = ecs.get_mut::<crate::game::PlayerFlags>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerFlags", "Player component missing while applying Up slot purchase");
                send_up_state_error(tx);
                return None;
            };
            flags.dirty = true;

            Some(true)
        })
        .flatten()
        .unwrap_or(false);

    if bought {
        send_up_page(state, tx, pid, -1);
    }
}

// ─── GUI rendering ─────────────────────────────────────────────────────────────

/// Build and send the `UpPage` JSON to the client.
/// Format: `"up:{json}"` sent via GU event.
fn send_up_page(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    selected_slot: i32,
) {
    let page_data = state.query_player_opt(pid, |ecs, entity| {
        let Some(skills) = ecs.get::<PlayerSkillsComp>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerSkillsComp", "Player component missing for Up page");
            return None;
        };
        let is_owner = current_up_pack_owner(state, pid);
        Some(build_up_page_json(
            &skills.states,
            skills.states.total_slots,
            selected_slot,
            is_owner,
        ))
    });

    let Some(json_str) = page_data else {
        send_up_state_error(tx);
        return;
    };
    send_u_packet(tx, "GU", format!("up:{json_str}").as_bytes());

    // Store selected slot in window state
    let updated = state
        .modify_player(pid, |ecs, entity| {
            let Some(mut ui) = ecs.get_mut::<PlayerUI>(entity) else {
                tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing while storing Up page state");
                return None;
            };
            // Preserve the "up:x:y" prefix, append selected slot
            let Some(window) = &ui.current_window else {
                return Some(());
            };
            if window.starts_with("up:") {
                let base = window.split(':').take(3).collect::<Vec<_>>().join(":");
                ui.current_window = Some(format!("{base}:{selected_slot}"));
            }
            Some(())
        })
        .is_some();
    if !updated {
        send_up_state_error(tx);
    }
}

/// Get the selected slot from the player's `current_window` state.
/// Window format: "`up:{x}:{y}:{selected_slot`}"
fn get_selected_slot(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) -> Option<i32> {
    let selected = state.query_player_opt(pid, |ecs, entity| {
        let Some(ui) = ecs.get::<PlayerUI>(entity) else {
            tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for Up selected slot");
            return None;
        };
        let Some(window) = ui.current_window.as_deref() else {
            return Some(-1);
        };
        // "up:x:y:slot"
        let parts: Vec<&str> = window.split(':').collect();
        if parts.len() >= 4 {
            Some(parts[3].parse::<i32>().ok().unwrap_or(-1))
        } else {
            Some(-1)
        }
    });
    if selected.is_none() {
        send_up_state_error(tx);
    }
    selected
}

/// Build the `UpPage` JSON string (1:1 with C# `Window.ToString()` for `UpPage`).
fn build_up_page_json(
    skills: &SkillSlots,
    total_slots: i32,
    selected_slot: i32,
    is_owner: bool,
) -> String {
    // Build skills list: "code:level:slot:can_upgrade" per УСТАНОВЛЕННЫЙ скилл.
    // 1:1 C# `obj["k"] = join("#", Skills.Select(x => "{code}:{level}:{slot}:{canUp}"))`.
    // Слоты реальные (ключ map). Сортируем по слоту для детерминированного вывода.
    let mut slotted: Vec<(&i32, &SkillEntry)> = skills.skills.iter().collect();
    slotted.sort_by_key(|(slot, _)| **slot);
    let mut skill_entries: Vec<String> = Vec::new();
    for (slot, entry) in slotted {
        let can_upgrade = SkillType::from_code(&entry.code).is_some_and(|st_type| {
            let needed = exp_needed(st_type, entry.level);
            entry.exp >= needed
        });
        skill_entries.push(format!(
            "{}:{}:{}:{}",
            entry.code,
            entry.level,
            slot,
            if can_upgrade { "1" } else { "0" }
        ));
    }
    let k_value = if skill_entries.is_empty() {
        "#".to_string()
    } else {
        format!("{}#", skill_entries.join("#"))
    };

    let mut obj = serde_json::Map::new();

    obj.insert(
        "title".into(),
        serde_json::Value::String(if selected_slot < 0 { "xxx" } else { "penis" }.into()),
    );
    if is_owner {
        obj.insert("admin".into(), serde_json::Value::Bool(true));
    }

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
        // Slot selected: check if it has a skill or is empty (реальный слот).
        if let Some(entry) = skills.skills.get(&selected_slot) {
            // Slot has a skill — show description, upgrade button if ready, delete option
            let skill_type = SkillType::from_code(&entry.code);

            let description =
                skill_type.map_or_else(String::new, |stype| build_skill_description(stype, entry));
            obj.insert("txt".into(), serde_json::Value::String(description));

            // Upgrade button if exp >= needed
            let can_upgrade = skill_type.is_some_and(|stype| {
                let needed = exp_needed(stype, entry.level);
                entry.exp >= needed
            });

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
            obj.insert("si".into(), serde_json::Value::String(entry.code.clone()));
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

// ─── Helpers ───────────────────────────────────────────────────────────────────

fn current_up_pack_owner(state: &Arc<GameState>, pid: PlayerId) -> bool {
    let coords = state.query_player_opt(pid, |ecs, entity| {
        let ui = ecs.get::<PlayerUI>(entity)?;
        let window = ui.current_window.as_deref()?;
        let rest = window.strip_prefix("up:")?;
        let mut parts = rest.split(':');
        let x = parts.next()?.parse::<i32>().ok()?;
        let y = parts.next()?.parse::<i32>().ok()?;
        Some((x, y))
    });

    coords
        .and_then(|(x, y)| state.get_pack_at(x, y))
        .is_some_and(|view| view.owner_id == pid)
}

/// Get the total number of skill slots for a player from the component.
const fn get_player_slot_count(comp: &PlayerSkillsComp) -> i32 {
    comp.states.total_slots
}

/// Check if a skill is visible (requirements installed) and meets level requirements.
/// C# ref: `Skill.Visible(Player p, out bool meet)`:
/// - If any requirement skill is not installed → not visible (return false)
/// - If requirement skill level - 3 < required level → visible but doesn't meet
fn is_skill_visible_and_meets_reqs(skills: &SkillSlots, skill: SkillType) -> bool {
    if let Some(reqs) = get_skill_requirements(skill) {
        for (req_skill, req_lvl) in &reqs {
            if let Some(s) = skills.find(req_skill.code()) {
                // C# ref: `skill.lvl - 3 < req.Value` → meet = false
                if s.level - 3 < *req_lvl {
                    return false;
                }
            } else {
                // Requirement skill not installed  — not visible
                return false;
            }
        }
    }
    true
}

/// Check if a skill is visible (requirement skills installed), regardless of level.
/// Returns (visible, `meets_reqs`).
fn skill_visibility(skills: &SkillSlots, skill: SkillType) -> (bool, bool) {
    if let Some(reqs) = get_skill_requirements(skill) {
        for (req_skill, req_lvl) in &reqs {
            if let Some(s) = skills.find(req_skill.code()) {
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
/// C# ref: `PlayerSkillsComp.SkillToInstall(Player p)` → Dict<`SkillType`, bool>.
/// Returns Vec<(`SkillType`, `meets_requirements`)>.
fn get_installable_skills(skills: &SkillSlots) -> Vec<(SkillType, bool)> {
    use SkillType::{
        AntiGun, BuildGreen, BuildRed, BuildRoad, BuildStructure, BuildWar, BuildYellow, Digging,
        Fridge, Health, Induction, MineGeneral, Movement, Packing, PackingBlue, PackingCyan,
        PackingGreen, PackingRed, PackingViolet, PackingWhite, Repair, RoadMovement,
    };
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
        if skills.find(stype.code()).is_some() {
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
fn build_skill_description(skill_type: SkillType, state: &SkillEntry) -> String {
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
                "Стройка красных Уровень:{lvl}\nExp - {exp_str}\nСтоимость блока: {effect}\nПрочность блока: {dur}"
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
            format!("Ремонт Уровень:{lvl}\nExp - {exp_str}\nСила лечения: {effect}")
        }
        SkillType::MineGeneral => {
            format!("Добыча Уровень:{lvl}\nExp - {exp_str}\nМножитель добычи: {effect:.2}")
        }
        SkillType::BuildRoad => {
            format!("Стройка дорог Уровень:{lvl}\nExp - {exp_str}\nСтоимость блока: {effect}")
        }
        _ => {
            let cost = skill_cost(skill_type, lvl);
            format!("lvl:{lvl} effect:{effect:.2} cost:{cost} exp:{exp_str}")
        }
    }
}

const fn skill_cost(skill_type: SkillType, _level: i32) -> i32 {
    match skill_type {
        SkillType::Movement => 0,
        _ => 1,
    }
}

fn build_up_admin_page_json(view: &PackView) -> serde_json::Value {
    serde_json::json!({
        "title": "UP",
        "text": "",
        "back": false,
        "buttons": [],
        "richList": [
            format!("hp {}/{}", view.hp, view.max_hp), "text", "", "", "",
            "динаху", "text", "", "", ""
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    #[test]
    fn up_page_json_has_reference_titles() {
        let slots = SkillSlots {
            skills: HashMap::new(),
            total_slots: 20,
        };

        let no_selection: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, -1, false)).unwrap();
        let selected: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, 0, false)).unwrap();

        assert_eq!(no_selection["title"], "xxx");
        assert_eq!(selected["title"], "penis");
    }

    #[test]
    fn anti_gun_is_marked_upgrade_ready_when_exp_needed_is_zero() {
        let slots = SkillSlots {
            skills: HashMap::from([(
                0,
                SkillEntry {
                    code: SkillType::AntiGun.code().to_string(),
                    level: 1,
                    exp: 0.0,
                },
            )]),
            total_slots: 20,
        };

        let json: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, 0, false)).unwrap();
        assert_eq!(json["k"], "u:1:0:1#");
    }

    #[test]
    fn up_page_json_marks_owner_admin() {
        let slots = SkillSlots {
            skills: HashMap::new(),
            total_slots: 20,
        };

        let owner: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, -1, true)).unwrap();
        let non_owner: serde_json::Value =
            serde_json::from_str(&build_up_page_json(&slots, 20, -1, false)).unwrap();

        assert_eq!(owner["admin"], true);
        assert!(non_owner.get("admin").is_none());
    }

    #[test]
    fn up_admin_page_matches_reference_content() {
        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: PlayerId(1),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 123,
            max_hp: 1000,
        };

        let json = build_up_admin_page_json(&view);
        assert_eq!(json["title"], "UP");
        assert_eq!(
            json["richList"],
            serde_json::json!([
                "hp 123/1000",
                "text",
                "",
                "",
                "",
                "динаху",
                "text",
                "",
                "",
                ""
            ])
        );
    }

    #[test]
    fn generic_skill_description_includes_cost_field() {
        let entry = SkillEntry {
            code: SkillType::Induction.code().to_string(),
            level: 2,
            exp: 0.0,
        };

        let description = build_skill_description(SkillType::Induction, &entry);
        assert!(description.contains("cost:1"));
    }

    #[tokio::test]
    async fn buyslot_keeps_creds_and_does_not_send_money_packet() {
        let test = make_up_test_state("buyslot").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: test.player.id.into(),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };
        open_up_gui(&test.state, &tx, test.player.id.into(), &view);
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "buyslot"
        ));

        let events = drain_events(&mut rx);
        assert!(events.iter().all(|(event, _)| event != "P$"));
        assert!(events.iter().any(|(event, payload)| {
            event == "GU" && std::str::from_utf8(payload).is_ok_and(|s| s.starts_with("up:"))
        }));

        let (creds, total_slots, dirty) =
            player_creds_slots_and_dirty(&test.state, test.player.id.into());
        assert_eq!(creds, 1001);
        assert_eq!(total_slots, 21);
        assert!(dirty);

        test.cleanup();
    }

    #[tokio::test]
    async fn skill_upgrade_sends_health_packet_for_non_health_skill() {
        let mut test = make_up_test_state("upgrade_health_packet").await;
        test.player.money = 1_000;
        test.player.skills.skills.get_mut(&1).unwrap().exp = 1.0;

        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);
        let expected_health = player_health_payload(&test.state, test.player.id.into());

        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: test.player.id.into(),
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };
        open_up_gui(&test.state, &tx, test.player.id.into(), &view);
        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "skill:1"
        ));
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "upgrade"
        ));

        let events = drain_events(&mut rx);
        let event_names = events
            .iter()
            .map(|(event, _)| event.as_str())
            .collect::<Vec<_>>();

        assert!(
            event_names
                .windows(3)
                .any(|window| window == ["@S", "LV", "@L"])
        );
        assert!(
            events
                .iter()
                .any(|(event, payload)| event == "@L" && payload == expected_health.as_bytes())
        );

        test.cleanup();
    }

    #[tokio::test]
    async fn skill_delete_sends_level_without_skills_packet() {
        let test = make_up_test_state("delete_no_skills_packet").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        open_test_up_gui(&test.state, &tx, test.player.id.into());
        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "skill:1"
        ));
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "delete:1"
        ));

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "LV"));
        assert!(events.iter().all(|(event, _)| event != "@S"));

        test.cleanup();
    }

    #[tokio::test]
    async fn skill_install_sends_level_without_skills_packet() {
        let test = make_up_test_state("install_no_skills_packet").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        open_test_up_gui(&test.state, &tx, test.player.id.into());
        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "skill:4"
        ));
        drain_events(&mut rx);

        assert!(handle_up_button(
            &test.state,
            &tx,
            test.player.id.into(),
            "install:p#4"
        ));

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "LV"));
        assert!(events.iter().all(|(event, _)| event != "@S"));

        test.cleanup();
    }

    #[tokio::test]
    async fn up_button_missing_ui_is_explicit_error_not_unhandled_button() {
        let test = make_up_test_state("up_button_missing_ui").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerUI>();
        }

        assert!(handle_up_button(&test.state, &tx, pid, "buyslot"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние апгрейда недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn buyslot_missing_skills_is_explicit_error_not_not_enough_slots_noop() {
        let test = make_up_test_state("buyslot_missing_skills").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        drain_events(&mut rx);
        {
            let entity = test.state.get_player_entity(pid).unwrap();
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerSkillsComp>();
        }

        assert!(handle_up_button(&test.state, &tx, pid, "buyslot"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние апгрейда недоступно."));

        test.cleanup();
    }

    #[tokio::test]
    async fn skill_upgrade_missing_flags_is_explicit_error_before_money_or_skill_mutation() {
        let mut test = make_up_test_state("upgrade_missing_flags").await;
        test.player.money = 1_000;
        test.player.skills.skills.get_mut(&1).unwrap().exp = 1.0;

        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:1"));
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "upgrade"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(
            std::str::from_utf8(&events[0].1)
                .unwrap()
                .contains("Состояние апгрейда недоступно.")
        );
        assert_eq!(player_money(&test.state, pid), 1_000);
        assert_eq!(skill_level_exp(&test.state, pid, 1), (1, 1.0));

        test.cleanup();
    }

    #[tokio::test]
    async fn skill_delete_missing_flags_is_explicit_error_without_skill_mutation_or_rerender() {
        let test = make_up_test_state("delete_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:1"));
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "delete:1"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(skill_exists(&test.state, pid, 1));

        test.cleanup();
    }

    #[tokio::test]
    async fn skill_install_missing_flags_is_explicit_error_without_skill_mutation() {
        let test = make_up_test_state("install_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        assert!(handle_up_button(&test.state, &tx, pid, "skill:4"));
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "install:p#4"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(!skill_exists(&test.state, pid, 4));

        test.cleanup();
    }

    #[tokio::test]
    async fn buyslot_missing_flags_is_explicit_error_without_slot_mutation() {
        let test = make_up_test_state("buyslot_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        open_test_up_gui(&test.state, &tx, pid);
        drain_events(&mut rx);
        remove_player_flags(&test.state, pid);

        assert!(handle_up_button(&test.state, &tx, pid, "buyslot"));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert_eq!(player_slot_count(&test.state, pid), 20);

        test.cleanup();
    }

    struct UpTestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        db_path: std::path::PathBuf,
        world_name: String,
        dir: std::path::PathBuf,
    }

    impl UpTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ =
                std::fs::remove_file(self.dir.join(format!("{}_durability.map", self.world_name)));
        }
    }

    fn open_test_up_gui(
        state: &Arc<GameState>,
        tx: &mpsc::UnboundedSender<Vec<u8>>,
        pid: PlayerId,
    ) {
        let view = PackView {
            id: 1,
            pack_type: crate::game::PackType::Up,
            x: 10,
            y: 10,
            owner_id: pid,
            clan_id: 0,
            charge: 0,
            max_charge: 0,
            hp: 1000,
            max_hp: 1000,
        };
        open_up_gui(state, tx, pid, &view);
    }

    async fn make_up_test_state(label: &str) -> UpTestState {
        let dir = std::env::temp_dir();
        let nonce = format!("{}_{}_{}", label, std::process::id(), unique_test_nonce());
        let db_path = dir.join(format!("up_building_{nonce}.db"));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let mut player = database.create_player("up-user", "p", "h").await.unwrap();
        player.creds = 1001;

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("up_building_world_{nonce}");
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };
        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config)
            .await
            .unwrap();

        UpTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    fn player_creds_slots_and_dirty(state: &Arc<GameState>, pid: PlayerId) -> (i64, i32, bool) {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                let flags = ecs.get::<crate::game::PlayerFlags>(entity)?;
                Some((player_stats.creds, skills.states.total_slots, flags.dirty))
            })
            .unwrap()
    }

    fn player_health_payload(state: &Arc<GameState>, pid: PlayerId) -> String {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(format!(
                    "{}:{}",
                    player_stats.health, player_stats.max_health
                ))
            })
            .unwrap()
    }

    fn remove_player_flags(state: &Arc<GameState>, pid: PlayerId) {
        let entity = state.get_player_entity(pid).unwrap();
        let mut ecs = state.ecs.write();
        ecs.entity_mut(entity).remove::<crate::game::PlayerFlags>();
    }

    fn player_money(state: &Arc<GameState>, pid: PlayerId) -> i64 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let player_stats = ecs.get::<PlayerStats>(entity)?;
                Some(player_stats.money)
            })
            .unwrap()
    }

    fn player_slot_count(state: &Arc<GameState>, pid: PlayerId) -> i32 {
        state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                Some(skills.states.total_slots)
            })
            .unwrap()
    }

    fn skill_exists(state: &Arc<GameState>, pid: PlayerId, slot: i32) -> bool {
        state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                Some(skills.states.skills.contains_key(&slot))
            })
            .unwrap()
    }

    fn skill_level_exp(state: &Arc<GameState>, pid: PlayerId, slot: i32) -> (i32, f32) {
        state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                let skill = skills.states.skills.get(&slot)?;
                Some((skill.level, skill.exp))
            })
            .unwrap()
    }

    fn drain_events(rx: &mut mpsc::UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn unique_test_nonce() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
