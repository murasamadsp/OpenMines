//! Слэш-команды чата: /give, /money, /tp, /heal, /kick, /role, /clan, /pack, /admin.
use crate::db::players::Role;
use crate::game::player::{PlayerFlags, PlayerInventory, PlayerStats};
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{building_extra_for_pack_type, modify_pack_with_db};
use crate::net::session::social::clans::{
    handle_clan_create, handle_clan_kick_by_name, handle_clan_leave,
};

const ADMIN_COMMAND_HELP: &str = concat!(
    "Админские команды:\n",
    "/give ITEM_ID AMOUNT — выдать предмет\n",
    "/giveall — выдать все предметы (по 10 шт.)\n",
    "/money AMOUNT — добавить денег\n",
    "/moneyall AMOUNT — добавить денег всем игрокам\n",
    "/tp X Y — телепортироваться\n",
    "/heal — восстановить HP\n",
    "/kick ИМЯ — кикнуть игрока\n",
    "/role ИМЯ admin|mod|player — установить роль\n",
    "/pack owner X Y OWNER_ID — сменить владельца здания\n",
    "/pack clan X Y CLAN_ID — изменить клан здания\n",
    "/pack move X Y NX NY — переместить здание\n",
    "/pack type X Y TYPE — сменить тип здания (T/R/G/M/U/L/F/O)\n",
    "/admin — показать справку по админ-командам",
);

const CMD_USAGE_GIVE: &str = "Использование: /give ITEM_ID AMOUNT";
const CMD_USAGE_MONEY: &str = "Использование: /money AMOUNT";
const CMD_USAGE_MONEY_ALL: &str = "Использование: /moneyall AMOUNT";
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

pub fn send_ok(tx: &mpsc::UnboundedSender<Vec<u8>>, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}

pub fn send_admin_help(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_ok(tx, "Админ-команды", ADMIN_COMMAND_HELP);
}

fn send_command_state_error(tx: &mpsc::UnboundedSender<Vec<u8>>) {
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

fn ensure_admin(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    state: &Arc<GameState>,
    pid: PlayerId,
) -> bool {
    if is_admin_command(state, pid) {
        true
    } else {
        send_ok(tx, "Ошибка", ADMIN_COMMAND_NO_RIGHTS);
        false
    }
}

// ─── Main dispatcher ────────────────────────────────────────────────────────

pub async fn handle_chat_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
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
    match cmd {
        "/give" => handle_chat_give_command(state, tx, pid, args),
        "/giveall" => handle_chat_giveall_command(state, tx, pid),
        "/money" => handle_chat_money_command(state, tx, pid, args),
        "/moneyall" => handle_chat_money_all_command(state, tx, pid, args).await,
        "/tp" => handle_chat_teleport_command(state, tx, pid, args),
        "/heal" => handle_chat_heal_command(state, tx, pid),
        "/kick" => handle_chat_kick_command(state, tx, pid, args),
        "/role" => handle_chat_role_command(state, tx, pid, args).await,
        "/clan" => handle_chat_clan_command(state, tx, pid, args).await,
        "/pack" => handle_chat_pack_command(state, tx, pid, args),
        "/admin" | "/adminhelp" => {
            if ensure_admin(tx, state, pid) {
                send_admin_help(tx);
            }
        }
        _ => {
            if is_admin_command(state, pid) {
                send_admin_help(tx);
            } else {
                send_ok(tx, "Ошибка", &format!("Неизвестная команда: {cmd}"));
            }
        }
    }
}

// ─── /giveall ───────────────────────────────────────────────────────────────

fn handle_chat_giveall_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
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

fn handle_chat_give_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
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

fn handle_chat_money_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY) {
        Some(a) => a,
        None => return,
    };
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none() {
            send_command_state_error(tx);
            return Some(());
        }
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        s.money += amount;
        let (m, c) = (s.money, s.creds);
        let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
        f.dirty = true;
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(())
    });
}

// ─── /moneyall ──────────────────────────────────────────────────────────────

async fn handle_chat_money_all_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    for entry in &state.active_players {
        state.modify_player(
            *entry.key(),
            |ecs: &mut bevy_ecs::prelude::World, entity| {
                let conn_tx = ecs
                    .get::<crate::game::player::PlayerConnection>(entity)
                    .map(|conn| conn.tx.clone());
                if ecs.get::<PlayerStats>(entity).is_none()
                    || ecs.get::<PlayerFlags>(entity).is_none()
                {
                    tracing::error!(
                        player_id = %entry.key(),
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
            },
        );
    }
    send_ok(
        tx,
        "Банк",
        &format!("Выдано $ {amount} всем игрокам ({count})"),
    );
}

// ─── /tp ────────────────────────────────────────────────────────────────────

fn handle_chat_teleport_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
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

fn handle_chat_heal_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        if ecs.get::<PlayerStats>(entity).is_none() || ecs.get::<PlayerFlags>(entity).is_none() {
            send_command_state_error(tx);
            return Some(());
        }
        let mut s = ecs.get_mut::<PlayerStats>(entity)?;
        s.health = s.max_health;
        let (h, mh) = (s.health, s.max_health);
        let mut f = ecs.get_mut::<PlayerFlags>(entity)?;
        f.dirty = true;
        send_u_packet(tx, "@L", &health(h, mh).1);
        Some(())
    });
}

// ─── /clan ──────────────────────────────────────────────────────────────────

async fn handle_chat_clan_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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

fn handle_chat_pack_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
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

fn handle_pack_owner_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
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

fn handle_pack_clan_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
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

fn handle_pack_move_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_MOVE, 0, 1) {
        Some(p) => p,
        None => return,
    };
    let nx = parts.get(2).and_then(|s| s.parse().ok());
    let ny = parts.get(3).and_then(|s| s.parse().ok());
    if let (Some(nx), Some(ny)) = (nx, ny) {
        // Снимок старого view ДО изменения GridPosition — нужен для переноса клеток
        // футпринта (иначе клетки остаются на старом месте призраками).
        let old_view = state.get_pack_at(x, y);
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
            if let Some(ov) = old_view {
                crate::net::session::social::buildings::move_pack_cells(state, &ov, nx, ny);
            }
            send_ok(tx, "Пак", "Позиция обновлена");
        } else {
            send_ok(tx, "Ошибка", "Здание не найдено");
        }
    }
}

fn handle_pack_type_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
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

fn handle_chat_kick_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let Some(&target_name) = args.first() else {
        send_ok(tx, "Кик", CMD_USAGE_KICK);
        return;
    };
    let target_pid = state.active_players.iter().find_map(|entry| {
        let tid = *entry.key();
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
            if state.kick_channels.remove(&tid).is_some() {
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    tx: &mpsc::UnboundedSender<Vec<u8>>,
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
    use crate::db::players::PlayerRow;
    use crate::game::player::{PlayerFlags, PlayerPosition};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct CommandTestState {
        state: Arc<GameState>,
        player: PlayerRow,
        db_path: PathBuf,
        world_name: String,
        dir: PathBuf,
    }

    impl CommandTestState {
        fn cleanup(&self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(
                self.dir
                    .join(format!("{}_durability.mapb", self.world_name)),
            );
        }
    }

    async fn make_command_test_state(label: &str) -> CommandTestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!(
            "commands_{label}_{}_{}.db",
            std::process::id(),
            nonce
        ));
        let _ = std::fs::remove_file(&db_path);

        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database
            .create_player("command-user", "p", "h")
            .await
            .unwrap();

        let cell_defs =
            crate::world::cells::CellDefs::load(crate::test_config_path("configs/cells.json"))
                .unwrap();
        let world_name = format!("commands_world_{label}_{}_{}", std::process::id(), nonce);
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

        CommandTestState {
            state,
            player,
            db_path,
            world_name,
            dir,
        }
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = bytes::BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    fn make_admin_and_remove_flags(game_state: &Arc<GameState>, pid: PlayerId) {
        let entity = game_state.get_player_entity(pid).unwrap();
        let mut ecs = game_state.ecs.write();
        let mut admin_stats = ecs.get_mut::<PlayerStats>(entity).unwrap();
        admin_stats.role = 2;
        ecs.entity_mut(entity).remove::<PlayerFlags>();
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

    #[tokio::test]
    async fn money_missing_flags_is_explicit_error_without_money_mutation() {
        let test = make_command_test_state("money_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
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

        test.cleanup();
    }

    #[tokio::test]
    async fn teleport_missing_flags_is_explicit_error_without_tp_packet_or_position_mutation() {
        let test = make_command_test_state("tp_missing_flags").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
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

        test.cleanup();
    }
}
