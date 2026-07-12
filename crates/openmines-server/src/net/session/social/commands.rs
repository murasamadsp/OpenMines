//! Слэш-команды чата: /give, /money, /tp, /heal, /kick, /role, /clan, /pack, /admin.
use crate::db::players::{PlayerRow, Role, SkillEntry};
use crate::game::logic::numeric::saturating_trunc_f32_to_i32;
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerSkillsComp, PlayerStats};
use crate::game::skills::MAX_SKILL_SLOTS;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::outbound::player_sync::{
    send_player_basket, send_player_health, send_player_level, send_player_skills,
    send_player_speed,
};
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    building_extra_for_pack_type, modify_pack_with_db, validate_pack_footprint,
};
use crate::net::session::social::clans::{
    handle_clan_create, handle_clan_kick_by_name, handle_clan_leave,
};
use strum::IntoEnumIterator;

const CMD_USAGE_GIVE: &str = "Использование: /give ITEM_ID AMOUNT";
const CMD_USAGE_MONEY: &str = "Использование: /money AMOUNT";
const CMD_USAGE_MONEY_ALL: &str = "Использование: /moneyall AMOUNT";
const CMD_USAGE_SKILL: &str =
    "Использование: /skill ИМЯ|me CODE LEVEL [SLOT] [EXP]. Коды: /skill codes";
// Largest f32 whose `exp * 100` progress value still fits the client's i32 field.
const MAX_ADMIN_SKILL_EXP: f32 = 21_474_834.0;
const CMD_USAGE_TP: &str = "Использование: /tp X Y";
const CMD_USAGE_PACK_OWNER: &str = "Использование: /pack owner X Y OWNER_ID";
const CMD_USAGE_PACK_CLAN: &str = "Использование: /pack clan X Y CLAN_ID";
const CMD_USAGE_PACK_MOVE: &str = "Использование: /pack move X Y NX NY";
const CMD_USAGE_PACK_TYPE: &str = "Использование: /pack type X Y TYPE (T/R/G/M/U/L/F/O)";
const CMD_USAGE_PACK: &str = "Команды: /pack owner X Y OWNER_ID | /pack clan X Y CLAN_ID | /pack move X Y NX NY | /pack type X Y TYPE";
const CMD_USAGE_KICK: &str = "Использование: /kick ИМЯ";
const CMD_USAGE_ROLE: &str = "Использование: /role ИМЯ admin|mod|player";
const CMD_USAGE_CLAN_CREATE: &str = "Использование: /clan create ИМЯ ТЕГ";
const CMD_USAGE_CLAN_KICK: &str = "Использование: /clan kick ИМЯ";
const CMD_USAGE_CLAN: &str = "Команды: /clan create ИМЯ ТЕГ | /clan leave | /clan kick ИМЯ";

const ADMIN_COMMAND_NO_RIGHTS: &str = "Нет прав на админ-команду";

// ─── Shared helpers ─────────────────────────────────────────────────────────

pub fn send_ok(tx: &Outbox, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}

pub fn send_admin_help(tx: &Outbox) {
    send_ok(tx, "Админ-команды", &crate::admin::slash_help());
}

fn send_command_state_error(tx: &Outbox) {
    send_ok(tx, "КОМАНДА", "Состояние игрока недоступно.");
}

pub fn is_admin_command(state: &Arc<GameState>, pid: PlayerId) -> bool {
    state
        .query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
            ecs.get::<crate::game::player::PlayerStats>(entity)
                .is_some_and(|s| s.role == 2)
        })
        .unwrap_or(false)
}

pub fn handle_admin_action(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    // C# ref: ADMN triggers AdminButton() on current window (gear icon).
    let handled = state.query_player_expected(pid, "ADMN packet", |ecs, entity| {
        let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
        ui.current_window.clone()
    });
    if let Some(ref window) = handled {
        if let Some(coords) = window.strip_prefix("resp:") {
            let parts: Vec<&str> = coords.split(':').collect();
            if parts.len() == 2
                && let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
            {
                crate::net::session::play::packs::open_resp_admin_gui(state, tx, pid, x, y);
                return;
            }
        }
        if let Some(rest) = window.strip_prefix("market:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() >= 2
                && let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
            {
                crate::net::session::ui::gui_buttons::open_market_admin_gui(state, tx, pid, x, y);
                return;
            }
        }
        if let Some(rest) = window.strip_prefix("up:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() >= 2
                && let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
            {
                crate::net::session::ui::up_building::open_up_admin_gui(state, tx, pid, x, y);
                return;
            }
        }
        if let Some(rest) = window.strip_prefix("pack:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() == 2
                && let (Ok(x), Ok(y)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>())
            {
                crate::net::session::ui::gui_buttons::open_pack_admin_gui(state, tx, pid, x, y);
                return;
            }
        }
    }
    if is_admin_command(state, pid) {
        send_admin_help(tx);
    } else {
        send_ok(tx, "Ошибка", "Нет прав администратора.");
    }
}

fn ensure_admin(tx: &Outbox, state: &Arc<GameState>, pid: PlayerId) -> bool {
    if is_admin_command(state, pid) {
        true
    } else {
        send_ok(tx, "Ошибка", ADMIN_COMMAND_NO_RIGHTS);
        false
    }
}

// ─── Main dispatcher ────────────────────────────────────────────────────────

pub async fn handle_chat_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, msg: &str) {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    let cmd = parts[0];
    let args = &parts[1..];

    let is_admin = is_admin_command(state, pid);
    tracing::info!(
        target: "audit",
        player_id = %pid,
        is_admin,
        command = cmd,
        arguments = ?args,
        "Chat command executed"
    );
    match crate::admin::AdminCommandName::from_slash(cmd) {
        Some(crate::admin::AdminCommandName::Give) => {
            handle_chat_give_command(state, tx, pid, args);
        }
        Some(crate::admin::AdminCommandName::GiveAll) => {
            handle_chat_giveall_command(state, tx, pid);
        }
        Some(crate::admin::AdminCommandName::Money) => {
            handle_chat_money_command(state, tx, pid, args);
        }
        Some(crate::admin::AdminCommandName::MoneyAll) => {
            handle_chat_money_all_command(state, tx, pid, args).await;
        }
        Some(crate::admin::AdminCommandName::Skill) => {
            handle_chat_skill_command(state, tx, pid, args).await;
        }
        Some(crate::admin::AdminCommandName::Teleport) => {
            handle_chat_teleport_command(state, tx, pid, args);
        }
        Some(crate::admin::AdminCommandName::Heal) => {
            handle_chat_heal_command(state, tx, pid);
        }
        Some(crate::admin::AdminCommandName::Kick) => {
            handle_chat_kick_command(state, tx, pid, args);
        }
        Some(crate::admin::AdminCommandName::Role) => {
            handle_chat_role_command(state, tx, pid, args).await;
        }
        Some(crate::admin::AdminCommandName::Clan) => {
            handle_chat_clan_command(state, tx, pid, args).await;
        }
        Some(crate::admin::AdminCommandName::Pack) => {
            handle_chat_pack_command(state, tx, pid, args);
        }
        Some(crate::admin::AdminCommandName::Help) => {
            if ensure_admin(tx, state, pid) {
                send_admin_help(tx);
            }
        }
        None => {
            if is_admin_command(state, pid) {
                send_admin_help(tx);
            } else {
                send_ok(tx, "Ошибка", &format!("Неизвестная команда: {cmd}"));
            }
        }
        Some(_) => unreachable!("slash parser returned a console-only admin command"),
    }
}

// ─── /giveall ───────────────────────────────────────────────────────────────

fn handle_chat_giveall_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    // ВСЕ предметы = индексы 0..=50. Это полный список из атласа клиента
    // (`client/Assets/Resources/inventory/inventory.png` — спрайты inventory_0..50).
    // Диапазон ровно совпадает с атласом → клиент рисует `sprites[id]` без выхода
    // за границы (давать 0..=50 безопасно). Часть индексов сервер пока не
    // обрабатывает в Use(), но предметы существуют и админ должен иметь всё.
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        if ecs.get::<PlayerInventory>(entity).is_none()
            || ecs.get::<PlayerStats>(entity).is_none()
            || ecs.get::<PlayerFlags>(entity).is_none()
        {
            send_command_state_error(tx);
            return Some(());
        }
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        for id in 0..=50 {
            *inv.items.entry(id).or_insert(0) += 10;
        }
        // Send full inventory list (not mini) so player sees everything
        inv.minv = false;
        send_inventory(tx, &mut inv);
        // Then switch back to mini and send again so hotbar appears
        inv.minv = true;
        inv.miniq.clear();
        let mut keys: Vec<i32> = inv.items.keys().copied().collect();
        keys.sort_unstable();
        for k in keys.iter().take(4) {
            inv.miniq.push(*k);
        }
        send_inventory(tx, &mut inv);
        let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
        flags.dirty = true;

        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        s.money = s.money.saturating_add(1_000_000);
        s.creds = s.creds.saturating_add(100_000);
        let (m, c) = (s.money, s.creds);
        let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
        flags.dirty = true;
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(())
    });
}

// ─── /give ──────────────────────────────────────────────────────────────────

fn handle_chat_give_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let item_id = match parse_required_arg::<i32>(tx, args, 0, CMD_USAGE_GIVE) {
        Some(id) => id,
        None => return,
    };
    let amount = match parse_optional_arg_with_default::<i32>(tx, args, 1, 1, CMD_USAGE_GIVE) {
        Some(a) => a,
        None => return,
    };
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        if ecs.get::<PlayerInventory>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none()
        {
            send_command_state_error(tx);
            return Some(());
        }
        {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            *inv.items.entry(item_id).or_insert(0) += amount;
        }
        let mut flags = ecs.get_mut::<PlayerFlags>(entity)?;
        flags.dirty = true;
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        send_inventory(tx, &mut inv);
        Some(())
    });
}

// ─── /money ─────────────────────────────────────────────────────────────────

fn handle_chat_money_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY) {
        Some(a) => a,
        None => return,
    };
    match crate::admin::add_player_money(state, pid, amount) {
        Ok(()) => {}
        Err(
            crate::admin::AdminCommandError::PlayerUnavailable
            | crate::admin::AdminCommandError::MissingPlayerState(_),
        ) => send_command_state_error(tx),
    }
}

// ─── /moneyall ──────────────────────────────────────────────────────────────

async fn handle_chat_money_all_command(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY_ALL) {
        Some(a) => a,
        None => return,
    };
    let count = match state.db.add_money_to_all(amount).await {
        Ok(count) => count,
        Err(e) => {
            tracing::error!(player_id = %pid, amount, error = ?e, "DB add_money_to_all failed");
            send_ok(tx, "Ошибка", "Не удалось выдать деньги всем игрокам");
            return;
        }
    };
    for target_pid in state.active_player_ids() {
        let conn_tx = state.player_sender(target_pid);
        state.modify_player(target_pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none()
            {
                tracing::error!(
                    player_id = %target_pid,
                    "Online moneyall skipped for incomplete player state"
                );
                if let Some(conn_tx) = conn_tx {
                    send_command_state_error(&conn_tx);
                }
                return Some(());
            }
            let mut s = ecs.get_mut::<PlayerStats>(entity)?;
            s.money = s.money.saturating_add(amount);
            let (m, c) = (s.money, s.creds);
            let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
            f.dirty = true;
            if let Some(conn_tx) = conn_tx {
                send_u_packet(&conn_tx, "P$", &money(m, c).1);
            }
            Some(())
        });
    }
    send_ok(
        tx,
        "Банк",
        &format!("Выдано $ {amount} всем игрокам ({count})"),
    );
}

// ─── /skill ────────────────────────────────────────────────────────────────

async fn handle_chat_skill_command(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.eq_ignore_ascii_case("codes") || arg.eq_ignore_ascii_case("help"))
    {
        send_ok(tx, "Скиллы", &admin_skill_codes_help());
        return;
    }
    let (Some(&target_arg), Some(&code), Some(&level_arg)) =
        (args.first(), args.get(1), args.get(2))
    else {
        send_ok(tx, "Скилл", CMD_USAGE_SKILL);
        return;
    };
    let target_pid = match resolve_online_player_arg(state, pid, target_arg) {
        Some(target_pid) => target_pid,
        None => {
            send_ok(tx, "Ошибка", &format!("Игрок '{target_arg}' не в сети"));
            return;
        }
    };
    let Some(skill_type) = SkillType::from_code(code) else {
        send_ok(
            tx,
            "Скилл",
            "Неизвестный wire/DB-код скилла. Примеры: U=геология, M=ход, d=копка, l=HP",
        );
        return;
    };
    let level = match level_arg.parse::<i64>() {
        Ok(level) if (1..=i64::from(i32::MAX)).contains(&level) => {
            i32::try_from(level).expect("validated skill level must fit i32")
        }
        _ => {
            send_ok(
                tx,
                "Скилл",
                &format!("LEVEL должен быть целым числом от 1 до {}", i32::MAX),
            );
            return;
        }
    };
    let slot = match args.get(3).map(|raw| raw.parse::<i32>()) {
        Some(Ok(slot)) if (0..MAX_SKILL_SLOTS).contains(&slot) => Some(slot),
        Some(_) => {
            send_ok(
                tx,
                "Скилл",
                &format!(
                    "SLOT должен быть целым числом от 0 до {}",
                    MAX_SKILL_SLOTS - 1
                ),
            );
            return;
        }
        None => None,
    };
    let exp = match args.get(4).map(|raw| raw.parse::<f32>()) {
        Some(Ok(exp)) if exp > MAX_ADMIN_SKILL_EXP => {
            send_ok(
                tx,
                "Скилл",
                &format!("EXP слишком большое. Максимум: {MAX_ADMIN_SKILL_EXP}"),
            );
            return;
        }
        Some(Ok(exp)) if exp >= 0.0 && exp.is_finite() => exp,
        Some(_) => {
            send_ok(tx, "Скилл", "EXP должен быть конечным числом >= 0");
            return;
        }
        None => 0.0,
    };

    let Some((target_name, chosen_slot, row)) =
        apply_admin_skill_set(state, tx, target_pid, skill_type, level, slot, exp)
    else {
        return;
    };
    if let Err(e) = state.db.save_player(&row).await {
        tracing::error!(player_id = %target_pid, error = ?e, "Failed to write-through save player after /skill");
        send_ok(
            tx,
            "Скилл",
            "Скилл изменён в текущей сессии, но не сохранён в БД.",
        );
        return;
    }
    send_ok(
        tx,
        "Скилл",
        &format!(
            "{}: {} level {} slot {} exp {}",
            target_name,
            skill_type.code(),
            level,
            chosen_slot,
            exp
        ),
    );
}

fn admin_skill_codes_help() -> String {
    SkillType::iter()
        .map(|skill| format!("{}={skill:?}", skill.code()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn resolve_online_player_arg(
    state: &Arc<GameState>,
    self_pid: PlayerId,
    arg: &str,
) -> Option<PlayerId> {
    if arg.eq_ignore_ascii_case("me") || arg == "self" {
        return Some(self_pid);
    }
    if let Ok(pid) = arg.parse::<PlayerId>() {
        return state.is_player_active(pid).then_some(pid);
    }
    state.active_player_ids().into_iter().find(|&candidate| {
        state
            .query_player(candidate, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerMetadata>(entity)
                    .is_some_and(|meta| meta.name.eq_ignore_ascii_case(arg))
            })
            .unwrap_or(false)
    })
}

fn apply_admin_skill_set(
    state: &Arc<GameState>,
    tx: &Outbox,
    target_pid: PlayerId,
    skill_type: SkillType,
    level: i32,
    slot: Option<i32>,
    exp: f32,
) -> Option<(String, i32, PlayerRow)> {
    state
        .modify_player(target_pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if ecs.get::<PlayerSkillsComp>(entity).is_none()
                || ecs.get::<PlayerStats>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
                || ecs
                    .get::<crate::game::player::PlayerMetadata>(entity)
                    .is_none()
            {
                send_command_state_error(tx);
                return None;
            }

            let chosen_slot = {
                let skills = ecs
                    .get::<PlayerSkillsComp>(entity)
                    .expect("PlayerSkillsComp checked before admin skill set");
                choose_admin_skill_slot(&skills.states, skill_type.code(), slot)?
            };
            {
                let mut skills = ecs
                    .get_mut::<PlayerSkillsComp>(entity)
                    .expect("PlayerSkillsComp checked before admin skill mutation");
                skills.states.skills.retain(|&existing_slot, entry| {
                    existing_slot == chosen_slot || entry.code != skill_type.code()
                });
                skills.states.total_slots = skills
                    .states
                    .total_slots
                    .clamp(chosen_slot + 1, MAX_SKILL_SLOTS);
                skills.states.skills.insert(
                    chosen_slot,
                    SkillEntry {
                        code: skill_type.code().to_string(),
                        level,
                        exp,
                    },
                );
            }
            if skill_type == SkillType::Health {
                let max_health = {
                    let skills = ecs
                        .get::<PlayerSkillsComp>(entity)
                        .expect("PlayerSkillsComp checked before admin health recalc");
                    saturating_trunc_f32_to_i32(get_player_skill_effect(
                        &skills.states,
                        SkillType::Health,
                    ))
                };
                let mut player_stats = ecs
                    .get_mut::<PlayerStats>(entity)
                    .expect("PlayerStats checked before admin health recalc");
                player_stats.max_health = max_health;
                player_stats.health = player_stats.health.min(player_stats.max_health).max(1);
            }
            let mut flags = ecs
                .get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before admin skill dirty mark");
            flags.dirty = true;

            let target_name = ecs
                .get::<crate::game::player::PlayerMetadata>(entity)
                .expect("PlayerMetadata checked before admin skill set")
                .name
                .clone();
            let skills = ecs
                .get::<PlayerSkillsComp>(entity)
                .expect("PlayerSkillsComp checked before admin skill sync");
            send_player_skills(tx, skills);
            send_player_level(tx, skills);
            send_player_speed(tx, skills);
            let player_stats = ecs
                .get::<PlayerStats>(entity)
                .expect("PlayerStats checked before admin skill sync");
            send_player_health(tx, player_stats);
            send_player_basket(tx, player_stats, skills);
            let row = crate::game::player::extract_player_row(ecs, entity)
                .expect("Player row checked before admin skill sync");
            Some((target_name, chosen_slot, row))
        })
        .flatten()
}

fn choose_admin_skill_slot(
    skills: &crate::db::SkillSlots,
    code: &str,
    explicit_slot: Option<i32>,
) -> Option<i32> {
    if let Some(slot) = explicit_slot {
        return Some(slot);
    }
    if let Some((&slot, _)) = skills
        .skills
        .iter()
        .find(|(slot, entry)| (0..MAX_SKILL_SLOTS).contains(slot) && entry.code == code)
    {
        return Some(slot);
    }
    (0..skills.total_slots.clamp(0, MAX_SKILL_SLOTS)).find(|slot| !skills.skills.contains_key(slot))
}

// ─── /tp ────────────────────────────────────────────────────────────────────

fn handle_chat_teleport_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let x = match parse_required_arg::<i32>(tx, args, 0, CMD_USAGE_TP) {
        Some(v) => v,
        None => return,
    };
    let y = match parse_required_arg::<i32>(tx, args, 1, CMD_USAGE_TP) {
        Some(v) => v,
        None => return,
    };
    if !state.world.valid_coord(x, y) {
        send_ok(tx, "Ошибка", "Координаты вне карты");
        return;
    }
    let teleported = state
        .modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if ecs
                .get::<crate::game::player::PlayerPosition>(entity)
                .is_none()
                || ecs.get::<crate::game::player::PlayerUI>(entity).is_none()
                || ecs.get::<crate::game::player::PlayerView>(entity).is_none()
                || ecs.get::<PlayerFlags>(entity).is_none()
            {
                send_command_state_error(tx);
                return false;
            }
            let mut pos = ecs
                .get_mut::<crate::game::player::PlayerPosition>(entity)
                .expect("PlayerPosition checked before teleport mutation");
            pos.x = x;
            pos.y = y;
            let mut ui = ecs
                .get_mut::<crate::game::player::PlayerUI>(entity)
                .expect("PlayerUI checked before teleport mutation");
            ui.current_window = None;
            let mut view = ecs
                .get_mut::<crate::game::player::PlayerView>(entity)
                .expect("PlayerView checked before teleport mutation");
            view.last_chunk = None;
            view.visible_chunks.clear();
            let mut f = ecs
                .get_mut::<PlayerFlags>(entity)
                .expect("PlayerFlags checked before teleport mutation");
            f.dirty = true;
            true
        })
        .unwrap_or(false);
    if !teleported {
        return;
    }
    send_u_packet(tx, "@T", &tp(x, y).1);
    check_chunk_changed(state, tx, pid);
}

// ─── /heal ──────────────────────────────────────────────────────────────────

fn handle_chat_heal_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    match crate::admin::heal_player(state, pid) {
        Ok(()) => {}
        Err(
            crate::admin::AdminCommandError::PlayerUnavailable
            | crate::admin::AdminCommandError::MissingPlayerState(_),
        ) => send_command_state_error(tx),
    }
}

// ─── /clan ──────────────────────────────────────────────────────────────────

async fn handle_chat_clan_command(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    args: &[&str],
) {
    let Some((sub, subargs)) = args.split_first() else {
        send_ok(tx, "Клан", CMD_USAGE_CLAN);
        return;
    };
    match *sub {
        "create" => handle_chat_clan_create_command(state, tx, pid, subargs).await,
        "leave" => handle_clan_leave(state, tx, pid).await,
        "kick" => handle_chat_clan_kick_command(state, tx, pid, subargs).await,
        _ => send_ok(tx, "Клан", CMD_USAGE_CLAN),
    }
}

async fn handle_chat_clan_create_command(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    args: &[&str],
) {
    let name = args.first().copied().unwrap_or("");
    let tag = args.get(1).copied().unwrap_or("");
    if name.is_empty() || tag.is_empty() {
        send_ok(tx, "Клан", CMD_USAGE_CLAN_CREATE);
        return;
    }
    handle_clan_create(state, tx, pid, name, tag).await;
}

async fn handle_chat_clan_kick_command(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    args: &[&str],
) {
    let target = args.first().copied().unwrap_or("");
    if target.is_empty() {
        send_ok(tx, "Клан", CMD_USAGE_CLAN_KICK);
        return;
    }
    handle_clan_kick_by_name(state, tx, pid, target).await;
}

// ─── /pack ──────────────────────────────────────────────────────────────────

fn handle_chat_pack_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let Some((sub, subargs)) = args.split_first() else {
        send_ok(tx, "Пак", CMD_USAGE_PACK);
        return;
    };
    match *sub {
        "owner" => handle_pack_owner_command(state, tx, subargs),
        "clan" => handle_pack_clan_command(state, tx, subargs),
        "move" => handle_pack_move_command(state, tx, subargs),
        "type" => handle_pack_type_command(state, tx, subargs),
        _ => send_ok(tx, "Пак", CMD_USAGE_PACK),
    }
}

fn handle_pack_owner_command(state: &Arc<GameState>, tx: &Outbox, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_OWNER, 0, 1) {
        Some(p) => p,
        None => return,
    };
    let owner = parts.get(2).and_then(|s| s.parse().ok());
    if let Some(oid) = owner {
        if modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut o) = ecs.get_mut::<crate::game::buildings::BuildingOwnership>(entity) {
                o.owner_id = oid;
            }
        })
        .is_ok()
        {
            send_ok(tx, "Пак", "Владелец обновлен");
        } else {
            send_ok(tx, "Ошибка", "Здание не найдено");
        }
    }
}

fn handle_pack_clan_command(state: &Arc<GameState>, tx: &Outbox, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_CLAN, 0, 1) {
        Some(p) => p,
        None => return,
    };
    let clan = parts.get(2).and_then(|s| s.parse().ok());
    if let Some(cid) = clan {
        if modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut o) = ecs.get_mut::<crate::game::buildings::BuildingOwnership>(entity) {
                o.clan_id = cid;
            }
        })
        .is_ok()
        {
            send_ok(tx, "Пак", "Клан обновлен");
        } else {
            send_ok(tx, "Ошибка", "Здание не найдено");
        }
    }
}

fn handle_pack_move_command(state: &Arc<GameState>, tx: &Outbox, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_MOVE, 0, 1) {
        Some(p) => p,
        None => return,
    };
    let nx = parts.get(2).and_then(|s| s.parse().ok());
    let ny = parts.get(3).and_then(|s| s.parse().ok());
    if let (Some(nx), Some(ny)) = (nx, ny) {
        // Снимок старого view ДО изменения GridPosition — нужен для переноса клеток
        // футпринта (иначе клетки остаются на старом месте призраками).
        let Some(old_view) = state.get_pack_at(x, y) else {
            send_ok(tx, "Ошибка", "Здание не найдено");
            return;
        };
        if let Err(msg) = validate_pack_footprint(state, &old_view, nx, ny, old_view.pack_type) {
            send_ok(tx, "Ошибка", msg);
            return;
        }
        // GridPosition + DB-строка обновляются в modify_pack_with_db (save новой x/y).
        if modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut p) = ecs.get_mut::<crate::game::buildings::GridPosition>(entity) {
                p.x = nx;
                p.y = ny;
            }
        })
        .is_ok()
        {
            state.move_building_entity(x, y, nx, ny);
            // Перенос МИРОВЫХ КЛЕТОК футпринта на новую позицию — закрывает
            // рассинхрон «index/ECS/DB на новом месте, а клетки на старом».
            crate::net::session::social::buildings::move_pack_cells(state, &old_view, nx, ny);
            send_ok(tx, "Пак", "Позиция обновлена");
        } else {
            send_ok(tx, "Ошибка", "Здание не найдено");
        }
    }
}

fn handle_pack_type_command(state: &Arc<GameState>, tx: &Outbox, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_TYPE, 0, 1) {
        Some(p) => p,
        None => return,
    };
    let t = parts
        .get(2)
        .and_then(|&s| crate::game::buildings::PackType::from_str(s));
    if let Some(nt) = t {
        let ex = match building_extra_for_pack_type(nt) {
            Ok(extra) => extra,
            Err(e) => {
                tracing::error!(?nt, error = ?e, "Missing building config for pack type command");
                send_ok(tx, "Ошибка", "Конфиг здания не найден");
                return;
            }
        };
        if modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut m) = ecs.get_mut::<crate::game::buildings::BuildingMetadata>(entity) {
                m.pack_type = nt;
            }
            if let Some(mut s) = ecs.get_mut::<crate::game::buildings::BuildingStats>(entity) {
                s.max_charge = ex.max_charge;
                s.charge = s.charge.min(ex.max_charge);
                s.max_hp = ex.max_hp;
                s.hp = s.hp.min(ex.max_hp);
            }
        })
        .is_ok()
        {
            send_ok(tx, "Пак", "Тип обновлен");
        } else {
            send_ok(tx, "Ошибка", "Здание не найдено");
        }
    }
}

// ─── /kick ──────────────────────────────────────────────────────────────────

fn handle_chat_kick_command(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let Some(&target_name) = args.first() else {
        send_ok(tx, "Кик", CMD_USAGE_KICK);
        return;
    };
    let target_pid = state.active_player_ids().into_iter().find_map(|tid| {
        state
            .query_player(tid, |ecs, entity| {
                ecs.get::<crate::game::player::PlayerMetadata>(entity)
                    .filter(|m| m.name.eq_ignore_ascii_case(target_name))
                    .map(|_| tid)
            })
            .flatten()
    });
    match target_pid {
        Some(tid) => {
            if state.kick_player(tid) {
                send_ok(tx, "Кик", &format!("Игрок {target_name} кикнут"));
            } else {
                send_ok(tx, "Ошибка", "Не удалось кикнуть игрока");
            }
        }
        None => send_ok(tx, "Ошибка", &format!("Игрок '{target_name}' не в сети")),
    }
}

// ─── /role ──────────────────────────────────────────────────────────────────

async fn handle_chat_role_command(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let (Some(&target_name), Some(&role_arg)) = (args.first(), args.get(1)) else {
        send_ok(tx, "Роль", CMD_USAGE_ROLE);
        return;
    };
    let role = match role_arg {
        "admin" => Role::Admin,
        "mod" | "moderator" => Role::Moderator,
        "player" => Role::Player,
        _ => {
            send_ok(tx, "Ошибка", "Роль: admin|mod|player");
            return;
        }
    };
    match state.db.get_player_by_name(target_name).await {
        Ok(Some(row)) => match state.db.set_player_role(row.id, role).await {
            Ok(true) => {
                state.modify_player(row.id.into(), |ecs, entity| {
                    if let Some(mut s) = ecs.get_mut::<PlayerStats>(entity) {
                        s.role = role as i32;
                    }
                    Some(())
                });
                let role_name = match role {
                    Role::Admin => "Admin",
                    Role::Moderator => "Mod",
                    Role::Player => "Player",
                };
                send_ok(
                    tx,
                    "Роль",
                    &format!("Игроку {} установлена роль {}", row.name, role_name),
                );
            }
            Ok(false) => send_ok(tx, "Ошибка", "Игрок не найден в БД"),
            Err(e) => send_ok(tx, "Ошибка", &e.to_string()),
        },
        Ok(None) => send_ok(tx, "Ошибка", &format!("Игрок '{target_name}' не найден")),
        Err(e) => send_ok(tx, "Ошибка", &e.to_string()),
    }
}

// ─── Parsing helpers ────────────────────────────────────────────────────────

fn parse_required_arg<T: std::str::FromStr>(
    tx: &Outbox,
    args: &[&str],
    idx: usize,
    usage: &str,
) -> Option<T> {
    args.get(idx).and_then(|s| s.parse().ok()).or_else(|| {
        send_ok(tx, "Ошибка", usage);
        None
    })
}

fn parse_optional_arg_with_default<T: std::str::FromStr>(
    tx: &Outbox,
    args: &[&str],
    idx: usize,
    def: T,
    usage: &str,
) -> Option<T> {
    args.get(idx)
        .map(|s| {
            s.parse().ok().or_else(|| {
                send_ok(tx, "Ошибка", usage);
                None
            })
        })
        .unwrap_or(Some(def))
}

fn parse_pack_pos(
    parts: &[&str],
    tx: &Outbox,
    usage: &str,
    xi: usize,
    yi: usize,
) -> Option<(i32, i32)> {
    let x = parts.get(xi).and_then(|s| s.parse().ok());
    let y = parts.get(yi).and_then(|s| s.parse().ok());
    match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => {
            send_ok(tx, "Пак", usage);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::player::{PlayerFlags, PlayerPosition};
    use crate::test_support::{ServerTestHarness, drain_events};

    async fn make_command_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(&format!("commands_{label}"), "command-user").await
    }

    fn make_admin_and_remove_flags(game_state: &Arc<GameState>, pid: PlayerId) {
        let entity = game_state.get_player_entity(pid).unwrap();
        let mut ecs = game_state.ecs.write();
        let mut admin_stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
        admin_stats.role = 2;
        ecs.entity_mut(entity).remove::<PlayerFlags>();
    }

    fn make_admin(game_state: &Arc<GameState>, pid: PlayerId) {
        let entity = game_state.get_player_entity(pid).unwrap();
        let mut ecs = game_state.ecs.write();
        let mut admin_stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
        admin_stats.role = 2;
    }

    fn player_money(game_state: &Arc<GameState>, pid: PlayerId) -> i64 {
        game_state
            .query_player_opt(pid, |ecs, entity| {
                let money_stats = ecs.get::<PlayerStats>(entity)?;
                Some(money_stats.money)
            })
            .unwrap()
    }

    fn player_pos(game_state: &Arc<GameState>, pid: PlayerId) -> (i32, i32) {
        game_state
            .query_player_opt(pid, |ecs, entity| {
                let pos = ecs.get::<PlayerPosition>(entity)?;
                Some((pos.x, pos.y))
            })
            .unwrap()
    }

    fn player_skill_entry(
        game_state: &Arc<GameState>,
        pid: PlayerId,
        slot: i32,
    ) -> Option<SkillEntry> {
        game_state.query_player_opt(pid, |ecs, entity| {
            let skills = ecs.get::<PlayerSkillsComp>(entity)?;
            skills.states.skills.get(&slot).cloned()
        })
    }

    fn player_skill_count(game_state: &Arc<GameState>, pid: PlayerId, code: &str) -> usize {
        game_state
            .query_player_opt(pid, |ecs, entity| {
                let skills = ecs.get::<PlayerSkillsComp>(entity)?;
                Some(
                    skills
                        .states
                        .skills
                        .values()
                        .filter(|entry| entry.code == code)
                        .count(),
                )
            })
            .unwrap()
    }

    #[tokio::test]
    async fn skill_sets_wire_code_slot_and_syncs_player_packets() {
        let test = make_command_test_state("skill_set").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin(&test.state, pid);

        handle_chat_skill_command(&test.state, &tx, pid, &["me", "U", "200", "10", "900000"]).await;

        let entry = player_skill_entry(&test.state, pid, 10).unwrap();
        assert_eq!(entry.code, SkillType::Geology.code());
        assert_eq!(entry.level, 200);
        assert!((entry.exp - 900_000.0).abs() < f32::EPSILON);
        assert_eq!(
            player_skill_count(&test.state, pid, SkillType::Geology.code()),
            1
        );
        let saved = test
            .state
            .db
            .get_player_by_id(test.player.id)
            .await
            .unwrap()
            .unwrap();
        let saved_entry = saved.skills.skills.get(&10).unwrap();
        assert_eq!(saved_entry.code, SkillType::Geology.code());
        assert_eq!(saved_entry.level, 200);
        assert!((saved_entry.exp - 900_000.0).abs() < f32::EPSILON);

        let events = drain_events(&mut rx);
        assert!(events.iter().any(|(event, _)| event == "@S"));
        assert!(events.iter().any(|(event, _)| event == "LV"));
        assert!(events.iter().any(|(event, _)| event == "sp"));
        assert!(events.iter().any(|(event, _)| event == "@L"));
        assert!(events.iter().any(|(event, _)| event == "@B"));
        assert_eq!(events.last().unwrap().0, "OK");
    }

    #[tokio::test]
    async fn skill_exp_above_wire_limit_is_rejected_before_mutation() {
        let test = make_command_test_state("skill_max_exp").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin(&test.state, pid);
        let command = format!("/skill me U 200 10 {}", f32::MAX);

        handle_chat_command(&test.state, &tx, pid, &command).await;

        assert!(player_skill_entry(&test.state, pid, 10).is_none());
        let saved = test
            .state
            .db
            .get_player_by_id(test.player.id)
            .await
            .unwrap()
            .unwrap();
        assert!(!saved.skills.skills.contains_key(&10));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("EXP слишком большое"));
        assert!(message.contains("21474834"));
    }

    #[tokio::test]
    async fn skill_rejects_slots_outside_domain_before_mutation() {
        let test = make_command_test_state("skill_slot_limit").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin(&test.state, pid);
        handle_chat_command(&test.state, &tx, pid, "/skill me U 200 34 1").await;

        assert!(player_skill_entry(&test.state, pid, 34).is_none());
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("SLOT должен быть целым числом от 0 до 33"));
    }

    #[tokio::test]
    async fn skill_unknown_code_is_explicit_wire_error_without_mutation() {
        let test = make_command_test_state("skill_unknown").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin(&test.state, pid);

        handle_chat_skill_command(&test.state, &tx, pid, &["me", "GEO", "200", "10", "900000"])
            .await;

        assert!(player_skill_entry(&test.state, pid, 10).is_none());
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Неизвестный wire/DB-код скилла"));
        assert!(message.contains("U=геология"));
    }

    #[tokio::test]
    async fn skill_codes_help_is_generated_from_wire_codes() {
        let test = make_command_test_state("skill_codes").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin(&test.state, pid);

        handle_chat_skill_command(&test.state, &tx, pid, &["codes"]).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("U=Geology"));
        assert!(message.contains("M=Movement"));
        assert!(message.contains("d=Digging"));
        assert!(!message.contains("GEO="));
    }

    #[tokio::test]
    async fn skill_missing_flags_is_explicit_error_without_skill_mutation() {
        let test = make_command_test_state("skill_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin_and_remove_flags(&test.state, pid);

        handle_chat_skill_command(&test.state, &tx, pid, &["me", "U", "200", "10", "900000"]).await;

        assert!(player_skill_entry(&test.state, pid, 10).is_none());
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn money_missing_flags_is_explicit_error_without_money_mutation() {
        let test = make_command_test_state("money_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin_and_remove_flags(&test.state, pid);
        let before_money = player_money(&test.state, pid);

        handle_chat_money_command(&test.state, &tx, pid, &["50"]);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_money(&test.state, pid), before_money);
    }

    #[tokio::test]
    async fn teleport_missing_flags_is_explicit_error_without_tp_packet_or_position_mutation() {
        let test = make_command_test_state("tp_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        make_admin_and_remove_flags(&test.state, pid);
        let before_pos = player_pos(&test.state, pid);

        handle_chat_teleport_command(&test.state, &tx, pid, &["12", "12"]);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
        assert_eq!(player_pos(&test.state, pid), before_pos);
    }
}
