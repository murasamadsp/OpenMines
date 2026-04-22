//! Слэш-команды чата: /give, /money, /tp, /heal, /clan, /pack, /admin.
use crate::game::player::PlayerInventory;
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
    "/money AMOUNT — добавить денег игроку\n",
    "/moneyall AMOUNT — добавить денег всем игрокам\n",
    "/tp X Y — телепортироваться\n",
    "/heal — восстановить HP\n",
    "/pack owner X Y OWNER_ID — сменить владельца здания\n",
    "/pack clan X Y CLAN_ID — изменить клан здания\n",
    "/pack move X Y NX NY — переместить здание\n",
    "/pack type X Y TYPE — сменить тип здания (T/R/G/M/U/L/F/O)\n",
    "/admin — показать справку по админ-командам\n",
    "/adminhelp — показать справку по админ-командам",
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
const CMD_USAGE_CLAN_CREATE: &str = "Использование: /clan create ИМЯ ТЕГ";
const CMD_USAGE_CLAN_KICK: &str = "Использование: /clan kick ИМЯ";
const CMD_USAGE_CLAN: &str = "Команды: /clan create ИМЯ ТЕГ | /clan leave | /clan kick ИМЯ";

const ADMIN_COMMAND_NO_RIGHTS: &str = "Нет прав на админ-команду";

// ─── Shared helpers ─────────────────────────────────────────────────────────

pub(crate) fn send_ok(tx: &mpsc::UnboundedSender<Vec<u8>>, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}

pub(crate) fn send_admin_help(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_ok(tx, "Админ-команды", ADMIN_COMMAND_HELP);
}

pub(crate) fn is_admin_command(state: &Arc<GameState>, pid: PlayerId) -> bool {
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

pub fn handle_chat_command(
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
    match cmd {
        "/give" => handle_chat_give_command(state, tx, pid, args),
        "/money" => handle_chat_money_command(state, tx, pid, args),
        "/moneyall" => handle_chat_money_all_command(state, tx, pid, args),
        "/tp" => handle_chat_teleport_command(state, tx, pid, args),
        "/heal" => handle_chat_heal_command(state, tx, pid),
        "/clan" => handle_chat_clan_command(state, tx, pid, args),
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
        {
            let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
            *inv.items.entry(item_id).or_insert(0) += amount;
        }
        if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            flags.dirty = true;
        }
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
        let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
        s.money += amount;
        let (m, c) = (s.money, s.creds);
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            f.dirty = true;
        }
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(())
    });
}

// ─── /moneyall ──────────────────────────────────────────────────────────────

fn handle_chat_money_all_command(
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
    if let Ok(count) = state.db.add_money_to_all(amount) {
        for entry in &state.active_players {
            state.modify_player(
                *entry.key(),
                |ecs: &mut bevy_ecs::prelude::World, entity| {
                    let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                    s.money = s.money.saturating_add(amount);
                    let (m, c) = (s.money, s.creds);
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        send_u_packet(&conn.tx, "P$", &money(m, c).1);
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
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        let mut pos = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
        pos.x = x;
        pos.y = y;
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
            ui.current_window = None;
        }
        if let Some(mut view) = ecs.get_mut::<crate::game::player::PlayerView>(entity) {
            view.last_chunk = None;
            view.visible_chunks.clear();
        }
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            f.dirty = true;
        }
        Some(())
    });
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
        let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
        s.health = s.max_health;
        let (h, mh) = (s.health, s.max_health);
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            f.dirty = true;
        }
        send_u_packet(tx, "@L", &health(h, mh).1);
        Some(())
    });
}

// ─── /clan ──────────────────────────────────────────────────────────────────

fn handle_chat_clan_command(
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
        "create" => handle_chat_clan_create_command(state, tx, pid, subargs),
        "leave" => handle_clan_leave(state, tx, pid),
        "kick" => handle_chat_clan_kick_command(state, tx, pid, subargs),
        _ => send_ok(tx, "Клан", CMD_USAGE_CLAN),
    }
}

fn handle_chat_clan_create_command(
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
    handle_clan_create(state, tx, pid, name, tag);
}

fn handle_chat_clan_kick_command(
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
    handle_clan_kick_by_name(state, tx, pid, target);
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
        // NOTE: Move requires updating building_index, but modify_pack_with_db doesn't handle it yet.
        // For now let's just update GridPosition and hope for the best or implement full move logic.
        if modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut p) = ecs.get_mut::<crate::game::buildings::GridPosition>(entity) {
                p.x = nx;
                p.y = ny;
            }
        })
        .is_ok()
        {
            if let Some((_, entity)) = state.building_index.remove(&(x, y)) {
                state.building_index.insert((nx, ny), entity);
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
        let ex = building_extra_for_pack_type(nt);
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
